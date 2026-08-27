//! Allocation-free Difficulty projection for the R2B Minecraft Java 26.2 bootstrap.
//!
//! This module encodes payload fields only; packet identity remains a generated compile-time fact
//! owned by the eventual R2B assembler. The semantic owner resolves difficulty/lock state, while the
//! target converts that compact state directly to the reviewed 26.2 wire representation.
//!
//! The complete payload is two bytes for the admitted 26.2 enum: one single-byte `VarInt` difficulty
//! ID followed by one canonical boolean byte. Building both bytes on the stack and appending them in
//! one operation gives one bounds check and whole-payload transactional mutation with no allocation.

use crucible_packet_core::{PacketCodecError, PacketWriter};

/// Exact Minecraft Java 26.2 difficulty IDs admitted from `Difficulty` source.
///
/// These are target wire facts, not a gameplay-policy type. A future target whose source changes the
/// IDs must requalify rather than silently inheriting this mapping.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Difficulty26_2 {
    /// Vanilla `PEACEFUL(0, "peaceful")`.
    Peaceful = 0,
    /// Vanilla `EASY(1, "easy")`.
    Easy = 1,
    /// Vanilla `NORMAL(2, "normal")`.
    Normal = 2,
    /// Vanilla `HARD(3, "hard")`.
    Hard = 3,
}

impl Difficulty26_2 {
    const fn wire_id(self) -> u8 {
        self as u8
    }
}

/// Source-admitted `ClientboundChangeDifficultyPacket` payload state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeDifficultyPayload {
    /// Difficulty already resolved by the level-data owner.
    pub difficulty: Difficulty26_2,
    /// Whether client difficulty controls are locked.
    pub locked: bool,
}

impl ChangeDifficultyPayload {
    /// Encodes the exact 26.2 payload: difficulty `VarInt`, then lock boolean.
    ///
    /// The admitted difficulty IDs are all below 128, so their canonical Minecraft `VarInt`
    /// representation is exactly one byte. This target specialization is intentionally guarded by
    /// the source fingerprints that pin all four IDs.
    ///
    /// # Errors
    ///
    /// Returns the bounded writer error before mutation when the complete two-byte payload does not
    /// fit.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        writer.write_bytes(&[self.difficulty.wire_id(), u8::from(self.locked)])
    }
}

#[cfg(test)]
mod tests {
    use crucible_packet_core::{PacketCodecError, PacketWriter};

    use super::{ChangeDifficultyPayload, Difficulty26_2};

    #[test]
    fn every_admitted_difficulty_and_lock_state_matches_wire_bytes() {
        let difficulties = [
            (Difficulty26_2::Peaceful, 0_u8),
            (Difficulty26_2::Easy, 1_u8),
            (Difficulty26_2::Normal, 2_u8),
            (Difficulty26_2::Hard, 3_u8),
        ];

        for (difficulty, expected_id) in difficulties {
            for locked in [false, true] {
                let mut writer = PacketWriter::new(2).expect("exact difficulty payload bound");
                ChangeDifficultyPayload { difficulty, locked }
                    .encode(&mut writer)
                    .expect("two-byte payload fits");
                assert_eq!(writer.as_slice(), &[expected_id, u8::from(locked)]);
            }
        }
    }

    #[test]
    fn selected_fresh_default_profile_is_normal_and_unlocked() {
        let mut writer = PacketWriter::new(2).expect("exact difficulty payload bound");
        ChangeDifficultyPayload {
            difficulty: Difficulty26_2::Normal,
            locked: false,
        }
        .encode(&mut writer)
        .expect("selected profile fits");

        assert_eq!(writer.as_slice(), &[0x02, 0x00]);
    }

    #[test]
    fn rejection_preserves_an_existing_packet_prefix() {
        let mut writer = PacketWriter::new(2).expect("prefix plus one remaining byte");
        writer
            .write_bytes(&[0x7f])
            .expect("existing packet prefix fits");

        let error = ChangeDifficultyPayload {
            difficulty: Difficulty26_2::Hard,
            locked: true,
        }
        .encode(&mut writer)
        .expect_err("complete two-byte payload must not partially append");

        assert!(matches!(
            error,
            PacketCodecError::PacketLimitExceeded { .. }
        ));
        assert_eq!(writer.as_slice(), &[0x7f]);
    }
}
