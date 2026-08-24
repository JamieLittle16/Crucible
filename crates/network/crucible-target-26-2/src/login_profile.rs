//! Compact accepted Login profile retained across the Configuration boundary.
//!
//! The selected offline path needs only the source-defined profile UUID and player name. Crucible
//! keeps those values inline instead of carrying a Mojang-shaped `GameProfile` object graph.

use std::str;

const MAX_PLAYER_NAME_ASCII_BYTES: usize = 16;

/// Minimal immutable profile state required after Login acceptance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct LoginProfile {
    id: [u8; 16],
    name: [u8; MAX_PLAYER_NAME_ASCII_BYTES],
    name_len: u8,
}

impl LoginProfile {
    /// Copies one already-source-validated printable-ASCII player name into fixed inline storage.
    pub(super) fn new(id: [u8; 16], name: &str) -> Self {
        debug_assert!(name.len() <= MAX_PLAYER_NAME_ASCII_BYTES);
        debug_assert!(name.as_bytes().iter().all(|byte| (b'!'..=b'~').contains(byte)));

        let mut stored = [0_u8; MAX_PLAYER_NAME_ASCII_BYTES];
        stored[..name.len()].copy_from_slice(name.as_bytes());
        let name_len = u8::try_from(name.len()).expect("validated player name length fits u8");
        Self {
            id,
            name: stored,
            name_len,
        }
    }

    /// Accepted offline profile UUID.
    pub(super) const fn id(self) -> [u8; 16] {
        self.id
    }

    /// Accepted player name.
    pub(super) fn name(&self) -> &str {
        let length = usize::from(self.name_len);
        str::from_utf8(&self.name[..length]).expect("stored Login profile name is printable ASCII")
    }
}

#[cfg(test)]
mod tests {
    use super::LoginProfile;

    #[test]
    fn profile_is_inline_and_roundtrips_empty_and_maximal_names() {
        let id = [0x5a; 16];
        for name in ["", "abcdefghijklmnop"] {
            let profile = LoginProfile::new(id, name);
            assert_eq!(profile.id(), id);
            assert_eq!(profile.name(), name);
        }
    }
}
