//! Semantic contracts for Crucible's world substrate.
//!
//! This crate describes obligations, not storage mechanisms. Implementations may use radically
//! different layouts so long as these semantics and their qualified external projections hold.

#![forbid(unsafe_code)]

/// Number of block cells on one axis of a chunk section.
pub const BLOCK_SECTION_AXIS: usize = 16;
/// Number of block cells in a chunk section.
pub const BLOCK_SECTION_CELLS: usize = BLOCK_SECTION_AXIS * BLOCK_SECTION_AXIS * BLOCK_SECTION_AXIS;
/// Number of biome cells on one axis of a chunk section.
pub const BIOME_SECTION_AXIS: usize = 4;
/// Number of biome cells in a chunk section.
pub const BIOME_SECTION_CELLS: usize = BIOME_SECTION_AXIS * BIOME_SECTION_AXIS * BIOME_SECTION_AXIS;

/// A validated block coordinate local to one 16×16×16 section.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SectionBlockPos {
    x: u8,
    y: u8,
    z: u8,
}

impl SectionBlockPos {
    /// Creates a local section coordinate when all components are in `0..16`.
    #[must_use]
    pub const fn new(x: u8, y: u8, z: u8) -> Option<Self> {
        if x < 16 && y < 16 && z < 16 {
            Some(Self { x, y, z })
        } else {
            None
        }
    }

    /// Returns the x component.
    #[must_use]
    pub const fn x(self) -> u8 {
        self.x
    }

    /// Returns the y component.
    #[must_use]
    pub const fn y(self) -> u8 {
        self.y
    }

    /// Returns the z component.
    #[must_use]
    pub const fn z(self) -> u8 {
        self.z
    }

    /// Returns Mojang's observable section-cell linearization: `(y << 8) | (z << 4) | x`.
    #[must_use]
    pub const fn index(self) -> usize {
        ((self.y as usize) << 8) | ((self.z as usize) << 4) | self.x as usize
    }
}

/// A validated biome coordinate local to one 4×4×4 section biome lattice.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SectionBiomePos {
    x: u8,
    y: u8,
    z: u8,
}

impl SectionBiomePos {
    /// Creates a local biome coordinate when all components are in `0..4`.
    #[must_use]
    pub const fn new(x: u8, y: u8, z: u8) -> Option<Self> {
        if x < 4 && y < 4 && z < 4 {
            Some(Self { x, y, z })
        } else {
            None
        }
    }

    /// Returns Mojang's observable biome-cell linearization: `(y << 4) | (z << 2) | x`.
    #[must_use]
    pub const fn index(self) -> usize {
        ((self.y as usize) << 4) | ((self.z as usize) << 2) | self.x as usize
    }
}

/// Precomputed semantic facts needed by section summary maintenance.
///
/// In the target-version generated database these facts are intended to be a direct lookup by
/// block-state identity. The abstraction exists so the section contract does not own registry
/// layout or force repeated object-oriented queries in the hot mutation path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SectionStateFacts(u8);

impl SectionStateFacts {
    const NON_AIR: u8 = 1 << 0;
    const COUNTED_FLUID: u8 = 1 << 1;
    const RANDOM_BLOCK: u8 = 1 << 2;
    const RANDOM_FLUID: u8 = 1 << 3;

    /// Constructs the exact section facts for one state.
    ///
    /// `counted_fluid` follows the target server's section-count semantics: a fluid is counted only
    /// on a non-air block state. Random block/fluid contributions are canonicalized to the same
    /// counted domains.
    #[must_use]
    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "the four booleans are independent target semantic facts packed into one byte"
    )]
    pub const fn new(
        non_air: bool,
        counted_fluid: bool,
        random_block: bool,
        random_fluid: bool,
    ) -> Self {
        let mut bits = 0;
        if non_air {
            bits |= Self::NON_AIR;
            if counted_fluid {
                bits |= Self::COUNTED_FLUID;
            }
            if random_block {
                bits |= Self::RANDOM_BLOCK;
            }
            if counted_fluid && random_fluid {
                bits |= Self::RANDOM_FLUID;
            }
        }
        Self(bits)
    }

    /// Whether this state contributes to the exact non-air count.
    #[must_use]
    pub const fn non_air(self) -> bool {
        self.0 & Self::NON_AIR != 0
    }

    /// Whether this state contributes to the exact section fluid count.
    #[must_use]
    pub const fn counted_fluid(self) -> bool {
        self.0 & Self::COUNTED_FLUID != 0
    }

    /// Whether this state is itself random-block-ticking.
    #[must_use]
    pub const fn random_block(self) -> bool {
        self.0 & Self::RANDOM_BLOCK != 0
    }

    /// Whether this state's counted fluid is random-fluid-ticking.
    #[must_use]
    pub const fn random_fluid(self) -> bool {
        self.0 & Self::RANDOM_FLUID != 0
    }
}

/// Exact semantic section summaries required by the simulation contract.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SectionSummary {
    /// Number of non-air block states in the section.
    pub non_air_count: u16,
    /// Number of counted non-empty fluid states in the section.
    pub fluid_count: u16,
    /// Whether at least one block state is random-block-ticking.
    pub random_block_present: bool,
    /// Whether at least one counted fluid state is random-fluid-ticking.
    pub random_fluid_present: bool,
}

impl SectionSummary {
    /// Whether the section contains only air according to target section semantics.
    #[must_use]
    pub const fn has_only_air(self) -> bool {
        self.non_air_count == 0
    }

    /// Whether the section contains at least one counted fluid state.
    #[must_use]
    pub const fn has_fluid(self) -> bool {
        self.fluid_count != 0
    }

    /// Whether random block or random fluid ticking is possible in this section.
    #[must_use]
    pub const fn is_randomly_ticking(self) -> bool {
        self.random_block_present || self.random_fluid_present
    }
}

/// Supplies pure target-version facts for a semantic block-state identity.
///
/// Production implementations should normally use a generated direct lookup. The trait is generic
/// and need not imply virtual dispatch: callers are expected to monomorphize HOT paths.
pub trait BlockStateFacts<S: Copy> {
    /// Returns the exact target-version section facts for `state`.
    fn facts(&self, state: S) -> SectionStateFacts;
}

/// Semantic block-section behavior shared by reference and production implementations.
///
/// This trait intentionally avoids object-safety requirements so production code can use static
/// dispatch. `maybe_contains` is a conservative query: `false` must prove absence; `true` may be a
/// false positive.
pub trait BlockSection<S: Copy + Eq> {
    /// Returns the exact semantic state at `pos`.
    fn get(&self, pos: SectionBlockPos) -> S;

    /// Replaces the state at `pos`, updates exact summaries, and returns the previous state.
    fn replace<F: BlockStateFacts<S>>(&mut self, pos: SectionBlockPos, state: S, facts: &F) -> S;

    /// Returns exact section summaries.
    fn summary(&self) -> SectionSummary;

    /// Returns a conservative answer to whether any state may satisfy `predicate`.
    ///
    /// A `false` result is a semantic guarantee of absence. A `true` result is only a hint that the
    /// caller may need an exact scan or operation.
    fn maybe_contains<P: FnMut(S) -> bool>(&self, predicate: P) -> bool;
}

/// Semantic biome-section behavior shared by reference and future production implementations.
///
/// The contract deliberately freezes only the source-backed 4×4×4 semantic lattice and its local
/// coordinate law. Palette shape, registry resolution, packed storage and protocol encoding remain
/// mechanism/boundary concerns. Implementations are expected to use static dispatch in HOT paths.
pub trait BiomeSection<B: Copy + Eq> {
    /// Returns the exact semantic biome identity at one local 4×4×4 lattice coordinate.
    fn get(&self, pos: SectionBiomePos) -> B;

    /// Replaces one semantic biome identity and returns the previous identity.
    fn replace(&mut self, pos: SectionBiomePos, biome: B) -> B;
}

#[cfg(test)]
mod tests {
    use super::{BIOME_SECTION_CELLS, BLOCK_SECTION_CELLS, SectionBiomePos, SectionBlockPos};

    #[test]
    fn block_linearization_is_bijective_over_section() {
        let mut seen = [false; BLOCK_SECTION_CELLS];
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let pos = SectionBlockPos::new(x, y, z).expect("bounded coordinate");
                    assert!(!seen[pos.index()]);
                    seen[pos.index()] = true;
                }
            }
        }
        assert!(seen.into_iter().all(|present| present));
    }

    #[test]
    fn biome_linearization_is_bijective_over_lattice() {
        let mut seen = [false; BIOME_SECTION_CELLS];
        for y in 0..4 {
            for z in 0..4 {
                for x in 0..4 {
                    let pos = SectionBiomePos::new(x, y, z).expect("bounded coordinate");
                    assert!(!seen[pos.index()]);
                    seen[pos.index()] = true;
                }
            }
        }
        assert!(seen.into_iter().all(|present| present));
    }

    #[test]
    fn invalid_coordinates_are_rejected() {
        assert!(SectionBlockPos::new(16, 0, 0).is_none());
        assert!(SectionBiomePos::new(0, 4, 0).is_none());
    }
}
