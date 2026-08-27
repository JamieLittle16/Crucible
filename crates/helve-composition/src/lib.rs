//! Generated static composition wiring. Regenerate with `tools/composition_resolver.py`.
//!
//! The generated surface deliberately re-exports concrete provider types. There is no
//! runtime service map or mandatory trait-object hop in this composition boundary.

#![forbid(unsafe_code)]

pub const COMPOSITION_SHA256: &str = "b49458049dc651a9d7118e5737b4750e4f1d081a0fb04533611f2493b0eb4e11";
pub const PROFILE: &str = "reference";
pub const MINECRAFT_VERSION: &str = "26.2";

pub use crucible_world_reference::DirectBlockSection as SectionStore;

#[cfg(test)]
mod tests {
    use super::{COMPOSITION_SHA256, MINECRAFT_VERSION, PROFILE};

    #[test]
    fn generated_identity_is_nonempty_and_pinned() {
        assert_eq!(COMPOSITION_SHA256.len(), 64);
        assert_eq!(MINECRAFT_VERSION, "26.2");
        assert_eq!(PROFILE, "reference");
    }
}
