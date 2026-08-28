//! Fresh-profile initial recipe-book add projection for R2B Minecraft Java 26.2.
//!
//! Source law builds this packet from `ServerRecipeBook.known` and always publishes it with
//! `replace=true`. The selected R2B profile explicitly admits fresh/default player state with an
//! empty known-recipe set; persisted-player recipe state is a later profile expansion. Crucible
//! therefore emits only `VarInt(0) + true` and does not source-admit or construct the general
//! recipe-display entry graph on this hot path.

use helve_packet_core::{PacketCodecError, PacketWriter};

/// Encodes the selected fresh/default recipe-add payload: zero entries and `replace=true`.
///
/// This path has no semantic state, so it is deliberately a function rather than a zero-sized
/// payload object. The complete two-byte payload is appended transactionally by one bounded write.
pub(crate) fn encode_fresh_recipe_book_add(
    writer: &mut PacketWriter,
) -> Result<(), PacketCodecError> {
    writer.write_bytes(&[0x00, 0x01])
}

#[cfg(test)]
mod tests {
    use helve_packet_core::{PacketCodecError, PacketWriter};

    use super::encode_fresh_recipe_book_add;

    #[test]
    fn fresh_recipe_book_add_is_empty_entries_with_replace_true() {
        let mut writer = PacketWriter::new(2).expect("exact fresh recipe-add body");
        encode_fresh_recipe_book_add(&mut writer).expect("two-byte payload fits");
        assert_eq!(writer.as_slice(), &[0x00, 0x01]);
    }

    #[test]
    fn fixed_payload_rejection_preserves_existing_prefix() {
        let mut writer = PacketWriter::new(2).expect("one byte short after packet id");
        writer.write_u8(0x4a).expect("recipe-add packet id");
        assert_eq!(
            encode_fresh_recipe_book_add(&mut writer),
            Err(PacketCodecError::PacketLimitExceeded {
                attempted: 3,
                maximum: 2,
            })
        );
        assert_eq!(writer.as_slice(), &[0x4a]);
    }
}
