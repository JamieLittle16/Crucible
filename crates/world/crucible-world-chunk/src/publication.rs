//! Explicit immutable publication boundary for live chunk state.
//!
//! Publication is a semantic projection, not a clone of the live storage representation. The
//! reference mechanism deliberately scans through [`BlockSection`] and owns one canonical,
//! section-major state image. Networking and persistence can therefore prepare work without
//! retaining a borrow of mutable world state.

use crucible_types::{ChunkPos, ChunkStamp};
use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection, SectionBlockPos};

use crate::{LiveChunkCore, SectionMasks};

/// Immutable semantic image of one exact live chunk state.
///
/// The state array is section-major. Inside each section, cells use
/// [`SectionBlockPos::index()`] order: `(y << 8) | (z << 4) | x`.
///
/// This type intentionally does not implement `Clone`. Moving an explicit publication to a
/// background consumer is cheap; duplicating the complete semantic image must remain an explicit
/// mechanism choice rather than accidental API behavior.
#[derive(Debug, Eq, PartialEq)]
pub struct PublishedChunk<S: Copy + Eq> {
    position: ChunkPos,
    stamp: ChunkStamp,
    min_section_y: i32,
    masks: SectionMasks,
    states: Box<[S]>,
}

impl<S: Copy + Eq> PublishedChunk<S> {
    /// Semantic chunk column represented by this publication.
    #[must_use]
    pub const fn position(&self) -> ChunkPos {
        self.position
    }

    /// Exact live generation/revision observed while publishing.
    #[must_use]
    pub const fn stamp(&self) -> ChunkStamp {
        self.stamp
    }

    /// Lowest logical section Y represented by the publication.
    #[must_use]
    pub const fn min_section_y(&self) -> i32 {
        self.min_section_y
    }

    /// Number of contiguous logical section images.
    #[must_use]
    pub fn section_count(&self) -> usize {
        self.states.len() / BLOCK_SECTION_CELLS
    }

    /// Vertical summary masks captured at the same live revision as the state image.
    #[must_use]
    pub const fn masks(&self) -> SectionMasks {
        self.masks
    }

    /// Complete canonical section-major semantic state image.
    #[must_use]
    pub fn states(&self) -> &[S] {
        &self.states
    }

    /// Canonical semantic image of one logical section by zero-based lattice index.
    #[must_use]
    pub fn section_states(&self, section_index: usize) -> Option<&[S]> {
        let start = section_index.checked_mul(BLOCK_SECTION_CELLS)?;
        let end = start.checked_add(BLOCK_SECTION_CELLS)?;
        self.states.get(start..end)
    }

    /// Reads one cell from the immutable publication.
    #[must_use]
    pub fn get(&self, section_index: usize, pos: SectionBlockPos) -> Option<S> {
        let base = section_index.checked_mul(BLOCK_SECTION_CELLS)?;
        let index = base.checked_add(pos.index())?;
        self.states.get(index).copied()
    }
}

impl<S, Section> LiveChunkCore<S, Section>
where
    S: Copy + Eq,
    Section: BlockSection<S>,
{
    /// Whether `stamp` is exactly current for this already-identified live chunk.
    ///
    /// Consumers that carry a complete [`PublishedChunk`] should prefer
    /// [`Self::is_publication_current`], which also validates chunk position.
    #[must_use]
    pub const fn accepts_stamp(&self, stamp: ChunkStamp) -> bool {
        self.stamp() == stamp
    }

    /// Whether an immutable publication still describes this exact live chunk state.
    #[must_use]
    pub const fn is_publication_current(&self, publication: &PublishedChunk<S>) -> bool {
        self.position() == publication.position && self.accepts_stamp(publication.stamp)
    }

    /// Projects the current live chunk into one immutable canonical semantic image.
    ///
    /// This reference publication mechanism performs one allocation for the state image and scans
    /// every semantic cell through the static [`BlockSection`] contract. It never observes or
    /// exposes the backing section representation.
    ///
    /// Publication is intentionally outside the ordinary mutation HOT path. Later reuse/COW/page
    /// candidates must demonstrate a measured whole-cost win against this transparent baseline.
    #[must_use]
    pub fn publish_semantic_image(&self) -> PublishedChunk<S> {
        let cell_count = self.sections.len() * BLOCK_SECTION_CELLS;
        let mut states = Vec::with_capacity(cell_count);

        for section in &self.sections {
            for y in 0_u8..16 {
                for z in 0_u8..16 {
                    for x in 0_u8..16 {
                        if let Some(pos) = SectionBlockPos::new(x, y, z) {
                            states.push(section.get(pos));
                        }
                    }
                }
            }
        }

        debug_assert_eq!(states.len(), cell_count);
        PublishedChunk {
            position: self.position,
            stamp: self.stamp(),
            min_section_y: self.min_section_y,
            masks: self.masks,
            states: states.into_boxed_slice(),
        }
    }
}
