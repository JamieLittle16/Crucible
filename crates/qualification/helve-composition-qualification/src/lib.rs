//! Qualification helpers for Crucible's generated static composition boundary.
//!
//! This crate is evidence infrastructure. It exists to make a regression from concrete static
//! wiring to a runtime-dispatched HOT boundary immediately visible.

#![forbid(unsafe_code)]

use core::any::TypeId;
use core::mem::{align_of, size_of};

use crucible_composition::SectionStore;
use crucible_generated::BlockStateId;
use crucible_world_contract::{BlockSection, SectionBlockPos};
use crucible_world_reference::DirectBlockSection;

/// Concrete section type selected by the generated reference composition.
pub type GeneratedSection = SectionStore<BlockStateId>;
/// The exact direct hand-wired reference provider selected by that composition.
pub type HandWiredSection = DirectBlockSection<BlockStateId>;

/// Reads one cell through the generated composition type name.
#[inline]
#[must_use]
pub fn generated_get(section: &GeneratedSection, pos: SectionBlockPos) -> BlockStateId {
    section.get(pos)
}

/// Reads one cell through the direct hand-wired provider type name.
#[inline]
#[must_use]
pub fn hand_wired_get(section: &HandWiredSection, pos: SectionBlockPos) -> BlockStateId {
    section.get(pos)
}

/// Whether the generated and hand-wired provider names resolve to the exact same Rust type.
#[must_use]
pub fn exact_type_identity() -> bool {
    TypeId::of::<GeneratedSection>() == TypeId::of::<HandWiredSection>()
        && size_of::<GeneratedSection>() == size_of::<HandWiredSection>()
        && align_of::<GeneratedSection>() == align_of::<HandWiredSection>()
}

#[cfg(test)]
mod tests {
    use core::any::TypeId;
    use core::mem::{align_of, size_of};

    use crucible_generated::{AIR, GeneratedStateFacts};
    use crucible_world_reference::DirectBlockSection;

    use super::{GeneratedSection, HandWiredSection, exact_type_identity};

    fn accepts_hand_wired(_: &HandWiredSection) {}

    #[test]
    fn generated_provider_is_the_exact_hand_wired_concrete_type() {
        assert!(exact_type_identity());
        assert_eq!(
            TypeId::of::<GeneratedSection>(),
            TypeId::of::<HandWiredSection>()
        );
        assert_eq!(size_of::<GeneratedSection>(), size_of::<HandWiredSection>());
        assert_eq!(
            align_of::<GeneratedSection>(),
            align_of::<HandWiredSection>()
        );

        let generated: GeneratedSection = DirectBlockSection::filled(AIR, &GeneratedStateFacts);
        accepts_hand_wired(&generated);
        let _: &HandWiredSection = &generated;
    }
}
