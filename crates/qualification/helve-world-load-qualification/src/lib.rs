//! Qualification-only state used to measure the complete R2C stored-block -> residency path.
//!
//! Production world crates must not depend on this crate. The direct section below is deliberately
//! transparent: it materializes all 4096 target-version block-state identities and maintains exact
//! section summaries. It is a stable correctness/performance baseline, not a production policy
//! choice for R2C.2S.

#![forbid(unsafe_code)]

use helve_generated::{BlockStateId, GeneratedStateFacts};
use helve_world_contract::{
    BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionBlockPos, SectionStateFacts,
    SectionSummary,
};
use helve_world_import::ImportedBlockSectionBuilder;

const BLOCK_SECTION_CELLS_U16: u16 = 4096;
const BLOCK_SECTION_CELLS_U64: u64 = 4096;

/// Deterministic construction accounting for the qualification direct-section baseline.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SectionBuildStats {
    /// Homogeneous final sections materialized directly from one semantic state.
    pub uniform_sections: u64,
    /// Non-uniform final sections materialized from importer cell scratch.
    pub dense_sections: u64,
    /// Semantic cells copied from reusable importer scratch into retained final sections.
    pub dense_cells_copied: u64,
    /// Total semantic cells written into retained direct-section storage, including uniform fills.
    pub retained_cells_written: u64,
}

/// Transparent 4096-cell section used only by end-to-end world-load qualification.
#[derive(Debug)]
pub struct QualificationDirectSection {
    cells: Box<[BlockStateId]>,
    non_air_count: u16,
    fluid_count: u16,
    random_block_count: u16,
    random_fluid_count: u16,
}

impl QualificationDirectSection {
    /// Materializes one uniform direct section.
    #[must_use]
    pub fn filled(state: BlockStateId) -> Self {
        let facts = GeneratedStateFacts.facts(state);
        Self {
            cells: vec![state; BLOCK_SECTION_CELLS].into_boxed_slice(),
            non_air_count: if facts.non_air() {
                BLOCK_SECTION_CELLS_U16
            } else {
                0
            },
            fluid_count: if facts.counted_fluid() {
                BLOCK_SECTION_CELLS_U16
            } else {
                0
            },
            random_block_count: if facts.random_block() {
                BLOCK_SECTION_CELLS_U16
            } else {
                0
            },
            random_fluid_count: if facts.random_fluid() {
                BLOCK_SECTION_CELLS_U16
            } else {
                0
            },
        }
    }

    fn from_states(states: &[BlockStateId]) -> Self {
        debug_assert_eq!(states.len(), BLOCK_SECTION_CELLS);
        let mut section = Self {
            cells: states.to_vec().into_boxed_slice(),
            non_air_count: 0,
            fluid_count: 0,
            random_block_count: 0,
            random_fluid_count: 0,
        };
        for &state in states {
            section.add_facts(GeneratedStateFacts.facts(state));
        }
        section
    }

    fn add_facts(&mut self, facts: SectionStateFacts) {
        self.non_air_count += u16::from(facts.non_air());
        self.fluid_count += u16::from(facts.counted_fluid());
        self.random_block_count += u16::from(facts.random_block());
        self.random_fluid_count += u16::from(facts.random_fluid());
    }

    fn remove_facts(&mut self, facts: SectionStateFacts) {
        debug_assert!(self.non_air_count >= u16::from(facts.non_air()));
        debug_assert!(self.fluid_count >= u16::from(facts.counted_fluid()));
        debug_assert!(self.random_block_count >= u16::from(facts.random_block()));
        debug_assert!(self.random_fluid_count >= u16::from(facts.random_fluid()));
        self.non_air_count -= u16::from(facts.non_air());
        self.fluid_count -= u16::from(facts.counted_fluid());
        self.random_block_count -= u16::from(facts.random_block());
        self.random_fluid_count -= u16::from(facts.random_fluid());
    }
}

impl BlockSection<BlockStateId> for QualificationDirectSection {
    #[inline]
    fn get(&self, pos: SectionBlockPos) -> BlockStateId {
        self.cells[pos.index()]
    }

    #[inline]
    fn replace<F: BlockStateFacts<BlockStateId>>(
        &mut self,
        pos: SectionBlockPos,
        state: BlockStateId,
        facts: &F,
    ) -> BlockStateId {
        let index = pos.index();
        let previous = self.cells[index];
        if previous == state {
            return previous;
        }
        self.remove_facts(facts.facts(previous));
        self.cells[index] = state;
        self.add_facts(facts.facts(state));
        previous
    }

    #[inline]
    fn summary(&self) -> SectionSummary {
        SectionSummary {
            non_air_count: self.non_air_count,
            fluid_count: self.fluid_count,
            random_block_present: self.random_block_count != 0,
            random_fluid_present: self.random_fluid_count != 0,
        }
    }

    fn maybe_contains<P: FnMut(BlockStateId) -> bool>(&self, predicate: P) -> bool {
        self.cells.iter().copied().any(predicate)
    }
}

/// Final-section builder used by the importer/residency qualification harness.
#[derive(Debug, Default)]
pub struct QualificationSectionBuilder {
    stats: SectionBuildStats,
}

impl QualificationSectionBuilder {
    /// Current deterministic section-materialization accounting.
    #[must_use]
    pub const fn stats(&self) -> SectionBuildStats {
        self.stats
    }
}

impl ImportedBlockSectionBuilder<BlockStateId> for QualificationSectionBuilder {
    type Section = QualificationDirectSection;

    fn build_uniform(&mut self, state: BlockStateId) -> Self::Section {
        self.stats.uniform_sections += 1;
        self.stats.retained_cells_written += BLOCK_SECTION_CELLS_U64;
        QualificationDirectSection::filled(state)
    }

    fn build_states(&mut self, states: &[BlockStateId]) -> Self::Section {
        debug_assert_eq!(states.len(), BLOCK_SECTION_CELLS);
        self.stats.dense_sections += 1;
        self.stats.dense_cells_copied += BLOCK_SECTION_CELLS_U64;
        self.stats.retained_cells_written += BLOCK_SECTION_CELLS_U64;
        QualificationDirectSection::from_states(states)
    }
}

#[cfg(test)]
mod tests {
    use helve_generated::{AIR, BlockStateId, GeneratedStateFacts};
    use helve_world_contract::{BlockSection, SectionBlockPos};
    use helve_world_import::ImportedBlockSectionBuilder;

    use super::{QualificationDirectSection, QualificationSectionBuilder};

    #[test]
    fn direct_builder_accounts_uniform_and_dense_materialization() {
        let stone = BlockStateId::new(1).expect("known target state identity");
        let mut builder = QualificationSectionBuilder::default();
        let uniform = builder.build_uniform(AIR);
        let mut cells = vec![AIR; 4096];
        cells[SectionBlockPos::new(3, 5, 7).expect("local pos").index()] = stone;
        let dense = builder.build_states(&cells);

        let build_stats = builder.stats();
        assert_eq!(build_stats.uniform_sections, 1);
        assert_eq!(build_stats.dense_sections, 1);
        assert_eq!(build_stats.dense_cells_copied, 4096);
        assert_eq!(build_stats.retained_cells_written, 8192);
        assert!(uniform.summary().has_only_air());
        assert_eq!(dense.summary().non_air_count, 1);
    }

    #[test]
    fn direct_section_mutation_keeps_exact_summary() {
        let stone = BlockStateId::new(1).expect("known target state identity");
        let mut section = QualificationDirectSection::filled(AIR);
        let pos = SectionBlockPos::new(1, 2, 3).expect("local pos");
        assert_eq!(section.replace(pos, stone, &GeneratedStateFacts), AIR);
        assert_eq!(section.get(pos), stone);
        assert_eq!(section.summary().non_air_count, 1);
        assert_eq!(section.replace(pos, AIR, &GeneratedStateFacts), stone);
        assert!(section.summary().has_only_air());
    }
}
