//! Independent policy candidates for the M0.3B section-representation laboratory.

use std::mem;

use crucible_world_contract::{
    BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionBlockPos, SectionSummary,
};

use super::{LiveHeader, Local8, PaletteSlot, SECTION_CELL_COUNT_U16};

const MAX_PACKED_BITS: u8 = 8;

impl<S: Copy + Eq> Local8<S> {
    fn from_uniform_fast(previous: S, changed_cell: usize, state: S) -> Self {
        debug_assert!(previous != state);
        let mut palette = Vec::with_capacity(256);
        palette.push(PaletteSlot::new(previous, SECTION_CELL_COUNT_U16 - 1));
        palette.push(PaletteSlot::new(state, 1));
        let mut indices = Box::new([0; BLOCK_SECTION_CELLS]);
        indices[changed_cell] = 1;
        Self { palette, indices }
    }
}

/// Physical representation used by [`FastLocalBlockSection`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FastLocalRepresentation {
    /// Homogeneous section with no heap cell backing.
    Uniform,
    /// Byte-indexed stable local palette.
    Local8Stable,
    /// Direct state IDs for every cell.
    DirectN,
}

#[derive(Clone, Debug)]
enum FastStorage<S: Copy + Eq> {
    Uniform(S),
    Local8(Local8<S>),
    Direct(Box<[S; BLOCK_SECTION_CELLS]>),
}

impl<S: Copy + Eq> FastStorage<S> {
    #[inline]
    fn get(&self, cell: usize) -> S {
        match self {
            Self::Uniform(state) => *state,
            Self::Local8(storage) => storage.get(cell),
            Self::Direct(storage) => storage[cell],
        }
    }

    const fn representation(&self) -> FastLocalRepresentation {
        match self {
            Self::Uniform(_) => FastLocalRepresentation::Uniform,
            Self::Local8(_) => FastLocalRepresentation::Local8Stable,
            Self::Direct(_) => FastLocalRepresentation::DirectN,
        }
    }

    fn backing_bytes(&self) -> usize {
        match self {
            Self::Uniform(_) => 0,
            Self::Local8(storage) => storage.heap_bytes(),
            Self::Direct(storage) => mem::size_of_val(storage.as_ref()),
        }
    }
}

/// Fast-local policy candidate: `Uniform -> Local8Stable -> DirectN`.
///
/// This deliberately spends more memory at low cardinality than the Local4 policy so #19 can
/// measure whether avoiding the Local4 stage is worth the mutation/read cost difference.
#[derive(Clone, Debug)]
pub struct FastLocalBlockSection<S: Copy + Eq> {
    storage: FastStorage<S>,
    header: LiveHeader,
}

impl<S: Copy + Eq> FastLocalBlockSection<S> {
    /// Creates a homogeneous, allocation-free section.
    #[must_use]
    pub fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        Self {
            storage: FastStorage::Uniform(state),
            header: LiveHeader::filled(state, facts),
        }
    }

    /// Returns the current physical form for qualification telemetry.
    #[must_use]
    pub const fn representation(&self) -> FastLocalRepresentation {
        self.storage.representation()
    }

    /// Returns heap bytes owned by the backing, excluding allocator metadata.
    #[must_use]
    pub fn backing_bytes(&self) -> usize {
        self.storage.backing_bytes()
    }

    /// Returns deterministic object-plus-backing bytes, excluding allocator metadata.
    #[must_use]
    pub fn owned_bytes(&self) -> usize {
        mem::size_of::<Self>() + self.backing_bytes()
    }

    /// Returns simultaneously-live local entries while palette-backed.
    #[must_use]
    pub fn live_palette_entries(&self) -> Option<usize> {
        match &self.storage {
            FastStorage::Uniform(_) => Some(1),
            FastStorage::Local8(storage) => Some(storage.live_entries()),
            FastStorage::Direct(_) => None,
        }
    }

    fn replace_storage(&mut self, cell: usize, state: S) {
        match &mut self.storage {
            FastStorage::Uniform(previous) => {
                let previous = *previous;
                debug_assert!(previous != state);
                self.storage =
                    FastStorage::Local8(Local8::from_uniform_fast(previous, cell, state));
            }
            FastStorage::Local8(storage) => {
                if storage.try_replace(cell, state) {
                    return;
                }
                let old = mem::replace(&mut self.storage, FastStorage::Uniform(state));
                let FastStorage::Local8(storage) = old else {
                    unreachable!("matched Local8 before replacement")
                };
                let mut direct = storage.into_direct();
                direct[cell] = state;
                self.storage = FastStorage::Direct(direct);
            }
            FastStorage::Direct(storage) => storage[cell] = state,
        }
    }
}

impl<S: Copy + Eq> BlockSection<S> for FastLocalBlockSection<S> {
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
            FastStorage::Uniform(state) => predicate(*state),
            FastStorage::Local8(storage) => storage
                .palette
                .iter()
                .filter(|slot| slot.is_live())
                .map(|slot| slot.state)
                .any(predicate),
            FastStorage::Direct(storage) => storage.iter().copied().any(predicate),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PackedReplace {
    Done,
    NeedsWider,
    NeedsDirect,
}

#[derive(Clone, Debug)]
struct PackedLocal<S: Copy + Eq> {
    palette: Vec<PaletteSlot<S>>,
    words: Box<[u64]>,
    bits: u8,
}

impl<S: Copy + Eq> PackedLocal<S> {
    fn from_uniform(previous: S, changed_cell: usize, state: S) -> Self {
        debug_assert!(previous != state);
        let mut palette = Vec::with_capacity(2);
        palette.extend([
            PaletteSlot::new(previous, SECTION_CELL_COUNT_U16 - 1),
            PaletteSlot::new(state, 1),
        ]);
        let mut result = Self {
            palette,
            words: vec![0; Self::word_count(1)].into_boxed_slice(),
            bits: 1,
        };
        result.set_local_index(changed_cell, 1);
        result
    }

    const fn local_capacity(bits: u8) -> usize {
        1_usize << bits
    }

    const fn word_count(bits: u8) -> usize {
        (BLOCK_SECTION_CELLS * bits as usize).div_ceil(64)
    }

    const fn value_mask(bits: u8) -> u64 {
        (1_u64 << bits) - 1
    }

    #[inline]
    fn local_index(&self, cell: usize) -> u8 {
        let bits = usize::from(self.bits);
        let bit_index = cell * bits;
        let word_index = bit_index >> 6;
        let shift = bit_index & 63;
        let mask = Self::value_mask(self.bits);
        let value = if shift + bits <= 64 {
            (self.words[word_index] >> shift) & mask
        } else {
            let low_bits = 64 - shift;
            let high_bits = bits - low_bits;
            let low = self.words[word_index] >> shift;
            let high_mask = (1_u64 << high_bits) - 1;
            let high = self.words[word_index + 1] & high_mask;
            low | (high << low_bits)
        };
        u8::try_from(value).expect("packed local index is at most eight bits")
    }

    #[inline]
    fn set_local_index(&mut self, cell: usize, local: u8) {
        let bits = usize::from(self.bits);
        debug_assert!(usize::from(local) < Self::local_capacity(self.bits));
        let bit_index = cell * bits;
        let word_index = bit_index >> 6;
        let shift = bit_index & 63;
        let mask = Self::value_mask(self.bits);
        let value = u64::from(local) & mask;

        if shift + bits <= 64 {
            let shifted_mask = mask << shift;
            self.words[word_index] = (self.words[word_index] & !shifted_mask) | (value << shift);
        } else {
            let low_bits = 64 - shift;
            let high_bits = bits - low_bits;
            let low_mask = (1_u64 << low_bits) - 1;
            let shifted_low_mask = low_mask << shift;
            self.words[word_index] =
                (self.words[word_index] & !shifted_low_mask) | ((value & low_mask) << shift);

            let high_mask = (1_u64 << high_bits) - 1;
            self.words[word_index + 1] =
                (self.words[word_index + 1] & !high_mask) | ((value >> low_bits) & high_mask);
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
            .map(|index| u8::try_from(index).expect("packed palette never exceeds 256 entries"))
    }

    fn dead_palette_index(&self) -> Option<u8> {
        self.palette
            .iter()
            .position(|slot| !slot.is_live())
            .map(|index| u8::try_from(index).expect("packed palette never exceeds 256 entries"))
    }

    fn try_replace(&mut self, cell: usize, state: S) -> PackedReplace {
        let old_local = self.local_index(cell);
        let old_index = usize::from(old_local);
        debug_assert!(self.palette[old_index].uses != 0);
        if self.palette[old_index].state == state {
            return PackedReplace::Done;
        }

        if let Some(target_local) = self.live_palette_index(state) {
            let target_index = usize::from(target_local);
            self.palette[old_index].uses -= 1;
            self.palette[target_index].uses += 1;
            self.set_local_index(cell, target_local);
            return PackedReplace::Done;
        }

        if let Some(target_local) = self.dead_palette_index() {
            let target_index = usize::from(target_local);
            self.palette[old_index].uses -= 1;
            self.palette[target_index] = PaletteSlot::new(state, 1);
            self.set_local_index(cell, target_local);
            return PackedReplace::Done;
        }

        if self.palette.len() < Self::local_capacity(self.bits) {
            let target_local =
                u8::try_from(self.palette.len()).expect("packed palette never exceeds 256 entries");
            self.palette[old_index].uses -= 1;
            self.palette.push(PaletteSlot::new(state, 1));
            self.set_local_index(cell, target_local);
            return PackedReplace::Done;
        }

        if self.palette[old_index].uses == 1 {
            self.palette[old_index].state = state;
            return PackedReplace::Done;
        }

        if self.bits < MAX_PACKED_BITS {
            PackedReplace::NeedsWider
        } else {
            PackedReplace::NeedsDirect
        }
    }

    fn widen(&self) -> Self {
        debug_assert!(self.bits < MAX_PACKED_BITS);
        let new_bits = self.bits + 1;
        let mut palette = Vec::with_capacity(Self::local_capacity(new_bits));
        palette.extend(self.palette.iter().copied());
        let mut widened = Self {
            palette,
            words: vec![0; Self::word_count(new_bits)].into_boxed_slice(),
            bits: new_bits,
        };
        for cell in 0..BLOCK_SECTION_CELLS {
            widened.set_local_index(cell, self.local_index(cell));
        }
        widened
    }

    #[expect(
        clippy::unnecessary_box_returns,
        reason = "packed-to-direct promotion keeps the full direct section heap-resident"
    )]
    fn into_direct(self) -> Box<[S; BLOCK_SECTION_CELLS]> {
        let mut cells = Box::new([self.palette[0].state; BLOCK_SECTION_CELLS]);
        for (cell, target) in cells.iter_mut().enumerate() {
            *target = self.get(cell);
        }
        cells
    }

    fn live_entries(&self) -> usize {
        self.palette.iter().filter(|slot| slot.is_live()).count()
    }

    fn backing_bytes(&self) -> usize {
        self.words.len() * mem::size_of::<u64>()
            + self.palette.capacity() * mem::size_of::<PaletteSlot<S>>()
    }
}

/// Physical representation used by [`PackedLocalBlockSection`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackedLocalRepresentation {
    /// Homogeneous section with no heap cell backing.
    Uniform,
    /// Bit-packed local indices; payload is the current bits per cell.
    Packed(u8),
    /// Direct state IDs for every cell.
    DirectN,
}

#[derive(Clone, Debug)]
enum PackedStorage<S: Copy + Eq> {
    Uniform(S),
    Packed(PackedLocal<S>),
    Direct(Box<[S; BLOCK_SECTION_CELLS]>),
}

impl<S: Copy + Eq> PackedStorage<S> {
    #[inline]
    fn get(&self, cell: usize) -> S {
        match self {
            Self::Uniform(state) => *state,
            Self::Packed(storage) => storage.get(cell),
            Self::Direct(storage) => storage[cell],
        }
    }

    const fn representation(&self) -> PackedLocalRepresentation {
        match self {
            Self::Uniform(_) => PackedLocalRepresentation::Uniform,
            Self::Packed(storage) => PackedLocalRepresentation::Packed(storage.bits),
            Self::Direct(_) => PackedLocalRepresentation::DirectN,
        }
    }

    fn backing_bytes(&self) -> usize {
        match self {
            Self::Uniform(_) => 0,
            Self::Packed(storage) => storage.backing_bytes(),
            Self::Direct(storage) => mem::size_of_val(storage.as_ref()),
        }
    }
}

/// Memory-frontier candidate with the smallest local bit width that fits allocated stable slots.
///
/// Width grows monotonically from one to eight bits. Width changes are explicit O(4096) transition
/// events; ordinary reads and replacements neither allocate nor compact the palette. At eight bits,
/// a genuine 257th simultaneously-live state promotes to `DirectN`.
#[derive(Clone, Debug)]
pub struct PackedLocalBlockSection<S: Copy + Eq> {
    storage: PackedStorage<S>,
    header: LiveHeader,
}

impl<S: Copy + Eq> PackedLocalBlockSection<S> {
    /// Creates a homogeneous, allocation-free section.
    #[must_use]
    pub fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        Self {
            storage: PackedStorage::Uniform(state),
            header: LiveHeader::filled(state, facts),
        }
    }

    /// Returns the current physical form and packed bit width when applicable.
    #[must_use]
    pub const fn representation(&self) -> PackedLocalRepresentation {
        self.storage.representation()
    }

    /// Returns heap bytes owned by the backing, excluding allocator metadata.
    #[must_use]
    pub fn backing_bytes(&self) -> usize {
        self.storage.backing_bytes()
    }

    /// Returns deterministic object-plus-backing bytes, excluding allocator metadata.
    #[must_use]
    pub fn owned_bytes(&self) -> usize {
        mem::size_of::<Self>() + self.backing_bytes()
    }

    /// Returns simultaneously-live local entries while palette-backed.
    #[must_use]
    pub fn live_palette_entries(&self) -> Option<usize> {
        match &self.storage {
            PackedStorage::Uniform(_) => Some(1),
            PackedStorage::Packed(storage) => Some(storage.live_entries()),
            PackedStorage::Direct(_) => None,
        }
    }

    fn replace_storage(&mut self, cell: usize, state: S) {
        match &mut self.storage {
            PackedStorage::Uniform(previous) => {
                let previous = *previous;
                debug_assert!(previous != state);
                self.storage =
                    PackedStorage::Packed(PackedLocal::from_uniform(previous, cell, state));
            }
            PackedStorage::Packed(storage) => match storage.try_replace(cell, state) {
                PackedReplace::Done => {}
                PackedReplace::NeedsWider => {
                    let old = mem::replace(&mut self.storage, PackedStorage::Uniform(state));
                    let PackedStorage::Packed(storage) = old else {
                        unreachable!("matched packed storage before replacement")
                    };
                    let mut widened = storage.widen();
                    debug_assert_eq!(widened.try_replace(cell, state), PackedReplace::Done);
                    self.storage = PackedStorage::Packed(widened);
                }
                PackedReplace::NeedsDirect => {
                    let old = mem::replace(&mut self.storage, PackedStorage::Uniform(state));
                    let PackedStorage::Packed(storage) = old else {
                        unreachable!("matched packed storage before replacement")
                    };
                    let mut direct = storage.into_direct();
                    direct[cell] = state;
                    self.storage = PackedStorage::Direct(direct);
                }
            },
            PackedStorage::Direct(storage) => storage[cell] = state,
        }
    }
}

impl<S: Copy + Eq> BlockSection<S> for PackedLocalBlockSection<S> {
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
            PackedStorage::Uniform(state) => predicate(*state),
            PackedStorage::Packed(storage) => storage
                .palette
                .iter()
                .filter(|slot| slot.is_live())
                .map(|slot| slot.state)
                .any(predicate),
            PackedStorage::Direct(storage) => storage.iter().copied().any(predicate),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::mem;

    use crucible_generated::{AIR, BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts};
    use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection, SectionBlockPos};
    use crucible_world_reference::DirectBlockSection;

    use crate::{AdaptiveBlockSection, DirectNBlockSection};

    use super::{
        FastLocalBlockSection, FastLocalRepresentation, PackedLocal, PackedLocalBlockSection,
        PackedLocalRepresentation, PaletteSlot,
    };

    fn pos(index: usize) -> SectionBlockPos {
        let x = u8::try_from(index & 15).expect("bounded x");
        let z = u8::try_from((index >> 4) & 15).expect("bounded z");
        let y = u8::try_from((index >> 8) & 15).expect("bounded y");
        SectionBlockPos::new(x, y, z).expect("decoded section coordinate")
    }

    fn state(raw: u32) -> BlockStateId {
        BlockStateId::new(raw).expect("test state is inside generated target universe")
    }

    fn assert_target_equivalent<C: BlockSection<BlockStateId>>(
        candidate: &C,
        reference: &DirectBlockSection<BlockStateId>,
    ) {
        for cell in 0..BLOCK_SECTION_CELLS {
            assert_eq!(candidate.get(pos(cell)), reference.get(pos(cell)));
        }
        assert_eq!(candidate.summary(), reference.summary());
        assert_eq!(
            reference.summary(),
            reference.recompute_summary(&GeneratedStateFacts)
        );
    }

    fn run_target_trace<C: BlockSection<BlockStateId>>(
        mut candidate: C,
        iterations: u32,
        seed: u64,
    ) {
        let mut reference = DirectBlockSection::filled(AIR, &GeneratedStateFacts);
        let target_count = u64::try_from(BLOCK_STATE_COUNT).expect("target state count fits u64");
        let mut rng = seed;
        for step in 0..iterations {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let cell = usize::try_from(rng & 4095).expect("masked section index");
            let raw = u32::try_from((rng >> 17) % target_count)
                .expect("modulo target state count fits u32");
            let next = state(raw);
            let previous_candidate = candidate.replace(pos(cell), next, &GeneratedStateFacts);
            let previous_reference = reference.replace(pos(cell), next, &GeneratedStateFacts);
            assert_eq!(previous_candidate, previous_reference);
            assert_eq!(candidate.summary(), reference.summary());

            if step.is_multiple_of(2048) {
                assert_target_equivalent(&candidate, &reference);
                for needle in [AIR, state(1), state(100), state(1000)] {
                    let exact =
                        (0..BLOCK_SECTION_CELLS).any(|index| reference.get(pos(index)) == needle);
                    if !candidate.maybe_contains(|value| value == needle) {
                        assert!(!exact, "target maybe_contains false negative");
                    }
                }
            }
        }
        assert_target_equivalent(&candidate, &reference);
    }

    #[test]
    fn direct_n_target_trace_matches_reference() {
        run_target_trace(
            DirectNBlockSection::filled(AIR, &GeneratedStateFacts),
            100_000,
            0x7F0D_A61C_4239_B5E1,
        );
    }

    #[test]
    fn adaptive_target_trace_matches_reference() {
        run_target_trace(
            AdaptiveBlockSection::filled(AIR, &GeneratedStateFacts),
            100_000,
            0x3C92_5E0B_A147_D8F6,
        );
    }

    #[test]
    fn fast_local_starts_uniform_then_uses_local8() {
        let mut section = FastLocalBlockSection::filled(AIR, &GeneratedStateFacts);
        assert_eq!(section.representation(), FastLocalRepresentation::Uniform);
        assert_eq!(section.backing_bytes(), 0);
        section.replace(pos(0), state(1), &GeneratedStateFacts);
        assert_eq!(
            section.representation(),
            FastLocalRepresentation::Local8Stable
        );
        assert_eq!(section.live_palette_entries(), Some(2));
        assert_eq!(
            section.backing_bytes(),
            BLOCK_SECTION_CELLS + 256 * mem::size_of::<PaletteSlot<BlockStateId>>()
        );
    }

    #[test]
    fn fast_local_reuses_last_use_at_256_then_promotes_on_257() {
        let mut section = FastLocalBlockSection::filled(AIR, &GeneratedStateFacts);
        for raw in 1_u32..=255 {
            section.replace(
                pos(usize::try_from(raw - 1).expect("small cell index")),
                state(raw),
                &GeneratedStateFacts,
            );
        }
        assert_eq!(section.live_palette_entries(), Some(256));
        assert_eq!(
            section.representation(),
            FastLocalRepresentation::Local8Stable
        );

        section.replace(pos(0), state(256), &GeneratedStateFacts);
        assert_eq!(section.live_palette_entries(), Some(256));
        assert_eq!(
            section.representation(),
            FastLocalRepresentation::Local8Stable
        );

        section.replace(pos(255), state(257), &GeneratedStateFacts);
        assert_eq!(section.representation(), FastLocalRepresentation::DirectN);
        assert_eq!(section.live_palette_entries(), None);
    }

    #[test]
    fn fast_local_target_trace_matches_reference() {
        run_target_trace(
            FastLocalBlockSection::filled(AIR, &GeneratedStateFacts),
            100_000,
            0x5A17_8D39_11C4_E2B7,
        );
    }

    #[test]
    fn packed_width_tracks_local_capacity() {
        let mut section = PackedLocalBlockSection::filled(AIR, &GeneratedStateFacts);
        assert_eq!(section.representation(), PackedLocalRepresentation::Uniform);
        section.replace(pos(0), state(1), &GeneratedStateFacts);
        assert_eq!(
            section.representation(),
            PackedLocalRepresentation::Packed(1)
        );
        let one_bit_bytes = 512 + 2 * mem::size_of::<PaletteSlot<BlockStateId>>();
        assert_eq!(section.backing_bytes(), one_bit_bytes);

        section.replace(pos(1), state(2), &GeneratedStateFacts);
        assert_eq!(
            section.representation(),
            PackedLocalRepresentation::Packed(2)
        );
        let two_bit_bytes = 1024 + 4 * mem::size_of::<PaletteSlot<BlockStateId>>();
        assert_eq!(section.backing_bytes(), two_bit_bytes);
    }

    #[test]
    fn packed_reuses_last_use_at_256_then_promotes_on_257() {
        let mut section = PackedLocalBlockSection::filled(AIR, &GeneratedStateFacts);
        for raw in 1_u32..=255 {
            section.replace(
                pos(usize::try_from(raw - 1).expect("small cell index")),
                state(raw),
                &GeneratedStateFacts,
            );
        }
        assert_eq!(
            section.representation(),
            PackedLocalRepresentation::Packed(8)
        );
        assert_eq!(section.live_palette_entries(), Some(256));

        section.replace(pos(0), state(256), &GeneratedStateFacts);
        assert_eq!(
            section.representation(),
            PackedLocalRepresentation::Packed(8)
        );
        assert_eq!(section.live_palette_entries(), Some(256));

        section.replace(pos(255), state(257), &GeneratedStateFacts);
        assert_eq!(section.representation(), PackedLocalRepresentation::DirectN);
        assert_eq!(section.live_palette_entries(), None);
    }

    #[test]
    fn packed_bit_access_handles_every_width_and_word_boundary() {
        for bits in 1_u8..=8 {
            let capacity = PackedLocal::<u16>::local_capacity(bits);
            let mut palette = Vec::with_capacity(capacity);
            for local in 0..capacity {
                palette.push(PaletteSlot::new(
                    u16::try_from(local).expect("packed local ID fits u16"),
                    1,
                ));
            }
            let mut packed = PackedLocal {
                palette,
                words: vec![0; PackedLocal::<u16>::word_count(bits)].into_boxed_slice(),
                bits,
            };
            for cell in 0..BLOCK_SECTION_CELLS {
                let local = u8::try_from(cell % capacity).expect("capacity is at most 256");
                packed.set_local_index(cell, local);
            }
            for cell in 0..BLOCK_SECTION_CELLS {
                let expected = u8::try_from(cell % capacity).expect("capacity is at most 256");
                assert_eq!(
                    packed.local_index(cell),
                    expected,
                    "bits={bits} cell={cell}"
                );
            }
        }
    }

    #[test]
    fn packed_target_trace_matches_reference() {
        run_target_trace(
            PackedLocalBlockSection::filled(AIR, &GeneratedStateFacts),
            100_000,
            0xB6E4_7A23_D908_51CF,
        );
    }
}
