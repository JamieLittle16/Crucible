//! Component vocabulary used by the composition layer.
//!
//! This crate describes component contracts; it does not contain a runtime service locator.

#![forbid(unsafe_code)]

/// Relative execution sensitivity of a component interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CostClass {
    Hot,
    Warm,
    Cold,
}

/// When an implementation may be replaced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwapClass {
    Static,
    Restart,
    Quiescent,
    LiveReversible,
}

/// Trust granted to executable code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustClass {
    DataOnly,
    Sandboxed,
    TrustedNative,
    EngineNative,
}

/// Semantic fidelity policy attached to a component/composition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fidelity {
    Strict,
    Relaxed,
}

/// Stable textual identity for a versioned component capability.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CapabilityId(pub &'static str);

impl CapabilityId {
    #[must_use]
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }
}

#[cfg(test)]
mod tests {
    use super::{CapabilityId, CostClass, Fidelity, SwapClass, TrustClass};

    #[test]
    fn component_vocabulary_is_explicit() {
        let section_store = CapabilityId::new("world.section-store/1");
        assert_eq!(section_store.0, "world.section-store/1");
        assert_eq!(CostClass::Hot, CostClass::Hot);
        assert_eq!(SwapClass::Static, SwapClass::Static);
        assert_eq!(TrustClass::EngineNative, TrustClass::EngineNative);
        assert_eq!(Fidelity::Strict, Fidelity::Strict);
    }
}
