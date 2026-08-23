use crucible_types::{BlockPos, ChunkPos};
use crucible_world_contract::BlockSection;

use super::{BLOCKS_PER_CHUNK_AXIS, ChunkCoreError, LiveChunkCore};

/// Construction/read failures for a borrowed dense chunk window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedChunkWindowError {
    /// A resolved window must cover at least one chunk in both horizontal axes.
    EmptyExtent,
    /// The requested rectangular extent cannot be represented safely.
    ExtentOverflow,
    /// A supplied live chunk does not belong to the requested rectangle.
    ChunkOutsideWindow {
        /// Unexpected chunk position.
        position: ChunkPos,
        /// Minimum chunk position of the requested rectangle.
        origin: ChunkPos,
        /// Chunk count along X.
        width: usize,
        /// Chunk count along Z.
        depth: usize,
    },
    /// More than one supplied chunk claims the same semantic chunk position.
    DuplicateChunk {
        /// Duplicated semantic chunk position.
        position: ChunkPos,
    },
    /// One required row-major slot was not supplied at construction.
    MissingChunk {
        /// Missing semantic chunk position.
        position: ChunkPos,
    },
    /// A block read falls outside the resolved horizontal rectangle.
    PositionOutsideWindow {
        /// Rejected semantic block position.
        pos: BlockPos,
        /// Minimum chunk position of the resolved rectangle.
        origin: ChunkPos,
        /// Chunk count along X.
        width: usize,
        /// Chunk count along Z.
        depth: usize,
    },
    /// The horizontal chunk was resolved, but its vertical lattice rejected the block position.
    Chunk(ChunkCoreError),
}

/// Borrowed row-major view over an exact rectangular set of already-live chunks.
///
/// Construction performs the generality-resolution work once. Repeated block reads then use
/// Euclidean chunk arithmetic plus one dense pointer lookup; they do not perform a global
/// directory/hash lookup, allocation, synchronization, reference counting, service lookup, or
/// dynamic dispatch.
///
/// The view is read-only in M0.4C.1. Its borrow keeps the admitted chunks immutably borrowed for
/// the view lifetime, so this slice does not smuggle ownership/migration semantics into the access
/// experiment.
#[derive(Debug)]
pub struct ResolvedChunkWindow<'a, S, Section>
where
    S: Copy + Eq,
    Section: BlockSection<S>,
{
    origin: ChunkPos,
    width: usize,
    depth: usize,
    chunks: Box<[&'a LiveChunkCore<S, Section>]>,
}

impl<'a, S, Section> ResolvedChunkWindow<'a, S, Section>
where
    S: Copy + Eq,
    Section: BlockSection<S>,
{
    /// Resolves an exact rectangular chunk set into dense row-major slots.
    ///
    /// Input order is irrelevant. Every semantic chunk position in the rectangle must be supplied
    /// exactly once; missing, duplicate, or out-of-window chunks fail closed.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty/overflowing extent or any incomplete/ambiguous chunk set.
    pub fn new<I>(
        origin: ChunkPos,
        width: usize,
        depth: usize,
        chunks: I,
    ) -> Result<Self, ResolvedChunkWindowError>
    where
        I: IntoIterator<Item = &'a LiveChunkCore<S, Section>>,
    {
        if width == 0 || depth == 0 {
            return Err(ResolvedChunkWindowError::EmptyExtent);
        }
        let area = width
            .checked_mul(depth)
            .ok_or(ResolvedChunkWindowError::ExtentOverflow)?;
        validate_extent_axis(origin.x, width)?;
        validate_extent_axis(origin.z, depth)?;

        let mut slots = vec![None; area];
        for chunk in chunks {
            let position = chunk.position();
            let Some(x) = axis_offset(origin.x, position.x, width) else {
                return Err(ResolvedChunkWindowError::ChunkOutsideWindow {
                    position,
                    origin,
                    width,
                    depth,
                });
            };
            let Some(z) = axis_offset(origin.z, position.z, depth) else {
                return Err(ResolvedChunkWindowError::ChunkOutsideWindow {
                    position,
                    origin,
                    width,
                    depth,
                });
            };
            let index = z
                .checked_mul(width)
                .and_then(|row| row.checked_add(x))
                .ok_or(ResolvedChunkWindowError::ExtentOverflow)?;
            if slots[index].replace(chunk).is_some() {
                return Err(ResolvedChunkWindowError::DuplicateChunk { position });
            }
        }

        if let Some(index) = slots.iter().position(Option::is_none) {
            return Err(ResolvedChunkWindowError::MissingChunk {
                position: position_for_index(origin, width, index),
            });
        }

        let chunks = slots
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .into_boxed_slice();
        debug_assert_eq!(chunks.len(), area);
        Ok(Self {
            origin,
            width,
            depth,
            chunks,
        })
    }

    /// Minimum semantic chunk position covered by this window.
    #[must_use]
    pub const fn origin(&self) -> ChunkPos {
        self.origin
    }

    /// Number of chunk columns along the X axis.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Number of chunk columns along the Z axis.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// Number of chunks retained by the dense resolved view.
    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Reads one exact semantic block state through the pre-resolved dense chunk window.
    ///
    /// # Errors
    ///
    /// Returns an error when the X/Z position lies outside the resolved rectangle or when the
    /// selected live chunk rejects the block's vertical Y coordinate.
    pub fn get_block(&self, pos: BlockPos) -> Result<S, ResolvedChunkWindowError> {
        let chunk_x = pos.x.div_euclid(BLOCKS_PER_CHUNK_AXIS);
        let chunk_z = pos.z.div_euclid(BLOCKS_PER_CHUNK_AXIS);
        let Some(x) = axis_offset(self.origin.x, chunk_x, self.width) else {
            return Err(self.outside_error(pos));
        };
        let Some(z) = axis_offset(self.origin.z, chunk_z, self.depth) else {
            return Err(self.outside_error(pos));
        };
        let index = z * self.width + x;
        let chunk = self.chunks[index];
        debug_assert_eq!(
            chunk.position(),
            ChunkPos {
                x: chunk_x,
                z: chunk_z
            }
        );

        chunk
            .get_pre_resolved_block(pos)
            .map_err(ResolvedChunkWindowError::Chunk)
    }

    fn outside_error(&self, pos: BlockPos) -> ResolvedChunkWindowError {
        ResolvedChunkWindowError::PositionOutsideWindow {
            pos,
            origin: self.origin,
            width: self.width,
            depth: self.depth,
        }
    }
}

fn validate_extent_axis(origin: i32, extent: usize) -> Result<(), ResolvedChunkWindowError> {
    let last_offset = extent
        .checked_sub(1)
        .ok_or(ResolvedChunkWindowError::EmptyExtent)?;
    let last_offset =
        i64::try_from(last_offset).map_err(|_| ResolvedChunkWindowError::ExtentOverflow)?;
    let last = i64::from(origin)
        .checked_add(last_offset)
        .ok_or(ResolvedChunkWindowError::ExtentOverflow)?;
    i32::try_from(last)
        .map(|_| ())
        .map_err(|_| ResolvedChunkWindowError::ExtentOverflow)
}

fn axis_offset(origin: i32, position: i32, extent: usize) -> Option<usize> {
    let delta = i64::from(position) - i64::from(origin);
    let offset = usize::try_from(delta).ok()?;
    (offset < extent).then_some(offset)
}

fn position_for_index(origin: ChunkPos, width: usize, index: usize) -> ChunkPos {
    let x_offset = index % width;
    let z_offset = index / width;
    let x_offset = i64::try_from(x_offset).expect("validated window X extent fits i64");
    let z_offset = i64::try_from(z_offset).expect("validated window Z extent fits i64");
    ChunkPos {
        x: i32::try_from(i64::from(origin.x) + x_offset)
            .expect("validated window X position fits i32"),
        z: i32::try_from(i64::from(origin.z) + z_offset)
            .expect("validated window Z position fits i32"),
    }
}
