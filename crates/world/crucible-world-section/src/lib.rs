//! Candidate live block-section storage for Crucible.
//!
//! This crate is a production-mechanism laboratory behind the source-backed world-section semantic
//! contract. It deliberately does not mirror Mojang's `PalettedContainer` representation ladder.
//! Candidates remain replaceable until differential correctness and benchmark evidence select a
//! production default.

#![forbid(unsafe_code)]

use std::mem;

use crucible_world_contract::{
    BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionBlockPos, SectionStateFacts,
    SectionSummary,
};

const SECTION_CELL_COUNT_U16: u16 = 4096;
const LOCAL4_BYTES: usize = BLOCK_SECTION_CELLS / 2;
const LOCAL4_CAPACITY: usize = 16;
const LOCAL8_CAPACITY: usize = 256;
const _: () = assert!(BLOCK_SECTION_CELLS == 4096);

/// Physical representation currently used by an [`AdaptiveBlockSection`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepresentationKind {
    /// All 4096 cells hold one state and no cell backing is heap allocated.
    Uniform,
    /// Four-bit stable local IDs with at most 16 simultaneously live states.
    Local4Stable,
    /// Byte-sized stable local IDs with at most 256 simultaneously live states.
    Local8Stable,
    /// Direct semantic state IDs for all 4096 cells.
    DirectN,
}

#[derive(Clone, Copy, Debug)]
struct PaletteSlot<S: Copy> {
    state: S,
    uses: u16,
}

impl<S: Copy> PaletteSlot<S> {
    const fn new(state: S, uses: u16) -> Self {
        Self { state, uses }
    }

    const fn is_live(self) -> bool {
        self.uses != 0
    }
}

#[derive(Clone, Copy, Debug)]
struct LiveHeader {
    non_air_count: u16,
    fluid_count: u16,
    random_block_count: u16,
    random_fluid_count: u16,
}

impl LiveHeader {
    fn filled<S: Copy, F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        let state_facts = facts.facts(state);
        Self {
            non_air_count: SECTION_CELL_COUNT_U16 * u16::from(state_facts.non_air()),
            fluid_count: SECTION_CELL_COUNT_U16 * u16::from(state_facts.counted_fluid()),
            random_block_count: SECTION_CELL_COUNT_U16 * u16::from(state_facts.random_block()),
            random_fluid_count: SECTION_CELL_COUNT_U16 * u16::from(state_facts.random_fluid()),
        }
    }

    fn apply_change(&mut self, previous: SectionStateFacts, next: SectionStateFacts) {
        debug_assert!(self.non_air_count >= u16::from(previous.non_air()));
        debug_assert!(self.fluid_count >= u16::from(previous.counted_fluid()));
        debug_assert!(self.random_block_count >= u16::from(previous.random_block()));
        debug_assert!(self.random_fluid_count >= u16::from(previous.random_fluid()));

        self.non_air_count =
            self.non_air_count - u16::from(previous.non_air()) + u16::from(next.non_air());
        self.fluid_count = self.fluid_count - u16::from(previous.counted_fluid())
            + u16::from(next.counted_fluid());
        self.random_block_count = self.random_block_count - u16::from(previous.random_block())
            + u16::from(next.random_block());
        self.random_fluid_count = self.random_fluid_count - u16::from(previous.random_fluid())
            + u16::from(next.random_fluid());
    }

    const fn summary(self) -> SectionSummary {
        SectionSummary {
            non_air_count: self.non_air_count,
            fluid_count: self.fluid_count,
            random_block_present: self.random_block_count != 0,
            random_fluid_present: self.random_fluid_count != 0,
        }
    }
}

#[derive(Clone, Debug)]
struct Local4<S: Copy + Eq> {
    palette: Vec<PaletteSlot<S>>,
    indices: Box<[u8; LOCAL4_BYTES]>,
}

impl<S: Copy + Eq> Local4<S> {
    fn from_uniform(previous: S, changed_cell: usize, state: S) -> Self {
        debug_assert!(previous != state);
        let mut palette = Vec::with_capacity(LOCAL4_CAPACITY);
        palette.push(PaletteSlot::new(previous, SECTION_CELL_COUNT_U16 - 1));
        palette.push(PaletteSlot::new(state, 1));

        let mut result = Self {
            palette,
            indices: Box::new([0; LOCAL4_BYTES]),
        };
        result.set_local_index(changed_cell, 1);
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
        let byte = &mut self.indices[cell >> 1];
        if cell & 1 == 0 {
            *byte = (*byte & 0xf0) | local;
        } else {
            *byte = (*byte & 0x0f) | (local << 4);
        }
    }

    #[inline]
    fn get(&self, cell: usize) -> S {
        self.palette[usize::from(self.local_index(cell))].state
    }

    fn live_palette_index(&self, state: S) -> Option<u8> {
        self.palette
            .iter()
            .position(|slot| slot.is_live() && slot.state == state)
            .map(|index| u8::try_from(index).expect("Local4 palette index always fits u8"))
    }

    fn dead_palette_index(&self) -> Option<u8> {
        self.palette
            .iter()
            .position(|slot| !slot.is_live())
            .map(|index| u8::try_from(index).expect("Local4 palette index always fits u8"))
    }

    fn try_replace(&mut self, cell: usize, state: S) -> bool {
        let old_local = self.local_index(cell);
        let old_index = usize::from(old_local);
        debug_assert!(self.palette[old_index].uses != 0);
        if self.palette[old_index].state == state {
            return true;
        }

        if let Some(target_local) = self.live_palette_index(state) {
            let target_index = usize::from(target_local);
            self.palette[old_index].uses -= 1;
            self.palette[target_index].uses += 1;
            self.set_local_index(cell, target_local);
            return true;
        }

        if let Some(target_local) = self.dead_palette_index() {
            let target_index = usize::from(target_local);
            self.palette[old_index].uses -= 1;
            self.palette[target_index] = PaletteSlot::new(state, 1);
            self.set_local_index(cell, target_local);
            return true;
        }

        if self.palette.len() < LOCAL4_CAPACITY {
            let target_local =
                u8::try_from(self.palette.len()).expect("Local4 palette index always fits u8");
            self.palette[old_index].uses -= 1;
            self.palette.push(PaletteSlot::new(state, 1));
            self.set_local_index(cell, target_local);
            return true;
        }

        // A full palette does not imply a promotion when the overwritten state dies here. Reusing
        // its own stable slot preserves the 16-state simultaneous-live bound without touching any
        // other cell index.
        if self.palette[old_index].uses == 1 {
            self.palette[old_index].state = state;
            return true;
        }

        false
    }

    fn live_entries(&self) -> usize {
        self.palette.iter().filter(|slot| slot.is_live()).count()
    }

    fn heap_bytes(&self) -> usize {
        LOCAL4_BYTES + self.palette.capacity() * mem::size_of::<PaletteSlot<S>>()
    }
}

#[derive(Clone, Debug)]
struct Local8<S: Copy + Eq> {
    palette: Vec<PaletteSlot<S>>,
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
        self.palette[usize::from(self.indices[cell])].state
    }

    fn live_palette_index(&self, state: S) -> Option<u8> {
        self.palette
            .iter()
            .position(|slot| slot.is_live() && slot.state == state)
            .map(|index| u8::try_from(index).expect("Local8 palette index always fits u8"))
    }

    fn dead_palette_index(&self) -> Option<u8> {
        self.palette
            .iter()
            .position(|slot| !slot.is_live())
            .map(|index| u8::try_from(index).expect("Local8 palette index always fits u8"))
    }

    fn try_replace(&mut self, cell: usize, state: S) -> bool {
        let old_local = self.indices[cell];
        let old_index = usize::from(old_local);
        debug_assert!(self.palette[old_index].uses != 0);
        if self.palette[old_index].state == state {
            return true;
        }

        if let Some(target_local) = self.live_palette_index(state) {
            let target_index = usize::from(target_local);
            self.palette[old_index].uses -= 1;
            self.palette[target_index].uses += 1;
            self.indices[cell] = target_local;
            return true;
        }

        if let Some(target_local) = self.dead_palette_index() {
            let target_index = usize::from(target_local);
            self.palette[old_index].uses -= 1;
            self.palette[target_index] = PaletteSlot::new(state, 1);
            self.indices[cell] = target_local;
            return true;
        }

        if self.palette.len() < LOCAL8_CAPACITY {
            let target_local =
                u8::try_from(self.palette.len()).expect("Local8 palette index always fits u8");
            self.palette[old_index].uses -= 1;
            self.palette.push(PaletteSlot::new(state, 1));
            self.indices[cell] = target_local;
            return true;
        }

        if self.palette[old_index].uses == 1 {
            self.palette[old_index].state = state;
            return true;
        }

        false
    }

    fn live_entries(&self) -> usize {
        self.palette.iter().filter(|slot| slot.is_live()).count()
    }

    fn into_direct(self) -> Box<[S; BLOCK_SECTION_CELLS]> {
        let mut cells = Box::new([self.palette[0].state; BLOCK_SECTION_CELLS]);
        for (cell, target) in cells.iter_mut().enumerate() {
            *target = self.palette[usize::from(self.indices[cell])].state;
        }
        cells
    }

    fn heap_bytes(&self) -> usize {
        BLOCK_SECTION_CELLS + self.palette.capacity() * mem::size_of::<PaletteSlot<S>>()
    }
}

#[derive(Clone, Debug)]
enum Storage<S: Copy + Eq> {
    Uniform(S),
    Local4(Local4<S>),
    Local8(Local8<S>),
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

    const fn kind(&self) -> RepresentationKind {
        match self {
            Self::Uniform(_) => RepresentationKind::Uniform,
            Self::Local4(_) => RepresentationKind::Local4Stable,
            Self::Local8(_) => RepresentationKind::Local8Stable,
            Self::Direct(_) => RepresentationKind::DirectN,
        }
    }

    fn live_palette_entries(&self) -> Option<usize> {
        match self {
            Self::Uniform(_) => Some(1),
            Self::Local4(storage) => Some(storage.live_entries()),
            Self::Local8(storage) => Some(storage.live_entries()),
            Self::Direct(_) => None,
        }
    }

    fn heap_bytes(&self) -> usize {
        match self {
            Self::Uniform(_) => 0,
            Self::Local4(storage) => storage.heap_bytes(),
            Self::Local8(storage) => storage.heap_bytes(),
            Self::Direct(storage) => mem::size_of_val(storage.as_ref()),
        }
    }
}

/// Promotion-only live section using stable local palette slots.
///
/// The mechanism is `Uniform -> Local4Stable -> Local8Stable -> DirectN`. Local palette slots carry
/// exact usage counts, so dead slots are reusable without rewriting the 4096-cell index backing.
/// Promotion depends on simultaneously live cardinality rather than historical palette churn.
#[derive(Clone, Debug)]
pub struct AdaptiveBlockSection<S: Copy + Eq> {
    storage: Storage<S>,
    header: LiveHeader,
}

impl<S: Copy + Eq> AdaptiveBlockSection<S> {
    /// Creates a homogeneous section without heap-allocating cell backing.
    #[must_use]
    pub fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        Self {
            storage: Storage::Uniform(state),
            header: LiveHeader::filled(state, facts),
        }
    }

    /// Returns the current physical representation for qualification and benchmark telemetry.
    #[must_use]
    pub const fn representation(&self) -> RepresentationKind {
        self.storage.kind()
    }

    /// Returns heap bytes owned by the current backing, excluding allocator metadata.
    #[must_use]
    pub fn backing_bytes(&self) -> usize {
        self.storage.heap_bytes()
    }

    /// Returns deterministic object-plus-backing bytes, excluding allocator metadata.
    #[must_use]
    pub fn owned_bytes(&self) -> usize {
        mem::size_of::<Self>() + self.backing_bytes()
    }

    /// Returns the number of simultaneously live local palette entries when palette-backed.
    ///
    /// `DirectN` returns `None` because it has no local palette.
    #[must_use]
    pub fn live_palette_entries(&self) -> Option<usize> {
        self.storage.live_palette_entries()
    }

    fn replace_storage(&mut self, cell: usize, state: S) {
        match &mut self.storage {
            Storage::Uniform(previous) => {
                let previous = *previous;
                debug_assert!(previous != state);
                self.storage = Storage::Local4(Local4::from_uniform(previous, cell, state));
            }
            Storage::Local4(storage) => {
                if storage.try_replace(cell, state) {
                    return;
                }

                let old_storage = mem::replace(&mut self.storage, Storage::Uniform(state));
                let Storage::Local4(storage) = old_storage else {
                    unreachable!("matched Local4 before replacement")
                };
                let mut promoted = Local8::from_local4(storage);
                let inserted = promoted.try_replace(cell, state);
                debug_assert!(inserted);
                self.storage = Storage::Local8(promoted);
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
        self.header
            .apply_change(facts.facts(previous), facts.facts(state));
        previous
    }

    #[inline]
    fn summary(&self) -> SectionSummary {
        self.header.summary()
    }

    fn maybe_contains<P: FnMut(S) -> bool>(&self, mut predicate: P) -> bool {
        match &self.storage {
            Storage::Uniform(state) => predicate(*state),
            Storage::Local4(storage) => storage
                .palette
                .iter()
                .filter(|slot| slot.is_live())
                .map(|slot| slot.state)
                .any(predicate),
            Storage::Local8(storage) => storage
                .palette
                .iter()
                .filter(|slot| slot.is_live())
                .map(|slot| slot.state)
                .any(predicate),
            Storage::Direct(storage) => storage.iter().copied().any(predicate),
        }
    }
}

/// Direct 4096-state CPU baseline for the live-representation laboratory.
///
/// This is intentionally separate from `crucible-world-reference::DirectBlockSection`: the latter
/// remains the correctness oracle, while this type is a benchmark candidate with lab diagnostics.
#[derive(Clone, Debug)]
pub struct DirectNBlockSection<S: Copy + Eq> {
    cells: Box<[S; BLOCK_SECTION_CELLS]>,
    header: LiveHeader,
}

impl<S: Copy + Eq> DirectNBlockSection<S> {
    /// Creates a direct section filled with one semantic state.
    #[must_use]
    pub fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        Self {
            cells: Box::new([state; BLOCK_SECTION_CELLS]),
            header: LiveHeader::filled(state, facts),
        }
    }

    /// Returns exact heap bytes owned by the direct cell backing, excluding allocator metadata.
    #[must_use]
    pub fn backing_bytes(&self) -> usize {
        mem::size_of_val(self.cells.as_ref())
    }

    /// Returns deterministic object-plus-backing bytes, excluding allocator metadata.
    #[must_use]
    pub fn owned_bytes(&self) -> usize {
        mem::size_of::<Self>() + self.backing_bytes()
    }
}

impl<S: Copy + Eq> BlockSection<S> for DirectNBlockSection<S> {
    #[inline]
    fn get(&self, pos: SectionBlockPos) -> S {
        self.cells[pos.index()]
    }

    #[inline]
    fn replace<F: BlockStateFacts<S>>(&mut self, pos: SectionBlockPos, state: S, facts: &F) -> S {
        let cell = pos.index();
        let previous = self.cells[cell];
        if previous == state {
            return previous;
        }

        self.cells[cell] = state;
        self.header
            .apply_change(facts.facts(previous), facts.facts(state));
        previous
    }

    #[inline]
    fn summary(&self) -> SectionSummary {
        self.header.summary()
    }

    fn maybe_contains<P: FnMut(S) -> bool>(&self, predicate: P) -> bool {
        self.cells.iter().copied().any(predicate)
    }
}

#[cfg(test)]
mod tests {
    use std::mem;

    use super::{AdaptiveBlockSection, DirectNBlockSection, PaletteSlot, RepresentationKind};
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

    fn assert_matches_reference<C: BlockSection<u16>>(
        candidate: &C,
        reference: &DirectBlockSection<u16>,
    ) {
        for index in 0..BLOCK_SECTION_CELLS {
            assert_eq!(candidate.get(pos(index)), reference.get(pos(index)));
        }
        assert_eq!(candidate.summary(), reference.summary());
        assert_eq!(
            reference.summary(),
            reference.recompute_summary(&SyntheticFacts)
        );
    }

    fn replace_both<C: BlockSection<u16>>(
        candidate: &mut C,
        reference: &mut DirectBlockSection<u16>,
        index: usize,
        state: u16,
    ) {
        let candidate_previous = candidate.replace(pos(index), state, &SyntheticFacts);
        let reference_previous = reference.replace(pos(index), state, &SyntheticFacts);
        assert_eq!(candidate_previous, reference_previous);
        assert_eq!(candidate.summary(), reference.summary());
    }

    #[test]
    fn uniform_section_has_no_heap_backing() {
        let mut section = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        assert_eq!(section.representation(), RepresentationKind::Uniform);
        assert_eq!(section.backing_bytes(), 0);
        assert_eq!(section.live_palette_entries(), Some(1));
        assert_eq!(section.replace(pos(17), 0, &SyntheticFacts), 0);
        assert_eq!(section.representation(), RepresentationKind::Uniform);
        assert_eq!(section.backing_bytes(), 0);
    }

    #[test]
    fn local4_reuses_preexisting_dead_slot_without_promotion() {
        let mut candidate = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        let mut reference = DirectBlockSection::filled(0_u16, &SyntheticFacts);

        for state in 1_u16..=15 {
            replace_both(
                &mut candidate,
                &mut reference,
                usize::from(state - 1),
                state,
            );
        }
        assert_eq!(candidate.representation(), RepresentationKind::Local4Stable);
        assert_eq!(candidate.live_palette_entries(), Some(16));

        replace_both(&mut candidate, &mut reference, 0, 0);
        assert_eq!(candidate.live_palette_entries(), Some(15));
        assert!(!candidate.maybe_contains(|state| state == 1));

        replace_both(&mut candidate, &mut reference, 15, 16);
        assert_eq!(candidate.representation(), RepresentationKind::Local4Stable);
        assert_eq!(candidate.live_palette_entries(), Some(16));
        assert!(candidate.maybe_contains(|state| state == 16));
        assert_matches_reference(&candidate, &reference);
    }

    #[test]
    fn full_local4_reuses_last_use_slot_before_promoting() {
        let mut candidate = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        let mut reference = DirectBlockSection::filled(0_u16, &SyntheticFacts);

        for state in 1_u16..=15 {
            replace_both(
                &mut candidate,
                &mut reference,
                usize::from(state - 1),
                state,
            );
        }
        assert_eq!(candidate.live_palette_entries(), Some(16));

        // State 1 is used exactly once. Replacing it with a new state keeps simultaneous cardinality
        // at 16 and must therefore reuse its own slot rather than promote.
        replace_both(&mut candidate, &mut reference, 0, 16);
        assert_eq!(candidate.representation(), RepresentationKind::Local4Stable);
        assert_eq!(candidate.live_palette_entries(), Some(16));
        assert!(!candidate.maybe_contains(|state| state == 1));
        assert_matches_reference(&candidate, &reference);
    }

    #[test]
    fn local4_promotes_only_on_seventeenth_simultaneously_live_state() {
        let mut candidate = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        let mut reference = DirectBlockSection::filled(0_u16, &SyntheticFacts);

        for state in 1_u16..=15 {
            replace_both(
                &mut candidate,
                &mut reference,
                usize::from(state - 1),
                state,
            );
        }
        assert_eq!(candidate.representation(), RepresentationKind::Local4Stable);
        assert_eq!(candidate.live_palette_entries(), Some(16));

        replace_both(&mut candidate, &mut reference, 15, 16);
        assert_eq!(candidate.representation(), RepresentationKind::Local8Stable);
        assert_eq!(candidate.live_palette_entries(), Some(17));
        assert_matches_reference(&candidate, &reference);
    }

    #[test]
    fn local8_reuses_dead_slots_and_promotes_only_on_257_live_states() {
        let mut candidate = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        let mut reference = DirectBlockSection::filled(0_u16, &SyntheticFacts);

        for state in 1_u16..=255 {
            replace_both(
                &mut candidate,
                &mut reference,
                usize::from(state - 1),
                state,
            );
        }
        assert_eq!(candidate.representation(), RepresentationKind::Local8Stable);
        assert_eq!(candidate.live_palette_entries(), Some(256));

        replace_both(&mut candidate, &mut reference, 0, 0);
        assert_eq!(candidate.live_palette_entries(), Some(255));
        replace_both(&mut candidate, &mut reference, 255, 256);
        assert_eq!(candidate.representation(), RepresentationKind::Local8Stable);
        assert_eq!(candidate.live_palette_entries(), Some(256));
        assert!(!candidate.maybe_contains(|state| state == 1));

        replace_both(&mut candidate, &mut reference, 256, 257);
        assert_eq!(candidate.representation(), RepresentationKind::DirectN);
        assert_eq!(candidate.live_palette_entries(), None);
        assert_matches_reference(&candidate, &reference);
    }

    #[test]
    fn local8_full_palette_reuses_last_use_slot_without_promotion() {
        let mut candidate = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        let mut reference = DirectBlockSection::filled(0_u16, &SyntheticFacts);

        for state in 1_u16..=255 {
            replace_both(
                &mut candidate,
                &mut reference,
                usize::from(state - 1),
                state,
            );
        }
        assert_eq!(candidate.live_palette_entries(), Some(256));

        replace_both(&mut candidate, &mut reference, 0, 256);
        assert_eq!(candidate.representation(), RepresentationKind::Local8Stable);
        assert_eq!(candidate.live_palette_entries(), Some(256));
        assert_matches_reference(&candidate, &reference);
    }

    #[test]
    fn same_state_replacement_is_noop_at_palette_boundaries() {
        let mut candidate = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        for state in 1_u16..=15 {
            candidate.replace(pos(usize::from(state - 1)), state, &SyntheticFacts);
        }
        let summary = candidate.summary();
        let bytes = candidate.backing_bytes();
        assert_eq!(candidate.replace(pos(14), 15, &SyntheticFacts), 15);
        assert_eq!(candidate.summary(), summary);
        assert_eq!(candidate.backing_bytes(), bytes);
        assert_eq!(candidate.representation(), RepresentationKind::Local4Stable);
    }

    #[test]
    fn clone_has_independent_backing() {
        let mut original = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        original.replace(pos(31), 9, &SyntheticFacts);
        let mut cloned = original.clone();

        cloned.replace(pos(31), 12, &SyntheticFacts);
        assert_eq!(original.get(pos(31)), 9);
        assert_eq!(cloned.get(pos(31)), 12);
    }

    #[test]
    fn direct_candidate_has_exact_owned_memory_accounting() {
        let direct = DirectNBlockSection::filled(0_u16, &SyntheticFacts);
        let backing = BLOCK_SECTION_CELLS * mem::size_of::<u16>();
        assert_eq!(direct.backing_bytes(), backing);
        assert_eq!(
            direct.owned_bytes(),
            mem::size_of::<DirectNBlockSection<u16>>() + backing
        );

        let mut adaptive = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        assert_eq!(adaptive.backing_bytes(), 0);
        adaptive.replace(pos(0), 1, &SyntheticFacts);
        assert_eq!(adaptive.representation(), RepresentationKind::Local4Stable);
        assert_eq!(
            adaptive.backing_bytes(),
            2048 + LOCAL4_CAPACITY * mem::size_of::<PaletteSlot<u16>>()
        );
    }

    #[test]
    fn direct_candidate_matches_independent_reference_on_long_trace() {
        let mut candidate = DirectNBlockSection::filled(0_u16, &SyntheticFacts);
        let mut reference = DirectBlockSection::filled(0_u16, &SyntheticFacts);
        let mut rng = 0xA24B_AED4_963E_E407_u64;

        for step in 0..100_000_u32 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let index = usize::try_from(rng & 4095).expect("masked section index");
            let state = u16::try_from((rng >> 16) % 384).expect("bounded synthetic state");
            replace_both(&mut candidate, &mut reference, index, state);

            if step.is_multiple_of(4096) {
                assert_matches_reference(&candidate, &reference);
            }
        }
        assert_matches_reference(&candidate, &reference);
    }

    #[test]
    fn adaptive_candidate_matches_independent_reference_on_long_churn_trace() {
        let mut candidate = AdaptiveBlockSection::filled(0_u16, &SyntheticFacts);
        let mut reference = DirectBlockSection::filled(0_u16, &SyntheticFacts);
        let mut rng = 0xD1B5_4A32_D192_ED03_u64;

        for step in 0..100_000_u32 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let index = usize::try_from(rng & 4095).expect("masked section index");
            let state = u16::try_from((rng >> 20) % 320).expect("bounded synthetic state");
            replace_both(&mut candidate, &mut reference, index, state);

            if step.is_multiple_of(2048) {
                assert_matches_reference(&candidate, &reference);
                for needle in [0_u16, 1, 15, 16, 255, 256, 319] {
                    let exact =
                        (0..BLOCK_SECTION_CELLS).any(|cell| reference.get(pos(cell)) == needle);
                    if !candidate.maybe_contains(|state| state == needle) {
                        assert!(!exact, "false negative for state {needle}");
                    }
                }
            }
        }
        assert_matches_reference(&candidate, &reference);
    }
}
