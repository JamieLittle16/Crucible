//! Deliberately simple correctness oracles for Crucible world semantics.
//!
//! These structures are not production storage candidates. They trade memory for transparency and
//! are intended to remain independent enough to catch bugs in optimized representations.

#![forbid(unsafe_code)]

use helve_world_contract::{
    BIOME_SECTION_CELLS, BLOCK_SECTION_CELLS, BiomeSection, BlockSection, BlockStateFacts,
    SectionBiomePos, SectionBlockPos, SectionStateFacts, SectionSummary,
};

/// Direct 4096-cell reference block section with independently maintained summary witnesses.
#[derive(Clone, Debug)]
pub struct DirectBlockSection<S: Copy + Eq> {
    cells: Box<[S; BLOCK_SECTION_CELLS]>,
    non_air_count: u16,
    fluid_count: u16,
    random_block_count: u16,
    random_fluid_count: u16,
}

impl<S: Copy + Eq> DirectBlockSection<S> {
    /// Creates a section filled with one state and derives exact summary witnesses from its facts.
    #[must_use]
    pub fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        const CELL_COUNT: u16 = 4096;
        let state_facts = facts.facts(state);
        Self {
            cells: Box::new([state; BLOCK_SECTION_CELLS]),
            non_air_count: if state_facts.non_air() { CELL_COUNT } else { 0 },
            fluid_count: if state_facts.counted_fluid() {
                CELL_COUNT
            } else {
                0
            },
            random_block_count: if state_facts.random_block() {
                CELL_COUNT
            } else {
                0
            },
            random_fluid_count: if state_facts.random_fluid() {
                CELL_COUNT
            } else {
                0
            },
        }
    }

    /// Recomputes all exact summaries by scanning the semantic cells.
    #[must_use]
    pub fn recompute_summary<F: BlockStateFacts<S>>(&self, facts: &F) -> SectionSummary {
        let mut non_air_count = 0_u16;
        let mut fluid_count = 0_u16;
        let mut random_block_count = 0_u16;
        let mut random_fluid_count = 0_u16;

        for &state in self.cells.iter() {
            let state_facts = facts.facts(state);
            non_air_count += u16::from(state_facts.non_air());
            fluid_count += u16::from(state_facts.counted_fluid());
            random_block_count += u16::from(state_facts.random_block());
            random_fluid_count += u16::from(state_facts.random_fluid());
        }

        SectionSummary {
            non_air_count,
            fluid_count,
            random_block_present: random_block_count != 0,
            random_fluid_present: random_fluid_count != 0,
        }
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

impl<S: Copy + Eq> BlockSection<S> for DirectBlockSection<S> {
    #[inline]
    fn get(&self, pos: SectionBlockPos) -> S {
        self.cells[pos.index()]
    }

    #[inline]
    fn replace<F: BlockStateFacts<S>>(&mut self, pos: SectionBlockPos, state: S, facts: &F) -> S {
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

    fn maybe_contains<P: FnMut(S) -> bool>(&self, predicate: P) -> bool {
        self.cells.iter().copied().any(predicate)
    }
}

/// Direct 64-cell reference biome lattice.
#[derive(Clone, Debug)]
pub struct DirectBiomeSection<B: Copy + Eq> {
    cells: Box<[B; BIOME_SECTION_CELLS]>,
}

impl<B: Copy + Eq> DirectBiomeSection<B> {
    /// Creates a biome lattice filled with one biome identity.
    #[must_use]
    pub fn filled(biome: B) -> Self {
        Self {
            cells: Box::new([biome; BIOME_SECTION_CELLS]),
        }
    }

    /// Returns the exact biome identity at `pos`.
    #[inline]
    #[must_use]
    pub fn get(&self, pos: SectionBiomePos) -> B {
        self.cells[pos.index()]
    }

    /// Replaces one biome identity and returns the previous identity.
    #[inline]
    pub fn replace(&mut self, pos: SectionBiomePos, biome: B) -> B {
        let previous = self.cells[pos.index()];
        self.cells[pos.index()] = biome;
        previous
    }

    /// Fills the whole biome lattice from a coordinate function.
    ///
    /// The call order intentionally matches the target 26.2 implementation: x outermost, then y,
    /// then z. Storage indexing remains the independently specified biome linearization.
    pub fn fill_with<F: FnMut(u8, u8, u8) -> B>(&mut self, mut resolve: F) {
        for x in 0..4 {
            for y in 0..4 {
                for z in 0..4 {
                    if let Some(pos) = SectionBiomePos::new(x, y, z) {
                        self.cells[pos.index()] = resolve(x, y, z);
                    }
                }
            }
        }
    }
}

impl<B: Copy + Eq> BiomeSection<B> for DirectBiomeSection<B> {
    #[inline]
    fn get(&self, pos: SectionBiomePos) -> B {
        self.cells[pos.index()]
    }

    #[inline]
    fn replace(&mut self, pos: SectionBiomePos, biome: B) -> B {
        let previous = self.cells[pos.index()];
        self.cells[pos.index()] = biome;
        previous
    }
}

#[cfg(test)]
mod tests {
    use super::{DirectBiomeSection, DirectBlockSection};
    use helve_world_contract::{
        BIOME_SECTION_CELLS, BLOCK_SECTION_CELLS, BiomeSection, BlockSection, BlockStateFacts,
        SectionBiomePos, SectionBlockPos, SectionStateFacts, SectionSummary,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum State {
        Air,
        Solid,
        Fluid,
        RandomBlock,
        RandomFluid,
        BothRandom,
    }

    struct Facts;

    impl BlockStateFacts<State> for Facts {
        fn facts(&self, state: State) -> SectionStateFacts {
            match state {
                State::Air => SectionStateFacts::new(false, false, false, false),
                State::Solid => SectionStateFacts::new(true, false, false, false),
                State::Fluid => SectionStateFacts::new(true, true, false, false),
                State::RandomBlock => SectionStateFacts::new(true, false, true, false),
                State::RandomFluid => SectionStateFacts::new(true, true, false, true),
                State::BothRandom => SectionStateFacts::new(true, true, true, true),
            }
        }
    }

    fn replace_through_biome_contract<Section: BiomeSection<u8>>(
        section: &mut Section,
        pos: SectionBiomePos,
        biome: u8,
    ) -> u8 {
        section.replace(pos, biome)
    }

    #[test]
    fn filled_section_has_exact_summary() {
        let section = DirectBlockSection::filled(State::Fluid, &Facts);
        assert_eq!(
            section.summary(),
            SectionSummary {
                non_air_count: 4096,
                fluid_count: 4096,
                random_block_present: false,
                random_fluid_present: false,
            }
        );
        assert_eq!(section.summary(), section.recompute_summary(&Facts));
    }

    #[test]
    fn mutation_updates_independent_witnesses() {
        let mut section = DirectBlockSection::filled(State::Air, &Facts);
        let pos = SectionBlockPos::new(3, 5, 7).expect("valid coordinate");
        assert_eq!(section.replace(pos, State::BothRandom, &Facts), State::Air);
        assert_eq!(section.get(pos), State::BothRandom);
        assert_eq!(section.summary().non_air_count, 1);
        assert_eq!(section.summary().fluid_count, 1);
        assert!(section.summary().random_block_present);
        assert!(section.summary().random_fluid_present);
        assert_eq!(section.summary(), section.recompute_summary(&Facts));

        assert_eq!(section.replace(pos, State::Air, &Facts), State::BothRandom);
        assert_eq!(section.summary(), SectionSummary::default());
        assert_eq!(section.summary(), section.recompute_summary(&Facts));
    }

    #[test]
    fn same_value_replacement_is_semantically_noop() {
        let mut section = DirectBlockSection::filled(State::Solid, &Facts);
        let before = section.summary();
        let pos = SectionBlockPos::new(15, 15, 15).expect("valid coordinate");
        assert_eq!(section.replace(pos, State::Solid, &Facts), State::Solid);
        assert_eq!(section.summary(), before);
    }

    #[test]
    fn long_mutation_trace_matches_full_recomputation() {
        let states = [
            State::Air,
            State::Solid,
            State::Fluid,
            State::RandomBlock,
            State::RandomFluid,
            State::BothRandom,
        ];
        let mut section = DirectBlockSection::filled(State::Air, &Facts);
        let mut rng = 0xD1B5_4A32_D192_ED03_u64;
        let cell_count = u64::try_from(BLOCK_SECTION_CELLS).expect("section size fits u64");
        let state_count = u64::try_from(states.len()).expect("state count fits u64");

        for step in 0..50_000_u32 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let index = usize::try_from(rng % cell_count).expect("bounded");
            let x = u8::try_from(index & 15).expect("bounded");
            let z = u8::try_from((index >> 4) & 15).expect("bounded");
            let y = u8::try_from((index >> 8) & 15).expect("bounded");
            let state_index = usize::try_from((rng >> 32) % state_count).expect("bounded");
            let state = states[state_index];
            let pos = SectionBlockPos::new(x, y, z).expect("decoded coordinate");
            section.replace(pos, state, &Facts);

            if step % 97 == 0 {
                assert_eq!(section.summary(), section.recompute_summary(&Facts));
            }
        }

        assert_eq!(section.summary(), section.recompute_summary(&Facts));
    }

    #[test]
    fn exact_reference_satisfies_conservative_query_contract() {
        let mut section = DirectBlockSection::filled(State::Air, &Facts);
        let pos = SectionBlockPos::new(1, 2, 3).expect("valid coordinate");
        section.replace(pos, State::RandomBlock, &Facts);
        assert!(section.maybe_contains(|state| state == State::RandomBlock));
        assert!(!section.maybe_contains(|state| state == State::RandomFluid));
    }

    #[test]
    fn biome_fill_matches_target_iteration_order_and_indexing() {
        let mut section = DirectBiomeSection::filled(0_u8);
        let mut calls = Vec::new();
        section.fill_with(|x, y, z| {
            calls.push((x, y, z));
            (y << 4) | (z << 2) | x
        });

        assert_eq!(calls.len(), BIOME_SECTION_CELLS);
        assert_eq!(calls[0], (0, 0, 0));
        assert_eq!(calls[1], (0, 0, 1));
        assert_eq!(calls[4], (0, 1, 0));
        assert_eq!(calls[16], (1, 0, 0));

        for y in 0..4 {
            for z in 0..4 {
                for x in 0..4 {
                    let pos = SectionBiomePos::new(x, y, z).expect("bounded");
                    assert_eq!(section.get(pos), (y << 4) | (z << 2) | x);
                }
            }
        }
    }

    #[test]
    fn direct_biome_reference_satisfies_semantic_contract() {
        let mut section = DirectBiomeSection::filled(3_u8);
        let pos = SectionBiomePos::new(2, 1, 3).expect("bounded");
        assert_eq!(replace_through_biome_contract(&mut section, pos, 9), 3);
        assert_eq!(BiomeSection::get(&section, pos), 9);
    }

    #[test]
    fn clone_is_storage_independent() {
        let original = DirectBlockSection::filled(State::Air, &Facts);
        let mut copy = original.clone();
        let pos = SectionBlockPos::new(2, 2, 2).expect("valid coordinate");
        copy.replace(pos, State::Solid, &Facts);
        assert_eq!(original.get(pos), State::Air);
        assert_eq!(copy.get(pos), State::Solid);
    }
}
