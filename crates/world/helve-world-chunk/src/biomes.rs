use core::marker::PhantomData;

use helve_types::{ChunkGeneration, ChunkPos};
use helve_world_contract::{BiomeSection, SectionBiomePos};

use crate::{VerticalSectionLattice, VerticalSectionLatticeError};

/// Fail-closed construction/access errors for a target-neutral chunk biome column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkBiomeColumnError {
    /// The requested contiguous vertical lattice is invalid.
    Lattice(VerticalSectionLatticeError),
    /// A logical section Y lies outside this chunk's admitted vertical biome lattice.
    SectionOutsideVerticalLattice {
        /// Rejected logical section Y.
        section_y: i32,
        /// Lowest logical section Y owned by this column.
        min_section_y: i32,
        /// Number of contiguous logical section slots.
        section_count: usize,
    },
}

impl From<VerticalSectionLatticeError> for ChunkBiomeColumnError {
    fn from(value: VerticalSectionLatticeError) -> Self {
        Self::Lattice(value)
    }
}

/// Target-neutral contiguous biome state for one live chunk incarnation.
///
/// The column freezes only source-backed semantic shape: one biome section per logical chunk
/// section and a 4×4×4 semantic lattice inside each section. `Section` is statically dispatched so
/// storage mechanisms may remain reference, palette, packed or otherwise qualified without changing
/// callers. Registry strings, persistence codecs and network palettes are deliberately absent.
///
/// This type intentionally carries the chunk generation but no independent revision counter.
/// Biome/block/light mutation freshness must eventually advance through one composite authoritative
/// chunk revision rather than sidecars inventing mutually inconsistent publication clocks.
#[derive(Debug)]
pub struct ChunkBiomeColumn<B, Section>
where
    B: Copy + Eq,
    Section: BiomeSection<B>,
{
    position: ChunkPos,
    generation: ChunkGeneration,
    lattice: VerticalSectionLattice,
    sections: Box<[Section]>,
    biome: PhantomData<fn() -> B>,
}

impl<B, Section> ChunkBiomeColumn<B, Section>
where
    B: Copy + Eq,
    Section: BiomeSection<B>,
{
    /// Creates one contiguous biome-section column for an exact chunk incarnation.
    ///
    /// Construction performs the only column-owned allocation by converting the supplied final
    /// section vector into its stable boxed slot array. Ordinary biome get/replace performs no
    /// allocation and no hash lookup.
    ///
    /// # Errors
    ///
    /// Returns [`ChunkBiomeColumnError::Lattice`] when the logical section lattice is empty, too
    /// large for the compact shared lattice identity, or cannot be represented safely in world
    /// block-space bounds.
    pub fn new(
        position: ChunkPos,
        generation: ChunkGeneration,
        min_section_y: i32,
        sections: Vec<Section>,
    ) -> Result<Self, ChunkBiomeColumnError> {
        let lattice = VerticalSectionLattice::new(min_section_y, sections.len())?;
        Ok(Self {
            position,
            generation,
            lattice,
            sections: sections.into_boxed_slice(),
            biome: PhantomData,
        })
    }

    /// Semantic chunk column position.
    #[must_use]
    pub const fn position(&self) -> ChunkPos {
        self.position
    }

    /// Exact live chunk incarnation this biome image belongs to.
    #[must_use]
    pub const fn generation(&self) -> ChunkGeneration {
        self.generation
    }

    /// Lowest logical section Y represented by this biome column.
    #[must_use]
    pub const fn min_section_y(&self) -> i32 {
        self.lattice.min_section_y()
    }

    /// Number of contiguous logical biome-section slots.
    #[must_use]
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Shared validated vertical lattice used by this chunk image.
    #[must_use]
    pub const fn vertical_lattice(&self) -> VerticalSectionLattice {
        self.lattice
    }

    /// Reads one exact semantic biome from a pre-resolved local 4×4×4 coordinate.
    ///
    /// # Errors
    ///
    /// Returns [`ChunkBiomeColumnError::SectionOutsideVerticalLattice`] when `section_y` lies
    /// outside this chunk's contiguous logical section range.
    #[inline]
    pub fn get(&self, section_y: i32, local: SectionBiomePos) -> Result<B, ChunkBiomeColumnError> {
        let index = self.resolve_section(section_y)?;
        Ok(self.sections[index].get(local))
    }

    /// Replaces one exact semantic biome and returns the previous identity.
    ///
    /// This primitive deliberately does not own publication-revision advancement; the future
    /// composite resident mutation boundary must coordinate that once across all chunk semantic
    /// components.
    ///
    /// # Errors
    ///
    /// Returns [`ChunkBiomeColumnError::SectionOutsideVerticalLattice`] when `section_y` lies
    /// outside this chunk's contiguous logical section range. Rejected operations do not mutate the
    /// column.
    #[inline]
    pub fn replace(
        &mut self,
        section_y: i32,
        local: SectionBiomePos,
        biome: B,
    ) -> Result<B, ChunkBiomeColumnError> {
        let index = self.resolve_section(section_y)?;
        Ok(self.sections[index].replace(local, biome))
    }

    #[inline]
    fn resolve_section(&self, section_y: i32) -> Result<usize, ChunkBiomeColumnError> {
        self.lattice.section_index_for_section_y(section_y).ok_or(
            ChunkBiomeColumnError::SectionOutsideVerticalLattice {
                section_y,
                min_section_y: self.lattice.min_section_y(),
                section_count: self.sections.len(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use helve_types::{ChunkGeneration, ChunkPos};
    use helve_world_contract::SectionBiomePos;
    use helve_world_reference::DirectBiomeSection;

    use super::{ChunkBiomeColumn, ChunkBiomeColumnError};

    fn local(x: u8, y: u8, z: u8) -> SectionBiomePos {
        SectionBiomePos::new(x, y, z).expect("bounded biome coordinate")
    }

    #[test]
    fn negative_zero_and_positive_logical_sections_resolve_exactly() {
        let position = ChunkPos { x: -2, z: 5 };
        let column = ChunkBiomeColumn::new(
            position,
            ChunkGeneration(11),
            -1,
            vec![
                DirectBiomeSection::filled(7_u16),
                DirectBiomeSection::filled(8_u16),
                DirectBiomeSection::filled(9_u16),
            ],
        )
        .expect("three-section biome column");

        assert_eq!(column.position(), position);
        assert_eq!(column.generation(), ChunkGeneration(11));
        assert_eq!(column.min_section_y(), -1);
        assert_eq!(column.section_count(), 3);
        assert_eq!(column.get(-1, local(0, 0, 0)), Ok(7));
        assert_eq!(column.get(0, local(3, 2, 1)), Ok(8));
        assert_eq!(column.get(1, local(1, 3, 2)), Ok(9));
    }

    #[test]
    fn reference_yzx_cells_survive_column_routing_and_local_mutation() {
        let mut lower = DirectBiomeSection::filled(0_u8);
        lower.fill_with(|x, y, z| (y << 4) | (z << 2) | x);
        let upper = DirectBiomeSection::filled(200_u8);
        let mut column = ChunkBiomeColumn::new(
            ChunkPos { x: 4, z: -7 },
            ChunkGeneration(3),
            -4,
            vec![lower, upper],
        )
        .expect("two-section biome column");

        for y in 0..4 {
            for z in 0..4 {
                for x in 0..4 {
                    assert_eq!(column.get(-4, local(x, y, z)), Ok((y << 4) | (z << 2) | x));
                }
            }
        }

        let changed = local(2, 1, 3);
        assert_eq!(column.replace(-3, changed, 17), Ok(200));
        assert_eq!(column.get(-3, changed), Ok(17));
        assert_eq!(column.get(-3, local(1, 1, 3)), Ok(200));
        assert_eq!(column.get(-4, changed), Ok((1 << 4) | (3 << 2) | 2));
    }

    #[test]
    fn out_of_range_section_access_fails_without_mutation() {
        let mut column = ChunkBiomeColumn::new(
            ChunkPos { x: 0, z: 0 },
            ChunkGeneration(1),
            -1,
            vec![DirectBiomeSection::filled(4_u8)],
        )
        .expect("single-section biome column");
        let pos = local(1, 2, 3);

        let expected_low = ChunkBiomeColumnError::SectionOutsideVerticalLattice {
            section_y: -2,
            min_section_y: -1,
            section_count: 1,
        };
        let expected_high = ChunkBiomeColumnError::SectionOutsideVerticalLattice {
            section_y: 0,
            min_section_y: -1,
            section_count: 1,
        };
        assert_eq!(column.get(-2, pos), Err(expected_low));
        assert_eq!(column.replace(0, pos, 9), Err(expected_high));
        assert_eq!(column.get(-1, pos), Ok(4));
    }
}
