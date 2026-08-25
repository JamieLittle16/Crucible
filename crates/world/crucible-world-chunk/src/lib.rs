//! Minimal live chunk kernel above Crucible's source-backed section contract.
//!
//! This crate owns chunk identity, a contiguous logical section lattice, vertical section-summary
//! masks, and the authoritative block-mutation/revision boundary. It deliberately does not own
//! packets, NBT, entities, world generation, tickets, networking, scheduling, or persistence.

#![forbid(unsafe_code)]

mod lattice;
mod publication;
mod region_cell;
mod window;

use core::marker::PhantomData;

use crucible_types::{BlockPos, ChunkGeneration, ChunkPos, ChunkRevision, ChunkStamp};
use crucible_world_contract::{BlockSection, BlockStateFacts, SectionBlockPos, SectionSummary};

pub use lattice::{VerticalSectionLattice, VerticalSectionLatticeError};
pub use publication::PublishedChunk;
pub use region_cell::{RegionCellAddress, RegionCellCoord, RegionCellLayout};
pub use window::{ResolvedChunkWindow, ResolvedChunkWindowError};

const BLOCKS_PER_CHUNK_AXIS: i32 = 16;
const MAX_MASKED_SECTIONS: usize = u64::BITS as usize;

/// Compact vertical-section summary masks maintained by [`LiveChunkCore`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SectionMasks {
    non_air: u64,
    fluid: u64,
    random_tick: u64,
}

impl SectionMasks {
    /// Raw bitset of logical sections containing at least one non-air block state.
    #[must_use]
    pub const fn non_air_bits(self) -> u64 {
        self.non_air
    }

    /// Raw bitset of logical sections containing at least one counted fluid state.
    #[must_use]
    pub const fn fluid_bits(self) -> u64 {
        self.fluid
    }

    /// Raw bitset of logical sections that may perform block or fluid random ticking.
    #[must_use]
    pub const fn random_tick_bits(self) -> u64 {
        self.random_tick
    }

    fn update(&mut self, section_index: usize, summary: SectionSummary) {
        let bit = 1_u64 << section_index;
        set_bit(&mut self.non_air, bit, !summary.has_only_air());
        set_bit(&mut self.fluid, bit, summary.has_fluid());
        set_bit(&mut self.random_tick, bit, summary.is_randomly_ticking());
    }
}

fn set_bit(mask: &mut u64, bit: u64, present: bool) {
    if present {
        *mask |= bit;
    } else {
        *mask &= !bit;
    }
}

/// Result of one authoritative semantic block mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationFacts<S: Copy + Eq> {
    /// World position whose semantic state was requested to change.
    pub pos: BlockPos,
    /// Exact state present before the operation.
    pub old: S,
    /// Requested replacement state.
    pub new: S,
    /// Whether the semantic image actually changed.
    pub changed: bool,
}

/// Fail-closed access/construction errors for the minimal live chunk core.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkCoreError {
    /// A live chunk must have at least one logical section slot.
    EmptySectionLattice,
    /// M0.4A uses one `u64` for each vertical summary mask.
    SectionCountExceedsMaskCapacity { count: usize },
    /// `min_section_y + section_count` cannot be represented safely.
    SectionRangeOverflow,
    /// A world block position belongs to a different chunk column.
    PositionOutsideChunk {
        /// Rejected semantic block position.
        pos: BlockPos,
        /// Chunk column owned by this core.
        expected_chunk: ChunkPos,
        /// Chunk column implied by `pos`.
        actual_chunk: ChunkPos,
    },
    /// A world block Y lies below or above this chunk's logical section lattice.
    PositionOutsideVerticalLattice {
        /// Rejected semantic block position.
        pos: BlockPos,
        /// Lowest logical section Y owned by this core.
        min_section_y: i32,
        /// Number of contiguous logical section slots.
        section_count: usize,
    },
}

/// Smallest live chunk state admitted by M0.4A.
///
/// `Section` is statically dispatched. The core intentionally has no `Clone` implementation: live
/// mutable world state must not acquire accidental copy semantics. Read-only consumers instead use
/// the explicit immutable [`PublishedChunk`] semantic projection.
#[derive(Debug)]
pub struct LiveChunkCore<S, Section>
where
    S: Copy + Eq,
    Section: BlockSection<S>,
{
    position: ChunkPos,
    generation: ChunkGeneration,
    revision: ChunkRevision,
    lattice: VerticalSectionLattice,
    sections: Box<[Section]>,
    masks: SectionMasks,
    state: PhantomData<fn() -> S>,
}

impl<S, Section> LiveChunkCore<S, Section>
where
    S: Copy + Eq,
    Section: BlockSection<S>,
{
    /// Creates a live chunk over one contiguous logical section lattice.
    ///
    /// Construction may allocate the owned section-slot array. Ordinary block mutation performs no
    /// chunk-core-owned allocation; any representation transition allocation remains the section
    /// mechanism's responsibility.
    ///
    /// # Errors
    ///
    /// Returns an error when the section lattice is empty, exceeds the compact-mask capacity, or
    /// its logical Y range cannot be represented safely.
    pub fn new(
        position: ChunkPos,
        generation: ChunkGeneration,
        min_section_y: i32,
        sections: Vec<Section>,
    ) -> Result<Self, ChunkCoreError> {
        if sections.is_empty() {
            return Err(ChunkCoreError::EmptySectionLattice);
        }
        if sections.len() > MAX_MASKED_SECTIONS {
            return Err(ChunkCoreError::SectionCountExceedsMaskCapacity {
                count: sections.len(),
            });
        }

        let lattice = VerticalSectionLattice::new(min_section_y, sections.len())
            .map_err(|_| ChunkCoreError::SectionRangeOverflow)?;
        let sections = sections.into_boxed_slice();
        let masks = masks_from_sections::<S, Section>(&sections);
        Ok(Self {
            position,
            generation,
            revision: ChunkRevision::default(),
            lattice,
            sections,
            masks,
            state: PhantomData,
        })
    }

    /// Semantic chunk column position.
    #[must_use]
    pub const fn position(&self) -> ChunkPos {
        self.position
    }

    /// Identity of this live incarnation of the chunk column.
    #[must_use]
    pub const fn generation(&self) -> ChunkGeneration {
        self.generation
    }

    /// Current monotone semantic revision inside this generation.
    #[must_use]
    pub const fn revision(&self) -> ChunkRevision {
        self.revision
    }

    /// Exact generation/revision identity for deferred-work validation.
    #[must_use]
    pub const fn stamp(&self) -> ChunkStamp {
        ChunkStamp {
            generation: self.generation,
            revision: self.revision,
        }
    }

    /// Lowest logical section Y represented by this chunk.
    #[must_use]
    pub const fn min_section_y(&self) -> i32 {
        self.lattice.min_section_y()
    }

    /// Resolved vertical section lattice used by block access.
    #[must_use]
    pub const fn vertical_lattice(&self) -> VerticalSectionLattice {
        self.lattice
    }

    /// Number of contiguous logical section slots.
    #[must_use]
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Current incrementally maintained vertical masks.
    #[must_use]
    pub const fn masks(&self) -> SectionMasks {
        self.masks
    }

    /// Reads the exact semantic block state at one world position.
    ///
    /// # Errors
    ///
    /// Returns an error when `pos` belongs to another chunk column or lies outside the configured
    /// vertical section lattice.
    pub fn get_block(&self, pos: BlockPos) -> Result<S, ChunkCoreError> {
        let (section_index, local) = self.resolve_block(pos)?;
        Ok(self.sections[section_index].get(local))
    }

    /// Performs the authoritative semantic block replacement for this chunk.
    ///
    /// A real semantic change advances the chunk revision exactly once and refreshes only the
    /// affected section's vertical mask bits. Same-state replacement leaves revision and masks
    /// unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when `pos` belongs to another chunk column or lies outside the configured
    /// vertical section lattice. Rejected mutations do not change chunk state.
    ///
    /// # Panics
    ///
    /// Panics only if one live chunk generation executes more than `u64::MAX` real semantic block
    /// mutations. Treating that physically unreachable counter exhaustion as an invariant failure
    /// avoids adding another pre-read/branch to every HOT mutation solely to support recovery from
    /// an exhausted 64-bit generation-local sequence number.
    pub fn replace_block<F: BlockStateFacts<S>>(
        &mut self,
        pos: BlockPos,
        state: S,
        facts: &F,
    ) -> Result<MutationFacts<S>, ChunkCoreError> {
        let (section_index, local) = self.resolve_block(pos)?;
        let before_masks = self.masks;
        let previous = self.sections[section_index].replace(local, state, facts);
        let changed = previous != state;

        if changed {
            let summary = self.sections[section_index].summary();
            self.masks.update(section_index, summary);
            self.revision.0 = self
                .revision
                .0
                .checked_add(1)
                .expect("chunk semantic revision exhausted u64");
        } else {
            debug_assert_eq!(self.masks, before_masks);
        }

        Ok(MutationFacts {
            pos,
            old: previous,
            new: state,
            changed,
        })
    }

    /// Independently reconstructs all vertical masks from the current section summaries.
    ///
    /// This is an evidence/recovery oracle. Production mutation updates only one section's bits and
    /// does not invoke this O(section-count) scan.
    #[must_use]
    pub fn recompute_masks(&self) -> SectionMasks {
        masks_from_sections::<S, Section>(&self.sections)
    }

    /// Whether incrementally maintained masks equal independent full recomputation.
    #[must_use]
    pub fn masks_match_recomputation(&self) -> bool {
        self.masks == self.recompute_masks()
    }

    fn get_pre_resolved_block(&self, pos: BlockPos) -> Result<S, ChunkCoreError> {
        debug_assert_eq!(
            ChunkPos {
                x: pos.x.div_euclid(BLOCKS_PER_CHUNK_AXIS),
                z: pos.z.div_euclid(BLOCKS_PER_CHUNK_AXIS),
            },
            self.position
        );
        let local_x = u8::try_from(pos.x.rem_euclid(BLOCKS_PER_CHUNK_AXIS))
            .expect("Euclidean chunk-local x is in 0..16");
        let local_z = u8::try_from(pos.z.rem_euclid(BLOCKS_PER_CHUNK_AXIS))
            .expect("Euclidean chunk-local z is in 0..16");
        let (section_index, local) = self.resolve_vertical(pos, local_x, local_z)?;
        Ok(self.sections[section_index].get(local))
    }

    fn resolve_block(&self, pos: BlockPos) -> Result<(usize, SectionBlockPos), ChunkCoreError> {
        let actual_chunk = ChunkPos {
            x: pos.x.div_euclid(BLOCKS_PER_CHUNK_AXIS),
            z: pos.z.div_euclid(BLOCKS_PER_CHUNK_AXIS),
        };
        if actual_chunk != self.position {
            return Err(ChunkCoreError::PositionOutsideChunk {
                pos,
                expected_chunk: self.position,
                actual_chunk,
            });
        }

        let local_x = u8::try_from(pos.x.rem_euclid(BLOCKS_PER_CHUNK_AXIS))
            .expect("Euclidean chunk-local x is in 0..16");
        let local_z = u8::try_from(pos.z.rem_euclid(BLOCKS_PER_CHUNK_AXIS))
            .expect("Euclidean chunk-local z is in 0..16");
        self.resolve_vertical(pos, local_x, local_z)
    }

    fn resolve_vertical(
        &self,
        pos: BlockPos,
        local_x: u8,
        local_z: u8,
    ) -> Result<(usize, SectionBlockPos), ChunkCoreError> {
        let Some((section_index, local_y)) = self.lattice.resolve_block_y(pos.y) else {
            return Err(ChunkCoreError::PositionOutsideVerticalLattice {
                pos,
                min_section_y: self.lattice.min_section_y(),
                section_count: self.sections.len(),
            });
        };
        let local = SectionBlockPos::new(local_x, local_y, local_z)
            .expect("resolved local coordinates are valid section coordinates");
        Ok((section_index, local))
    }
}

fn masks_from_sections<S, Section>(sections: &[Section]) -> SectionMasks
where
    S: Copy + Eq,
    Section: BlockSection<S>,
{
    let mut masks = SectionMasks::default();
    for (index, section) in sections.iter().enumerate() {
        masks.update(index, section.summary());
    }
    masks
}
