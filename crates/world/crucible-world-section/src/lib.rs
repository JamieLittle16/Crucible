//! Candidate live block-section storage for Crucible.
//!
//! This crate is a production-mechanism laboratory behind the source-backed world-section semantic
//! contract. It deliberately does not mirror Mojang's `PalettedContainer` representation ladder.
//! Candidates must remain replaceable until equivalence and benchmark evidence select a default.

#![forbid(unsafe_code)]

use std::mem;

use crucible_world_contract::{
    BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionBlockPos, SectionStateFacts,
    SectionSummary,
};

const LOCAL4_BYTES: usize = BLOCK_SECTION_CELLS / 2;
const LOCAL4_CAPACITY: usize = 16;
const LOCAL8_CAPACITY: usize = 256;

/// Physical representation currently used by an [`AdaptiveBlockSection`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepresentationKind {
    /// All 4096 cells hold one semantic state and no cell backing is allocated.
    Uniform,
    /// Four-bit stable local IDs backed by 2048 bytes.
    Local4,
    /// Byte-sized stable local IDs backed by 4096 bytes.
    Local8,
    /// Direct semantic state IDs/values for all 4096 cells.
    Direct,
}

#[derive(Clone, Debug)]
struct Local4<S: Copy + Eq> {
    palette: Vec<S>,
    indices: Box<[u8; LOCAL4_BYTES]>,
}

impl<S: Copy + Eq> Local4<S> {
    fn from_uniform(previous: S, changed_index: usize, state: S) -> Self {
        debug_assert!(previous != state);
        let mut result = Self {
            palette: Vec::with_capacity(LOCAL4_CAPACITY),
            indices: Box::new([0; LOCAL4_BYTES]),
        };
        result.palette.push(previous);
        result.palette.push(state);
        result.set_local_index(changed_index, 1);
        result
    }

    #[inline]
    fn local_index(&self, cell: usize) -> u8 {
        let byte = self.indices[cell >> 1];
        if cell & 1 == 0 {
            byte & 0x0f
        } else {
            byte >> 4
        }
    }

    #[inline]
    fn set_local_index(&mut self, cell: usize, local: u8) {
        debug_assert!(local < 16);
        let slot = &mut self.indices[cell >> 1];
        if cell & 1 == 0 {
            *slot = (*slot & 0xf0) | local;
        } else {
            *slot = (*slot & 0x0f) | (local << 4);
        }
    }

    #[inline]
    fn get(&self, cell: usize) -> S {
        self.palette[usize::from(self.local_index(cell))]
    }

    fn palette_index(&self, state: S) -> Option<u8> {
        self.palette
            .iter()
            .position(|candidate| *candidate == state)
            .map(|index| u8::try_from(index).expect("Local4 palette index always fits u8"))
    }

    fn try_replace(&mut self, cell: usize, state: S) -> bool {
        if let Some(local) = self.palette_index(state) {
            self.set_local_index(cell, local);
            return true;
        }
        if self.palette.len() == LOCAL4_CAPACITY {
            return false;
        }
        let local = u8::try_from(self.palette.len()).expect("Local4 palette index always fits u8");
        self.palette.push(state);
        self.set_local_index(cell, local);
        true
    }

    fn heap_bytes(&self) -> usize {
        LOCAL4_BYTES + self.palette.capacity() * mem::size_of::<S>()
    }
}

#[derive(Clone, Debug)]
struct Local8<S: Copy + Eq> {
    palette: Vec<S>,
    indices: Box<[u8; BLOCK_SECTION_CELLS]>,
}

impl<S: Copy + Eq> Local8<S> {
    fn from_local4(local4: Local4<S>) -> Self {
        let mut indices = Box::new([0; BLOCK_SECTION_CELLS]);
        for (cell, target) in indices.iter_mut().enumerate() {
            *target = local4.local_index(cell);
        }
        let mut palette = Vec::with_capacity(LOCAL8_CAPACITY);
        palette.extend(local4.palette);
        Self { palette, indices }
    }

    #[inline]
    fn get(&self, cell: usize) -> S {
        self.palette[usize::from(self.indices[cell])]
    }

    fn palette_index(&self, state: S) -> Option<u8> {
        self.palette
            .iter()
            .position(|candidate| *candidate == state)
            .map(|index| u8::try_from(index).expect("Local8 palette index always fits u8"))
    }

    fn try_replace(&mut self, cell: usize, state: S) -> bool {
        if let Some(local) = self.palette_index(state) {
            self.indices[cell] = local;
            return true;
        }
        if self.palette.len() == LOCAL8_CAPACITY {
            return false;
        }
        let local = u8::try_from(self.palette.len()).expect("Local8 palette index always fits u8");
        self.palette.push(state);
        self.indices[cell] = local;
        true
    }

    fn into_direct(self) -> Box<[S; BLOCK_SECTION_CELLS]> {
        let first = self.palette[0];
        let mut result = Box::new([first; BLOCK_SECTION_CELLS]);
        for (cell, target) in result.iter_mut().enumerate() {
            *target = self.palette[usize::from(self.indices[cell])];
        }
        result
    }

    fn heap_bytes(&self) -> usize {
        BLOCK_SECTION_CELLS + self.palette.capacity() * mem::size_of::<S>()
    }
}

#[derive(Clone, Debug)]
enum Storage<S: Copy + Eq> {
    Uniform(S),
    Local4(Box<Local4<S>>),
    Local8(Box<Local8<S>>),
    Direct(Box<[S; BLOCK_SECTION_CELLS]>),
}

impl<S: Copy + Eq> Storage<S> {
    #[inline]
    fn get(&self, cell: usize) -> S {
        match self {
            Self::Uniform(state) => *state,
            Self::Local4(storage) => storage.get(cell),
            Self::Local8(storage) => storage.get(cell),
            Self::Direct(storage) => storage[cell],
        }
    }

    fn kind(&self) -> RepresentationKind {
        match self {
            Self::Uniform(_) => RepresentationKind::Uniform,
            Self::Local4(_) => RepresentationKind::Local4,
            Self::Local8(_) => RepresentationKind::Local8,
            Self::Direct(_) => RepresentationKind::Direct,
        }
    }

    fn heap_bytes(&self) -> usize {
        match self {
            Self::Uniform(_) => 0,
            Self::Local4(storage) => storage.heap_bytes(),
            Self::Local8(storage) => storage.heap_bytes(),
            Self::Direct(_) => BLOCK_SECTION_CELLS * mem::size_of::<S>(),
        }
    }
}

/// Adaptive live-section candidate with tiny uniform state and bounded one-way promotion.
///
/// The representation ladder is intentionally short: `Uniform → Local4 → Local8 → Direct`.
/// Local palette entries are stable for the life of a representation and are not removed on small
/// mutations, avoiding O(section) cleanup. The current boxed backing is a laboratory allocation
/// strategy; owner-local arena allocation remains a separate experiment before `LiveChunkCore`.
#[derive(Clone, Debug)]
pub struct AdaptiveBlockSection<S: Copy + Eq> {
    storage: Storage<S>,
    non_air_count: u16,
    fluid_count: u16,
    random_block_count: u16,
    random_fluid_count: u16,
}

impl<S: Copy + Eq> AdaptiveBlockSection<S> {
    /// Creates a genuinely allocation-free uniform section.
    #[must_use]
    pub fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        const CELL_COUNT: u16 = 4096;
        let state_facts = facts.facts(state);
        Self {
            storage: Storage::Uniform(state),
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

    /// Returns the current physical representation for qualification/telemetry.
    #[must_use]
    pub fn representation(&self) -> RepresentationKind {
        self.storage.kind()
    }

    /// Returns the currently owned heap backing in bytes, excluding allocator metadata.
    ///
    /// This is deterministic diagnostic accounting for representation comparisons, not an RSS
    /// measurement. Final qualification must measure process-level resident memory separately.
    #[must_use]
    pub fn backing_bytes(&self) -> usize {
        self.storage.heap_bytes()
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

    fn replace_storage(&mut self, cell: usize, state: S) {
        match &mut self.storage {
            Storage::Uniform(previous) => {
                let old = *previous;
                debug_assert!(old != state);
                self.storage = Storage::Local4(Box::new(Local4::from_uniform(old, cell, state)));
            }
            Storage::Local4(storage) => {
                if storage.try_replace(cell, state) {
                    return;
                }
                let old_storage = mem::replace(&mut self.storage, Storage::Uniform(state));
                let Storage::Local4(storage) = old_storage else {
                    unreachable!("matched Local4 before replacement")
                };
                let mut promoted = Local8::from_local4(*storage);
                let inserted = promoted.try_replace(cell, state);
                debug_assert!(inserted);
                self.storage = Storage::Local8(Box::new(promoted));
            }
            Storage::Local8(storage) => {
                if storage.try_replace(cell, state) {
                    return;
                }
                let old_storage = mem::replace(&mut self.storage, Storage::Uniform(state));
                let Storage::Local8(storage) = old_storage else {
                    unreachable!("matched Local8 before replacement")
                };
                let mut direct = storage.into_direct();
                direct[cell] = state;
                self.storage = Storage::Direct(direct);
            }
            Storage::Direct(storage) => storage[cell] = state,
        }
    }
}

impl<S: Copy + Eq> BlockSection<S> for AdaptiveBlockSection<S> {
    #[inline]
    fn get(&self, pos: SectionBlockPos) -> S {
        self.storage.get(pos.index())
    }

    #[inline]
    fn replace<F: BlockStateFacts<S>>(&mut self, pos: SectionBlockPos, state: S, facts: &F) -> S {
        let cell = pos.index();
        let previous = self.storage.get(cell);
        if previous == state {
            return previous;
        }

        self.replace_storage(cell, state);
        self.remove_facts(facts.facts(previous));
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

    fn maybe_contains<P: FnMut(S) -> bool>(&self, mut predicate: P) -> bool {
        match &self.storage {
            Storage::Uniform(state) => predicate(*state),
            Storage::Local4(storage) => storage.palette.iter().copied().any(predicate),
            Storage::Local8(storage) => storage.palette.iter().copied().any(predicate),
            Storage::Direct(storage) => storage.iter().copied().any(predicate),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::mem;

    use super::{AdaptiveBlockSection, RepresentationKind};
    use crucible_world_contract::{
        BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionBlockPos, SectionStateFacts,
    };
    use crucible_world_reference::DirectBlockSection;

    struct SyntheticFacts;

    impl BlockStateFacts<u16> for SyntheticFacts {
        fn facts(&self, state: u16) -> SectionStateFacts {
            let non_air = state != 0;
            SectionStateFacts::new(
                non_air,
                non_air && state.is_multiple_of(5),
                non_air && state.is_multiple_of(7),
                non_air && state.is_multiple_of(11),
            )
        }
    }

    fn pos(index: usize) -> SectionBlockPos {
        let x = u8::try_from(index & 15).expect("bounded x");
        let z = u8::try_from((index >> 4) & 15).expect("bounded z");
        let y = u8::try_from((index >> 8) & 15).expect("bounded y");
        SectionBlockPos::new(x, y, z).expect("decoded section coordinate")
    }

    #[test]
    fn uniform_section_has_no_cell_backing() {
        let mut section = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        assert_eq!(section.representation(), RepresentationKind::Uniform);
        assert_eq!(section.backing_bytes(), 0);
        assert_eq!(section.replace(pos(17), 0, &SyntheticFacts), 0);
        assert_eq!(section.representation(), RepresentationKind::Uniform);
        assert_eq!(section.backing_bytes(), 0);
    }

    #[test]
    fn promotion_ladder_is_bounded_and_preserves_cells() {
        let mut section = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        section.replace(pos(0), 1, &SyntheticFacts);
        assert_eq!(section.representation(), RepresentationKind::Local4);
        assert!(section.backing_bytes() >= 2048);

        for state in 2_u16..=16 {
            let index = usize::from(state - 1);
            section.replace(pos(index), state, &SyntheticFacts);
        }
        assert_eq!(section.representation(), RepresentationKind::Local8);
        assert!(section.backing_bytes() >= 4096);

        for state in 17_u16..=256 {
            let index = usize::from(state - 1);
            section.replace(pos(index), state, &SyntheticFacts);
        }
        assert_eq!(section.representation(), RepresentationKind::Direct);
        assert_eq!(
            section.backing_bytes(),
            BLOCK_SECTION_CELLS * mem::size_of::<u16>()
        );

        for state in 1_u16..=256 {
            let index = usize::from(state - 1);
            assert_eq!(section.get(pos(index)), state);
        }
        assert_eq!(section.get(pos(256)), 0);
    }

    #[test]
    fn long_trace_is_differentially_equivalent_to_direct_oracle() {
        let mut candidate = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        let mut reference = DirectBlockSection::filled(0_u16, &SyntheticFacts);
        let mut rng = 0xA076_1D64_78BD_642F_u64;
        let cell_count = u64::try_from(BLOCK_SECTION_CELLS).expect("section size fits u64");

        for step in 0..100_000_u32 {
            rng ^= rng << 7;
            rng ^= rng >> 9;
            rng ^= rng << 8;
            let index = usize::try_from(rng % cell_count).expect("bounded index");
            let state = u16::try_from((rng >> 24) % 320).expect("synthetic state fits u16");
            let position = pos(index);

            assert_eq!(
                candidate.replace(position, state, &SyntheticFacts),
                reference.replace(position, state, &SyntheticFacts)
            );
            assert_eq!(candidate.summary(), reference.summary());

            if step.is_multiple_of(251) {
                let probe = usize::try_from((rng >> 12) % cell_count).expect("bounded probe");
                assert_eq!(candidate.get(pos(probe)), reference.get(pos(probe)));
            }
        }

        for index in 0..BLOCK_SECTION_CELLS {
            assert_eq!(candidate.get(pos(index)), reference.get(pos(index)));
        }
    }

    #[test]
    fn palette_membership_is_conservative_after_state_disappears() {
        let mut section = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        let target = pos(42);
        section.replace(target, 9, &SyntheticFacts);
        section.replace(target, 0, &SyntheticFacts);

        assert!(section.maybe_contains(|state| state == 9));
        assert!(!section.maybe_contains(|state| state == 777));
    }

    #[test]
    fn cloning_is_semantically_independent_across_backings() {
        let mut original = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        for state in 1_u16..20 {
            original.replace(pos(usize::from(state)), state, &SyntheticFacts);
        }
        let mut clone = original.clone();
        clone.replace(pos(3), 299, &SyntheticFacts);
        assert_ne!(clone.get(pos(3)), original.get(pos(3)));
        assert_eq!(original.get(pos(3)), 3);
    }
}
