//! Target-neutral loaded-dimension and resident-chunk lifecycle for Helve.
//!
//! This crate sits deliberately above [`helve_world_chunk::LiveChunkCore`]. It owns the sparse
//! COLD/BOUNDARY directory used to discover, load and unload resident chunks, while resolved HOT
//! work borrows the already-validated chunk directly. Networking, packet identities, NBT, world
//! generation, scheduling and region-worker placement do not belong here.

#![forbid(unsafe_code)]

use std::collections::{HashMap, hash_map::Entry};

use helve_types::{ChunkGeneration, ChunkPos, DimensionId, DimensionTypeId};
use helve_world_chunk::{ChunkCoreError, LiveChunkCore};
use helve_world_contract::BlockSection;

const BLOCKS_PER_SECTION_AXIS: i32 = 16;
const BLOCKS_PER_SECTION_AXIS_U32: u32 = 16;
const MAX_CHUNK_SECTION_COUNT: u32 = u64::BITS;

/// Immutable, pre-resolved semantic facts for one loaded dimension type.
///
/// Resource-location strings and target registry identities are intentionally absent. They are
/// resolved at cold composition/protocol boundaries into compact identities before HOT world code
/// runs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DimensionRuntimeProfile {
    type_id: DimensionTypeId,
    min_block_y: i32,
    max_block_y_exclusive: i32,
    min_section_y: i32,
    section_count: u8,
    has_skylight: bool,
}

impl DimensionRuntimeProfile {
    /// Builds a compact runtime profile from a section-aligned vertical lattice.
    ///
    /// `height` is measured in blocks and must be non-zero, section-aligned and small enough for
    /// the chunk kernel's current `u64` vertical summary masks.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested lattice cannot be represented exactly by the current
    /// chunk contract.
    pub fn new(
        type_id: DimensionTypeId,
        min_block_y: i32,
        height: u32,
        has_skylight: bool,
    ) -> Result<Self, DimensionProfileError> {
        if height == 0 {
            return Err(DimensionProfileError::ZeroHeight);
        }
        if min_block_y.rem_euclid(BLOCKS_PER_SECTION_AXIS) != 0 {
            return Err(DimensionProfileError::MinBlockYNotSectionAligned { min_block_y });
        }
        if !height.is_multiple_of(BLOCKS_PER_SECTION_AXIS_U32) {
            return Err(DimensionProfileError::HeightNotSectionAligned { height });
        }

        let section_count = height / BLOCKS_PER_SECTION_AXIS_U32;
        if section_count > MAX_CHUNK_SECTION_COUNT {
            return Err(
                DimensionProfileError::SectionCountExceedsChunkMaskCapacity {
                    count: section_count,
                },
            );
        }

        let height_i32 =
            i32::try_from(height).map_err(|_| DimensionProfileError::BlockRangeOverflow {
                min_block_y,
                height,
            })?;
        let max_block_y_exclusive = min_block_y.checked_add(height_i32).ok_or(
            DimensionProfileError::BlockRangeOverflow {
                min_block_y,
                height,
            },
        )?;
        let section_count = u8::try_from(section_count).map_err(|_| {
            DimensionProfileError::SectionCountExceedsChunkMaskCapacity {
                count: section_count,
            }
        })?;

        Ok(Self {
            type_id,
            min_block_y,
            max_block_y_exclusive,
            min_section_y: min_block_y.div_euclid(BLOCKS_PER_SECTION_AXIS),
            section_count,
            has_skylight,
        })
    }

    /// Compact process-local identity of the immutable dimension-type facts.
    #[must_use]
    pub const fn type_id(self) -> DimensionTypeId {
        self.type_id
    }

    /// Inclusive minimum semantic block Y.
    #[must_use]
    pub const fn min_block_y(self) -> i32 {
        self.min_block_y
    }

    /// Exclusive maximum semantic block Y.
    #[must_use]
    pub const fn max_block_y_exclusive(self) -> i32 {
        self.max_block_y_exclusive
    }

    /// Lowest logical section Y represented by chunks in this dimension.
    #[must_use]
    pub const fn min_section_y(self) -> i32 {
        self.min_section_y
    }

    /// Number of contiguous logical section slots in every admitted resident chunk.
    #[must_use]
    pub const fn section_count(self) -> usize {
        self.section_count as usize
    }

    /// Whether this dimension has semantic skylight.
    #[must_use]
    pub const fn has_skylight(self) -> bool {
        self.has_skylight
    }
}

/// Fail-closed construction errors for [`DimensionRuntimeProfile`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DimensionProfileError {
    /// A loaded dimension cannot have an empty vertical lattice.
    ZeroHeight,
    /// The minimum block Y must start on a logical section boundary.
    MinBlockYNotSectionAligned {
        /// Rejected minimum block Y.
        min_block_y: i32,
    },
    /// The dimension height must contain a whole number of logical sections.
    HeightNotSectionAligned {
        /// Rejected height in blocks.
        height: u32,
    },
    /// The current chunk kernel maintains vertical summary masks in one `u64`.
    SectionCountExceedsChunkMaskCapacity {
        /// Requested logical section count.
        count: u32,
    },
    /// The exclusive maximum block Y cannot be represented in `i32`.
    BlockRangeOverflow {
        /// Requested minimum block Y.
        min_block_y: i32,
        /// Requested height in blocks.
        height: u32,
    },
}

/// Stable process-local identity of one currently resident chunk incarnation.
///
/// Worker placement and active-region membership are intentionally absent. Those are execution
/// topology, not semantic chunk identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ResidentChunkHandle {
    /// Loaded dimension that owns the chunk.
    pub dimension: DimensionId,
    /// Semantic chunk-column position.
    pub position: ChunkPos,
    /// Exact live incarnation at this position.
    pub generation: ChunkGeneration,
}

/// COLD/BOUNDARY chunk-load failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadChunkError {
    /// The semantic chunk position is already resident in this dimension.
    AlreadyResident {
        /// Existing resident identity.
        handle: ResidentChunkHandle,
    },
    /// Imported/constructed section count does not match the dimension's frozen lattice.
    SectionCountMismatch {
        /// Required number of logical sections.
        expected: usize,
        /// Supplied number of logical sections.
        actual: usize,
    },
    /// The process-local generation sequence is exhausted.
    GenerationExhausted,
    /// The underlying chunk core rejected construction.
    ChunkCore(ChunkCoreError),
}

/// Fail-closed errors when resolving or unloading a resident handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResidentChunkAccessError {
    /// A handle from another loaded dimension was presented here.
    WrongDimension {
        /// Dimension that owns this runtime.
        expected: DimensionId,
        /// Dimension carried by the rejected handle.
        actual: DimensionId,
    },
    /// No chunk is currently resident at the handle's semantic position.
    NotResident {
        /// Requested semantic chunk position.
        position: ChunkPos,
    },
    /// The position is resident, but the handle belongs to an older incarnation.
    StaleGeneration {
        /// Semantic chunk position.
        position: ChunkPos,
        /// Current live generation.
        current: ChunkGeneration,
        /// Generation carried by the rejected handle.
        handle: ChunkGeneration,
    },
}

/// Minimal loaded dimension state for R2C.
///
/// The sparse `HashMap` is deliberately a lifecycle/discovery structure. HOT code should resolve a
/// [`ResidentChunkHandle`] once, borrow the corresponding [`LiveChunkCore`], and reuse that direct
/// reference for the bounded operation instead of repeatedly routing through this map.
///
/// The runtime has no `Clone` implementation: loaded mutable world authority must not gain
/// accidental copy semantics.
#[derive(Debug)]
pub struct DimensionInstance<S, Section>
where
    S: Copy + Eq,
    Section: BlockSection<S>,
{
    id: DimensionId,
    profile: DimensionRuntimeProfile,
    resident: HashMap<ChunkPos, LiveChunkCore<S, Section>>,
    next_generation: u64,
}

impl<S, Section> DimensionInstance<S, Section>
where
    S: Copy + Eq,
    Section: BlockSection<S>,
{
    /// Creates an empty loaded dimension without pre-reserving chunk-directory storage.
    #[must_use]
    pub fn new(id: DimensionId, profile: DimensionRuntimeProfile) -> Self {
        Self::with_chunk_capacity(id, profile, 0)
    }

    /// Creates an empty loaded dimension with a COLD directory capacity hint.
    ///
    /// Importers that know how many chunks they are about to install can use this to avoid repeated
    /// sparse-directory growth without changing HOT chunk layout.
    #[must_use]
    pub fn with_chunk_capacity(
        id: DimensionId,
        profile: DimensionRuntimeProfile,
        chunk_capacity: usize,
    ) -> Self {
        Self {
            id,
            profile,
            resident: HashMap::with_capacity(chunk_capacity),
            next_generation: 1,
        }
    }

    /// Compact process-local identity of this loaded dimension instance.
    #[must_use]
    pub const fn id(&self) -> DimensionId {
        self.id
    }

    /// Pre-resolved immutable dimension facts.
    #[must_use]
    pub const fn profile(&self) -> DimensionRuntimeProfile {
        self.profile
    }

    /// Number of currently resident semantic chunks.
    #[must_use]
    pub fn resident_chunk_count(&self) -> usize {
        self.resident.len()
    }

    /// Discovers the exact current handle for a resident semantic position.
    ///
    /// This performs one sparse directory lookup and is therefore a lifecycle/resolution operation,
    /// not the API to call for every block access in a HOT loop.
    #[must_use]
    pub fn discover_chunk(&self, position: ChunkPos) -> Option<ResidentChunkHandle> {
        self.resident
            .get(&position)
            .map(|chunk| ResidentChunkHandle {
                dimension: self.id,
                position,
                generation: chunk.generation(),
            })
    }

    /// Installs one new resident chunk incarnation.
    ///
    /// The sparse directory is probed exactly once. Generation advances only after all validation
    /// and chunk construction succeed. A rejected load therefore cannot consume semantic identity
    /// or replace an existing resident chunk.
    ///
    /// # Errors
    ///
    /// Returns an explicit error for duplicate residency, vertical-lattice mismatch, generation
    /// exhaustion or lower-level chunk construction failure.
    pub fn load_chunk(
        &mut self,
        position: ChunkPos,
        sections: Vec<Section>,
    ) -> Result<ResidentChunkHandle, LoadChunkError> {
        match self.resident.entry(position) {
            Entry::Occupied(entry) => Err(LoadChunkError::AlreadyResident {
                handle: ResidentChunkHandle {
                    dimension: self.id,
                    position,
                    generation: entry.get().generation(),
                },
            }),
            Entry::Vacant(entry) => {
                let expected = self.profile.section_count();
                if sections.len() != expected {
                    return Err(LoadChunkError::SectionCountMismatch {
                        expected,
                        actual: sections.len(),
                    });
                }

                let next_generation = self
                    .next_generation
                    .checked_add(1)
                    .ok_or(LoadChunkError::GenerationExhausted)?;
                let generation = ChunkGeneration(self.next_generation);
                let chunk = LiveChunkCore::new(
                    position,
                    generation,
                    self.profile.min_section_y(),
                    sections,
                )
                .map_err(LoadChunkError::ChunkCore)?;

                entry.insert(chunk);
                self.next_generation = next_generation;
                Ok(ResidentChunkHandle {
                    dimension: self.id,
                    position,
                    generation,
                })
            }
        }
    }

    /// Resolves an already-known resident handle to direct immutable chunk access.
    ///
    /// A caller performing many local reads should keep this borrow and avoid repeating sparse
    /// dimension routing inside the operation.
    ///
    /// # Errors
    ///
    /// Rejects handles from another dimension, unloaded positions and stale generations.
    pub fn resolve_chunk(
        &self,
        handle: ResidentChunkHandle,
    ) -> Result<&LiveChunkCore<S, Section>, ResidentChunkAccessError> {
        self.validate_dimension(handle)?;
        let chunk =
            self.resident
                .get(&handle.position)
                .ok_or(ResidentChunkAccessError::NotResident {
                    position: handle.position,
                })?;
        validate_generation(handle, chunk.generation())?;
        Ok(chunk)
    }

    /// Resolves an already-known resident handle to direct authoritative chunk mutation access.
    ///
    /// # Errors
    ///
    /// Rejects handles from another dimension, unloaded positions and stale generations.
    pub fn resolve_chunk_mut(
        &mut self,
        handle: ResidentChunkHandle,
    ) -> Result<&mut LiveChunkCore<S, Section>, ResidentChunkAccessError> {
        self.validate_dimension(handle)?;
        let chunk = self.resident.get_mut(&handle.position).ok_or(
            ResidentChunkAccessError::NotResident {
                position: handle.position,
            },
        )?;
        validate_generation(handle, chunk.generation())?;
        Ok(chunk)
    }

    /// Removes exactly the resident chunk incarnation named by `handle` and returns its semantic
    /// state to the lifecycle caller.
    ///
    /// Returning the chunk rather than silently dropping it keeps later persistence/eviction
    /// policy outside this target-neutral lifecycle primitive. The sparse directory is probed once.
    ///
    /// # Errors
    ///
    /// Rejects wrong-dimension, unloaded and stale-generation handles without changing residency.
    pub fn unload_chunk(
        &mut self,
        handle: ResidentChunkHandle,
    ) -> Result<LiveChunkCore<S, Section>, ResidentChunkAccessError> {
        self.validate_dimension(handle)?;
        match self.resident.entry(handle.position) {
            Entry::Vacant(_) => Err(ResidentChunkAccessError::NotResident {
                position: handle.position,
            }),
            Entry::Occupied(entry) => {
                validate_generation(handle, entry.get().generation())?;
                Ok(entry.remove())
            }
        }
    }

    fn validate_dimension(
        &self,
        handle: ResidentChunkHandle,
    ) -> Result<(), ResidentChunkAccessError> {
        if handle.dimension == self.id {
            Ok(())
        } else {
            Err(ResidentChunkAccessError::WrongDimension {
                expected: self.id,
                actual: handle.dimension,
            })
        }
    }
}

fn validate_generation(
    handle: ResidentChunkHandle,
    current: ChunkGeneration,
) -> Result<(), ResidentChunkAccessError> {
    if handle.generation == current {
        Ok(())
    } else {
        Err(ResidentChunkAccessError::StaleGeneration {
            position: handle.position,
            current,
            handle: handle.generation,
        })
    }
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use helve_types::{BlockPos, ChunkGeneration, ChunkPos, DimensionId, DimensionTypeId};
    use helve_world_contract::{BlockStateFacts, SectionStateFacts};
    use helve_world_reference::DirectBlockSection;

    use super::{
        DimensionInstance, DimensionProfileError, DimensionRuntimeProfile, LoadChunkError,
        ResidentChunkAccessError, ResidentChunkHandle,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum State {
        Air,
        Solid,
    }

    struct Facts;

    impl BlockStateFacts<State> for Facts {
        fn facts(&self, state: State) -> SectionStateFacts {
            match state {
                State::Air => SectionStateFacts::new(false, false, false, false),
                State::Solid => SectionStateFacts::new(true, false, false, false),
            }
        }
    }

    fn profile() -> DimensionRuntimeProfile {
        DimensionRuntimeProfile::new(DimensionTypeId(3), 0, 32, true)
            .expect("two-section test profile")
    }

    fn sections(profile: DimensionRuntimeProfile) -> Vec<DirectBlockSection<State>> {
        (0..profile.section_count())
            .map(|_| DirectBlockSection::filled(State::Air, &Facts))
            .collect()
    }

    #[test]
    fn standard_vertical_lattice_is_resolved_once() {
        let profile = DimensionRuntimeProfile::new(DimensionTypeId(9), -64, 384, true)
            .expect("standard overworld lattice is representable");
        assert_eq!(profile.min_block_y(), -64);
        assert_eq!(profile.max_block_y_exclusive(), 320);
        assert_eq!(profile.min_section_y(), -4);
        assert_eq!(profile.section_count(), 24);
        assert!(profile.has_skylight());
        assert_eq!(profile.type_id(), DimensionTypeId(9));
    }

    #[test]
    fn invalid_dimension_lattices_fail_closed() {
        assert_eq!(
            DimensionRuntimeProfile::new(DimensionTypeId(0), 0, 0, true),
            Err(DimensionProfileError::ZeroHeight)
        );
        assert_eq!(
            DimensionRuntimeProfile::new(DimensionTypeId(0), 1, 16, true),
            Err(DimensionProfileError::MinBlockYNotSectionAligned { min_block_y: 1 })
        );
        assert_eq!(
            DimensionRuntimeProfile::new(DimensionTypeId(0), 0, 17, true),
            Err(DimensionProfileError::HeightNotSectionAligned { height: 17 })
        );
        assert_eq!(
            DimensionRuntimeProfile::new(DimensionTypeId(0), 0, 16 * 65, true),
            Err(DimensionProfileError::SectionCountExceedsChunkMaskCapacity { count: 65 })
        );
    }

    #[test]
    fn runtime_identities_remain_compact() {
        assert!(size_of::<DimensionRuntimeProfile>() <= 24);
        assert!(size_of::<ResidentChunkHandle>() <= 24);
    }

    #[test]
    fn load_resolve_mutate_and_unload_preserve_exact_identity() {
        let profile = profile();
        let mut dimension = DimensionInstance::with_chunk_capacity(DimensionId(7), profile, 4);
        let position = ChunkPos { x: -2, z: 5 };

        let handle = dimension
            .load_chunk(position, sections(profile))
            .expect("first load succeeds");
        assert_eq!(dimension.resident_chunk_count(), 1);
        assert_eq!(dimension.discover_chunk(position), Some(handle));
        assert_eq!(
            dimension
                .resolve_chunk(handle)
                .expect("current handle")
                .revision()
                .0,
            0
        );

        let changed = dimension
            .resolve_chunk_mut(handle)
            .expect("current mutable handle")
            .replace_block(
                BlockPos {
                    x: -32,
                    y: 0,
                    z: 80,
                },
                State::Solid,
                &Facts,
            )
            .expect("block belongs to resident chunk");
        assert!(changed.changed);
        assert_eq!(
            dimension
                .resolve_chunk(handle)
                .expect("same generation")
                .revision()
                .0,
            1
        );

        let unloaded = dimension
            .unload_chunk(handle)
            .expect("exact current handle unloads");
        assert_eq!(unloaded.position(), position);
        assert_eq!(unloaded.generation(), handle.generation);
        assert_eq!(unloaded.revision().0, 1);
        assert_eq!(dimension.resident_chunk_count(), 0);
        assert!(matches!(
            dimension.resolve_chunk(handle),
            Err(ResidentChunkAccessError::NotResident { position: rejected }) if rejected == position
        ));
    }

    #[test]
    fn reload_creates_a_new_generation_and_stale_handles_fail() {
        let profile = profile();
        let mut dimension = DimensionInstance::new(DimensionId(1), profile);
        let position = ChunkPos { x: 4, z: -9 };

        let first = dimension
            .load_chunk(position, sections(profile))
            .expect("first load");
        dimension.unload_chunk(first).expect("first unload");
        let second = dimension
            .load_chunk(position, sections(profile))
            .expect("reload");

        assert_ne!(first.generation, second.generation);
        assert_eq!(second.generation, ChunkGeneration(first.generation.0 + 1));
        assert!(matches!(
            dimension.resolve_chunk(first),
            Err(ResidentChunkAccessError::StaleGeneration {
                position: rejected,
                current,
                handle,
            }) if rejected == position && current == second.generation && handle == first.generation
        ));
        assert!(dimension.resolve_chunk(second).is_ok());
    }

    #[test]
    fn rejected_loads_do_not_replace_state_or_consume_generation() {
        let profile = profile();
        let mut dimension = DimensionInstance::new(DimensionId(4), profile);
        let first_position = ChunkPos { x: 0, z: 0 };

        assert_eq!(
            dimension.load_chunk(
                first_position,
                vec![DirectBlockSection::filled(State::Air, &Facts)]
            ),
            Err(LoadChunkError::SectionCountMismatch {
                expected: 2,
                actual: 1,
            })
        );

        let first = dimension
            .load_chunk(first_position, sections(profile))
            .expect("valid load after rejection");
        assert_eq!(first.generation, ChunkGeneration(1));

        assert_eq!(
            dimension.load_chunk(first_position, sections(profile)),
            Err(LoadChunkError::AlreadyResident { handle: first })
        );
        assert_eq!(dimension.resident_chunk_count(), 1);
    }

    #[test]
    fn handles_cannot_cross_dimension_boundaries() {
        let profile = profile();
        let mut first = DimensionInstance::new(DimensionId(10), profile);
        let second =
            DimensionInstance::<State, DirectBlockSection<State>>::new(DimensionId(11), profile);
        let position = ChunkPos { x: 1, z: 1 };
        let handle = first
            .load_chunk(position, sections(profile))
            .expect("resident chunk");

        assert!(matches!(
            second.resolve_chunk(handle),
            Err(ResidentChunkAccessError::WrongDimension { expected, actual })
                if expected == DimensionId(11) && actual == DimensionId(10)
        ));
    }
}
