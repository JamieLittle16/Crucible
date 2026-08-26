//! Compact recipe-book projection for the R2B Minecraft Java 26.2 bootstrap.
//!
//! Vanilla represents the initial recipe-book settings as four `TypeSettings` values, each holding
//! `(open, filtering)`, and serializes those eight booleans in crafting/furnace/blast-furnace/smoker
//! order. Crucible keeps the same observable state in one typed byte and expands it only at the wire
//! boundary. This avoids carrying eight booleans or a Mojang-shaped object graph per player.

use crucible_packet_core::{PacketCodecError, PacketWriter};

/// Compact source-admitted recipe-book settings mask.
///
/// Every bit corresponds to one exact 26.2 wire boolean. The private representation prevents
/// unsupported bits or field-order drift from entering through arbitrary raw construction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecipeBookSettingFlags(u8);

impl RecipeBookSettingFlags {
    /// Vanilla default: every recipe book closed and every filtering mode disabled.
    pub const NONE: Self = Self(0);
    /// Crafting book open.
    pub const CRAFTING_OPEN: Self = Self(1 << 0);
    /// Crafting book filtering craftable recipes.
    pub const CRAFTING_FILTERING: Self = Self(1 << 1);
    /// Furnace book open.
    pub const FURNACE_OPEN: Self = Self(1 << 2);
    /// Furnace book filtering craftable recipes.
    pub const FURNACE_FILTERING: Self = Self(1 << 3);
    /// Blast-furnace book open.
    pub const BLAST_FURNACE_OPEN: Self = Self(1 << 4);
    /// Blast-furnace book filtering craftable recipes.
    pub const BLAST_FURNACE_FILTERING: Self = Self(1 << 5);
    /// Smoker book open.
    pub const SMOKER_OPEN: Self = Self(1 << 6);
    /// Smoker book filtering craftable recipes.
    pub const SMOKER_FILTERING: Self = Self(1 << 7);

    /// Combines two source-valid setting sets without exposing raw-bit construction.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn wire_bool(self, bit: u32) -> u8 {
        (self.0 >> bit) & 1
    }
}

/// Initial recipe-book settings payload.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecipeBookSettingsPayload {
    /// Already-resolved compact settings state.
    pub flags: RecipeBookSettingFlags,
}

impl RecipeBookSettingsPayload {
    /// Encodes the exact eight 26.2 boolean fields in vanilla `STREAM_CODEC` order.
    ///
    /// # Errors
    ///
    /// Returns the bounded writer error before mutation when all eight bytes do not fit.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        writer.write_bytes(&[
            self.flags.wire_bool(0),
            self.flags.wire_bool(1),
            self.flags.wire_bool(2),
            self.flags.wire_bool(3),
            self.flags.wire_bool(4),
            self.flags.wire_bool(5),
            self.flags.wire_bool(6),
            self.flags.wire_bool(7),
        ])
    }
}

#[cfg(test)]
mod tests {
    use crucible_packet_core::{PacketCodecError, PacketWriter};

    use super::{RecipeBookSettingFlags, RecipeBookSettingsPayload};

    #[test]
    fn selected_fresh_profile_is_eight_false_booleans() {
        let mut writer = PacketWriter::new(8).expect("exact recipe-settings payload bound");
        RecipeBookSettingsPayload {
            flags: RecipeBookSettingFlags::NONE,
        }
        .encode(&mut writer)
        .expect("default settings fit");

        assert_eq!(writer.as_slice(), &[0; 8]);
    }

    #[test]
    fn every_setting_bit_expands_in_source_codec_order() {
        let all = RecipeBookSettingFlags::CRAFTING_OPEN
            .union(RecipeBookSettingFlags::CRAFTING_FILTERING)
            .union(RecipeBookSettingFlags::FURNACE_OPEN)
            .union(RecipeBookSettingFlags::FURNACE_FILTERING)
            .union(RecipeBookSettingFlags::BLAST_FURNACE_OPEN)
            .union(RecipeBookSettingFlags::BLAST_FURNACE_FILTERING)
            .union(RecipeBookSettingFlags::SMOKER_OPEN)
            .union(RecipeBookSettingFlags::SMOKER_FILTERING);
        let mut writer = PacketWriter::new(8).expect("recipe-settings payload");
        RecipeBookSettingsPayload { flags: all }
            .encode(&mut writer)
            .expect("all settings fit");

        assert_eq!(writer.as_slice(), &[1; 8]);
    }

    #[test]
    fn individual_bits_preserve_pair_and_book_order() {
        let flags = RecipeBookSettingFlags::CRAFTING_FILTERING
            .union(RecipeBookSettingFlags::FURNACE_OPEN)
            .union(RecipeBookSettingFlags::BLAST_FURNACE_FILTERING)
            .union(RecipeBookSettingFlags::SMOKER_OPEN);
        let mut writer = PacketWriter::new(8).expect("recipe-settings payload");
        RecipeBookSettingsPayload { flags }
            .encode(&mut writer)
            .expect("settings fit");

        assert_eq!(writer.as_slice(), &[0, 1, 1, 0, 0, 1, 1, 0]);
    }

    #[test]
    fn settings_payload_is_one_transaction() {
        let mut writer = PacketWriter::new(7).expect("one byte too small");
        let error = RecipeBookSettingsPayload {
            flags: RecipeBookSettingFlags::NONE,
        }
        .encode(&mut writer)
        .expect_err("eight-byte payload must not fit");

        assert!(matches!(
            error,
            PacketCodecError::PacketLimitExceeded { .. }
        ));
        assert!(writer.is_empty());
    }
}
