//! Correctness fixtures and workload traces for Crucible world-access performance qualification.
//!
//! The benchmark compares two routing mechanisms over the exact same immutable live chunks and
//! query streams. This crate is qualification infrastructure, not production world storage.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use crucible_generated::{BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts};
use crucible_types::{BlockPos, ChunkGeneration, ChunkPos};
use crucible_world_chunk::{
    ChunkCoreError, LiveChunkCore, ResolvedChunkWindow, ResolvedChunkWindowError,
};
use crucible_world_reference::DirectBlockSection;

const BLOCKS_PER_CHUNK_AXIS: i32 = 16;
const STANDARD_MIN_SECTION_Y: i32 = -4;
const STANDARD_SECTION_COUNT: usize = 24;

/// Concrete reference chunk used to isolate routing cost from section-representation choice.
pub type BenchChunk = LiveChunkCore<BlockStateId, DirectBlockSection<BlockStateId>>;

/// Fail-closed fixture/trace/read errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorldAccessError {
    InvalidExtent,
    CoordinateOverflow,
    InvalidStateIdentity,
    EmptyTrace,
    MissingReferenceChunk { position: ChunkPos },
    SemanticMismatch { reference: u64, resolved: u64 },
    Chunk(ChunkCoreError),
    Window(ResolvedChunkWindowError),
}

impl From<ChunkCoreError> for WorldAccessError {
    fn from(value: ChunkCoreError) -> Self {
        Self::Chunk(value)
    }
}

impl From<ResolvedChunkWindowError> for WorldAccessError {
    fn from(value: ResolvedChunkWindowError) -> Self {
        Self::Window(value)
    }
}

/// Shape of one precomputed world-access trace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkloadKind {
    RandomPoint,
    CollisionSweep,
    PathfindingWalk,
    Streaming,
}

/// Immutable description of one benchmark case.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaseSpec {
    pub name: &'static str,
    pub origin: ChunkPos,
    pub width: usize,
    pub depth: usize,
    pub kind: WorkloadKind,
}

/// Exact live world and query stream shared by every routing candidate in a benchmark case.
#[derive(Debug)]
pub struct PreparedCase {
    spec: CaseSpec,
    chunks: Vec<BenchChunk>,
    trace: Vec<BlockPos>,
}

impl PreparedCase {
    /// Builds the live chunks and deterministic query trace for one case.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid/overflowing extent, target-state identity failure, invalid
    /// live chunk lattice, or an empty trace.
    pub fn new(spec: CaseSpec, query_count: usize) -> Result<Self, WorldAccessError> {
        if spec.width == 0 || spec.depth == 0 || query_count == 0 {
            return Err(if query_count == 0 {
                WorldAccessError::EmptyTrace
            } else {
                WorldAccessError::InvalidExtent
            });
        }
        checked_block_bounds(spec)?;

        let mut chunks = Vec::with_capacity(
            spec.width
                .checked_mul(spec.depth)
                .ok_or(WorldAccessError::InvalidExtent)?,
        );
        for z_offset in 0..spec.depth {
            for x_offset in 0..spec.width {
                let x_offset =
                    i32::try_from(x_offset).map_err(|_| WorldAccessError::CoordinateOverflow)?;
                let z_offset =
                    i32::try_from(z_offset).map_err(|_| WorldAccessError::CoordinateOverflow)?;
                let position = ChunkPos {
                    x: spec
                        .origin
                        .x
                        .checked_add(x_offset)
                        .ok_or(WorldAccessError::CoordinateOverflow)?,
                    z: spec
                        .origin
                        .z
                        .checked_add(z_offset)
                        .ok_or(WorldAccessError::CoordinateOverflow)?,
                };
                chunks.push(build_chunk(position)?);
            }
        }

        let trace = build_trace(spec, query_count)?;
        Ok(Self {
            spec,
            chunks,
            trace,
        })
    }

    #[must_use]
    pub const fn spec(&self) -> CaseSpec {
        self.spec
    }

    #[must_use]
    pub fn trace(&self) -> &[BlockPos] {
        &self.trace
    }

    #[must_use]
    pub fn chunks(&self) -> &[BenchChunk] {
        &self.chunks
    }

    /// Builds the conventional global point-routing baseline once.
    #[must_use]
    pub fn reference_router(&self) -> ReferenceRouter<'_> {
        ReferenceRouter::new(self.chunks.iter())
    }

    /// Performs the candidate's one-time rectangular boundary resolution.
    ///
    /// # Errors
    ///
    /// Propagates fail-closed resolved-window construction errors.
    pub fn resolved_window(
        &self,
    ) -> Result<
        ResolvedChunkWindow<'_, BlockStateId, DirectBlockSection<BlockStateId>>,
        WorldAccessError,
    > {
        Ok(ResolvedChunkWindow::new(
            self.spec.origin,
            self.spec.width,
            self.spec.depth,
            self.chunks.iter(),
        )?)
    }

    /// Proves both routing mechanisms produce the same exact checksum on this trace.
    ///
    /// # Errors
    ///
    /// Returns any routing/access failure or a checksum mismatch rather than normalizing an
    /// evidence failure during a benchmark case.
    pub fn validate_equivalence(&self) -> Result<u64, WorldAccessError> {
        let reference = self.reference_router();
        let window = self.resolved_window()?;
        let reference_checksum =
            checksum(self.trace.iter().copied(), |pos| reference.get_block(pos))?;
        let resolved_checksum = checksum(self.trace.iter().copied(), |pos| {
            window.get_block(pos).map_err(WorldAccessError::from)
        })?;
        if reference_checksum != resolved_checksum {
            return Err(WorldAccessError::SemanticMismatch {
                reference: reference_checksum,
                resolved: resolved_checksum,
            });
        }
        Ok(reference_checksum)
    }
}

/// Conventional repeated global point-routing baseline.
#[derive(Debug)]
pub struct ReferenceRouter<'a> {
    chunks: HashMap<ChunkPos, &'a BenchChunk>,
}

impl<'a> ReferenceRouter<'a> {
    #[must_use]
    pub fn new(chunks: impl IntoIterator<Item = &'a BenchChunk>) -> Self {
        Self {
            chunks: chunks
                .into_iter()
                .map(|chunk| (chunk.position(), chunk))
                .collect(),
        }
    }

    /// Resolves world X/Z through the reference directory and reads the selected live chunk.
    ///
    /// # Errors
    ///
    /// Returns an error for missing horizontal coverage or an invalid live-chunk vertical access.
    pub fn get_block(&self, pos: BlockPos) -> Result<BlockStateId, WorldAccessError> {
        let position = ChunkPos {
            x: pos.x.div_euclid(BLOCKS_PER_CHUNK_AXIS),
            z: pos.z.div_euclid(BLOCKS_PER_CHUNK_AXIS),
        };
        let chunk = self
            .chunks
            .get(&position)
            .copied()
            .ok_or(WorldAccessError::MissingReferenceChunk { position })?;
        Ok(chunk.get_block(pos)?)
    }
}

/// Official smoke cases. These remain small enough for hosted CI timing diagnostics.
#[must_use]
pub fn smoke_cases() -> Vec<CaseSpec> {
    vec![
        CaseSpec {
            name: "random-1x1-positive",
            origin: ChunkPos { x: 4, z: 4 },
            width: 1,
            depth: 1,
            kind: WorkloadKind::RandomPoint,
        },
        CaseSpec {
            name: "collision-3x3-signed",
            origin: ChunkPos { x: -1, z: -1 },
            width: 3,
            depth: 3,
            kind: WorkloadKind::CollisionSweep,
        },
        CaseSpec {
            name: "pathfinding-5x5-negative",
            origin: ChunkPos { x: -8, z: -8 },
            width: 5,
            depth: 5,
            kind: WorkloadKind::PathfindingWalk,
        },
    ]
}

/// Full target-hardware workload matrix. Hosted CI must not be used to select a winner from it.
#[must_use]
pub fn full_cases() -> Vec<CaseSpec> {
    vec![
        CaseSpec {
            name: "random-1x1-positive",
            origin: ChunkPos { x: 4, z: 4 },
            width: 1,
            depth: 1,
            kind: WorkloadKind::RandomPoint,
        },
        CaseSpec {
            name: "random-1x1-negative",
            origin: ChunkPos { x: -5, z: -5 },
            width: 1,
            depth: 1,
            kind: WorkloadKind::RandomPoint,
        },
        CaseSpec {
            name: "collision-3x3-signed",
            origin: ChunkPos { x: -1, z: -1 },
            width: 3,
            depth: 3,
            kind: WorkloadKind::CollisionSweep,
        },
        CaseSpec {
            name: "collision-3x3-negative",
            origin: ChunkPos { x: -12, z: -9 },
            width: 3,
            depth: 3,
            kind: WorkloadKind::CollisionSweep,
        },
        CaseSpec {
            name: "pathfinding-5x5-positive",
            origin: ChunkPos { x: 7, z: 5 },
            width: 5,
            depth: 5,
            kind: WorkloadKind::PathfindingWalk,
        },
        CaseSpec {
            name: "pathfinding-5x5-negative",
            origin: ChunkPos { x: -8, z: -8 },
            width: 5,
            depth: 5,
            kind: WorkloadKind::PathfindingWalk,
        },
        CaseSpec {
            name: "streaming-9x9-positive",
            origin: ChunkPos { x: 20, z: 20 },
            width: 9,
            depth: 9,
            kind: WorkloadKind::Streaming,
        },
        CaseSpec {
            name: "streaming-9x9-negative",
            origin: ChunkPos { x: -29, z: -29 },
            width: 9,
            depth: 9,
            kind: WorkloadKind::Streaming,
        },
    ]
}

fn build_chunk(position: ChunkPos) -> Result<BenchChunk, WorldAccessError> {
    let sections = (0..STANDARD_SECTION_COUNT)
        .map(|section_index| {
            Ok(DirectBlockSection::filled(
                state_for(position, section_index)?,
                &GeneratedStateFacts,
            ))
        })
        .collect::<Result<Vec<_>, WorldAccessError>>()?;
    Ok(LiveChunkCore::new(
        position,
        ChunkGeneration(1),
        STANDARD_MIN_SECTION_Y,
        sections,
    )?)
}

fn state_for(position: ChunkPos, section_index: usize) -> Result<BlockStateId, WorldAccessError> {
    let section_index =
        u64::try_from(section_index).map_err(|_| WorldAccessError::InvalidStateIdentity)?;
    let x = i64::from(position.x).unsigned_abs();
    let z = i64::from(position.z).unsigned_abs();
    let mixed = x
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(z.wrapping_mul(0x85EB_CA77))
        .wrapping_add(section_index.wrapping_mul(0xC2B2_AE3D));
    let non_air_universe =
        u64::try_from(BLOCK_STATE_COUNT - 1).map_err(|_| WorldAccessError::InvalidStateIdentity)?;
    let raw = u32::try_from(mixed % non_air_universe + 1)
        .map_err(|_| WorldAccessError::InvalidStateIdentity)?;
    BlockStateId::new(raw).ok_or(WorldAccessError::InvalidStateIdentity)
}

fn build_trace(spec: CaseSpec, query_count: usize) -> Result<Vec<BlockPos>, WorldAccessError> {
    match spec.kind {
        WorkloadKind::RandomPoint => random_trace(spec, query_count),
        WorkloadKind::CollisionSweep => collision_trace(spec, query_count),
        WorkloadKind::PathfindingWalk => pathfinding_trace(spec, query_count),
        WorkloadKind::Streaming => streaming_trace(spec, query_count),
    }
}

fn random_trace(spec: CaseSpec, query_count: usize) -> Result<Vec<BlockPos>, WorldAccessError> {
    let bounds = checked_block_bounds(spec)?;
    let mut rng = Rng::new(case_seed(spec));
    let mut trace = Vec::with_capacity(query_count);
    for _ in 0..query_count {
        trace.push(BlockPos {
            x: rng.i32_in(bounds.min_x, bounds.max_x_exclusive),
            y: rng.i32_in(-64, 320),
            z: rng.i32_in(bounds.min_z, bounds.max_z_exclusive),
        });
    }
    Ok(trace)
}

fn collision_trace(spec: CaseSpec, query_count: usize) -> Result<Vec<BlockPos>, WorldAccessError> {
    let bounds = checked_block_bounds(spec)?;
    let interior_x = bounds.max_x_exclusive - bounds.min_x - 4;
    let interior_z = bounds.max_z_exclusive - bounds.min_z - 4;
    if interior_x <= 0 || interior_z <= 0 {
        return random_trace(spec, query_count);
    }

    let mut trace = Vec::with_capacity(query_count);
    let mut step = 0_i32;
    while trace.len() < query_count {
        let center_x = bounds.min_x + 2 + (step * 3).rem_euclid(interior_x);
        let center_z = bounds.min_z + 2 + (step * 5).rem_euclid(interior_z);
        let center_y = 64 + (step.rem_euclid(17) - 8);
        for dy in -1..=2 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    if trace.len() == query_count {
                        return Ok(trace);
                    }
                    trace.push(BlockPos {
                        x: center_x + dx,
                        y: center_y + dy,
                        z: center_z + dz,
                    });
                }
            }
        }
        step = step.wrapping_add(1);
    }
    Ok(trace)
}

fn pathfinding_trace(
    spec: CaseSpec,
    query_count: usize,
) -> Result<Vec<BlockPos>, WorldAccessError> {
    const NEIGHBORS: [(i32, i32, i32); 7] = [
        (0, 0, 0),
        (1, 0, 0),
        (-1, 0, 0),
        (0, 0, 1),
        (0, 0, -1),
        (0, 1, 0),
        (0, -1, 0),
    ];

    let bounds = checked_block_bounds(spec)?;
    let mut rng = Rng::new(case_seed(spec) ^ 0xA076_1D64_78BD_642F);
    let mut x = bounds.min_x + (bounds.max_x_exclusive - bounds.min_x) / 2;
    let mut z = bounds.min_z + (bounds.max_z_exclusive - bounds.min_z) / 2;
    let mut y = 64_i32;
    let mut trace = Vec::with_capacity(query_count);

    while trace.len() < query_count {
        for &(dx, dy, dz) in &NEIGHBORS {
            if trace.len() == query_count {
                return Ok(trace);
            }
            trace.push(BlockPos {
                x: (x + dx).clamp(bounds.min_x, bounds.max_x_exclusive - 1),
                y: (y + dy).clamp(-64, 319),
                z: (z + dz).clamp(bounds.min_z, bounds.max_z_exclusive - 1),
            });
        }

        match rng.next() & 3 {
            0 => x = (x + 1).min(bounds.max_x_exclusive - 2),
            1 => x = (x - 1).max(bounds.min_x + 1),
            2 => z = (z + 1).min(bounds.max_z_exclusive - 2),
            _ => z = (z - 1).max(bounds.min_z + 1),
        }
        let y_delta = i32::try_from((rng.next() >> 7) % 3)
            .map_err(|_| WorldAccessError::CoordinateOverflow)?
            - 1;
        y = (y + y_delta).clamp(-63, 318);
    }
    Ok(trace)
}

fn streaming_trace(spec: CaseSpec, query_count: usize) -> Result<Vec<BlockPos>, WorldAccessError> {
    let bounds = checked_block_bounds(spec)?;
    let mut rng = Rng::new(case_seed(spec) ^ 0xE703_7ED1_A0B4_28DB);
    let mut trace = Vec::with_capacity(query_count);
    for index in 0..query_count {
        let index = i64::try_from(index).map_err(|_| WorldAccessError::CoordinateOverflow)?;
        let width = i64::from(bounds.max_x_exclusive - bounds.min_x);
        let depth = i64::from(bounds.max_z_exclusive - bounds.min_z);
        let x_offset = (index.wrapping_mul(131) + i64::try_from(rng.next() & 0xff).unwrap_or(0))
            .rem_euclid(width);
        let z_offset = (index.wrapping_mul(977)
            + i64::try_from((rng.next() >> 8) & 0xff).unwrap_or(0))
        .rem_euclid(depth);
        trace.push(BlockPos {
            x: bounds.min_x
                + i32::try_from(x_offset).map_err(|_| WorldAccessError::CoordinateOverflow)?,
            y: -64
                + i32::try_from((rng.next() >> 16) % 384)
                    .map_err(|_| WorldAccessError::CoordinateOverflow)?,
            z: bounds.min_z
                + i32::try_from(z_offset).map_err(|_| WorldAccessError::CoordinateOverflow)?,
        });
    }
    Ok(trace)
}

fn checksum<I, F>(positions: I, mut read: F) -> Result<u64, WorldAccessError>
where
    I: IntoIterator<Item = BlockPos>,
    F: FnMut(BlockPos) -> Result<BlockStateId, WorldAccessError>,
{
    let mut checksum = 0x6A09_E667_F3BC_C909_u64;
    for pos in positions {
        let state = read(pos)?;
        checksum = checksum.rotate_left(9)
            ^ u64::try_from(state.as_usize())
                .map_err(|_| WorldAccessError::InvalidStateIdentity)?
                .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    Ok(checksum)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlockBounds {
    min_x: i32,
    max_x_exclusive: i32,
    min_z: i32,
    max_z_exclusive: i32,
}

fn checked_block_bounds(spec: CaseSpec) -> Result<BlockBounds, WorldAccessError> {
    let width = i32::try_from(spec.width).map_err(|_| WorldAccessError::CoordinateOverflow)?;
    let depth = i32::try_from(spec.depth).map_err(|_| WorldAccessError::CoordinateOverflow)?;
    let min_x = spec
        .origin
        .x
        .checked_mul(BLOCKS_PER_CHUNK_AXIS)
        .ok_or(WorldAccessError::CoordinateOverflow)?;
    let min_z = spec
        .origin
        .z
        .checked_mul(BLOCKS_PER_CHUNK_AXIS)
        .ok_or(WorldAccessError::CoordinateOverflow)?;
    let x_end = spec
        .origin
        .x
        .checked_add(width)
        .and_then(|value| value.checked_mul(BLOCKS_PER_CHUNK_AXIS))
        .ok_or(WorldAccessError::CoordinateOverflow)?;
    let z_end = spec
        .origin
        .z
        .checked_add(depth)
        .and_then(|value| value.checked_mul(BLOCKS_PER_CHUNK_AXIS))
        .ok_or(WorldAccessError::CoordinateOverflow)?;
    Ok(BlockBounds {
        min_x,
        max_x_exclusive: x_end,
        min_z,
        max_z_exclusive: z_end,
    })
}

fn case_seed(spec: CaseSpec) -> u64 {
    let mut seed = 0x243F_6A88_85A3_08D3_u64;
    for byte in spec.name.as_bytes() {
        seed ^= u64::from(*byte);
        seed = seed.rotate_left(7).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    seed
}

#[derive(Clone, Copy, Debug)]
struct Rng(u64);

impl Rng {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn i32_in(&mut self, min: i32, max_exclusive: i32) -> i32 {
        debug_assert!(min < max_exclusive);
        let width = u64::try_from(i64::from(max_exclusive) - i64::from(min))
            .expect("positive i32 range fits u64");
        let offset = i32::try_from(self.next() % width).expect("bounded i32 range offset fits i32");
        min + offset
    }
}

#[cfg(test)]
mod tests {
    use super::{PreparedCase, full_cases, smoke_cases};

    #[test]
    fn smoke_cases_are_exactly_equivalent() {
        for spec in smoke_cases() {
            let case = PreparedCase::new(spec, 8_192).expect("valid smoke case");
            let checksum = case
                .validate_equivalence()
                .expect("exact routing equivalence");
            assert_ne!(checksum, 0);
        }
    }

    #[test]
    fn full_case_traces_are_deterministic() {
        for spec in full_cases() {
            let first = PreparedCase::new(spec, 4_096).expect("valid full case");
            let second = PreparedCase::new(spec, 4_096).expect("valid full case");
            assert_eq!(first.trace(), second.trace(), "{}", spec.name);
            assert_eq!(
                first.validate_equivalence(),
                second.validate_equivalence(),
                "{}",
                spec.name
            );
        }
    }
}
