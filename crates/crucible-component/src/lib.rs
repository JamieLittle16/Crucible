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

/// Stable identity for one exact version of a component capability.
///
/// Human-readable manifests serialize this as `<name>/<version>`, for example
/// `world.section-store/1`. Keeping the version structurally separate in Rust prevents callers
/// from treating an unversioned capability name as a complete compatibility identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CapabilityId {
    name: &'static str,
    version: u32,
}

impl CapabilityId {
    /// Creates a versioned capability identity.
    ///
    /// # Panics
    ///
    /// Panics in const-evaluation or at runtime when `name` is empty or `version` is zero. The
    /// cold-path composition resolver applies the stricter manifest grammar before generated
    /// wiring reaches this crate.
    #[must_use]
    pub const fn new(name: &'static str, version: u32) -> Self {
        assert!(!name.is_empty(), "capability name must not be empty");
        assert!(version != 0, "capability version must be non-zero");
        Self { name, version }
    }

    /// Returns the unversioned semantic capability name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Returns the exact compatibility version.
    #[must_use]
    pub const fn version(self) -> u32 {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use super::{CapabilityId, CostClass, Fidelity, SwapClass, TrustClass};

    #[test]
    fn component_vocabulary_distinguishes_policy_classes() {
        let section_store = CapabilityId::new("world.section-store", 1);
        assert_eq!(section_store.name(), "world.section-store");
        assert_eq!(section_store.version(), 1);
        assert_ne!(CostClass::Hot, CostClass::Cold);
        assert_ne!(SwapClass::Static, SwapClass::LiveReversible);
        assert_ne!(TrustClass::Sandboxed, TrustClass::EngineNative);
        assert_ne!(Fidelity::Strict, Fidelity::Relaxed);
    }

    #[test]
    #[should_panic(expected = "capability version must be non-zero")]
    fn zero_capability_version_is_rejected() {
        let _ = CapabilityId::new("world.section-store", 0);
    }
}
