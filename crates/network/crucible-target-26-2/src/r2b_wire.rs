//! Source-admitted reusable wire primitives for the R2B Minecraft Java 26.2 bootstrap.
//!
//! This file remains behind the R2B qualification harness until the admitted Play-entry gate is
//! installed in the repository evidence tree. It contains only reusable primitives established by
//! the final-seam review: packed `BlockPos`, registry/id-mapper `VarInt` identities, reference-holder
//! `id + 1` encoding and bounded map counts. It deliberately does not contain packet IDs, stage
//! order, world/chunk/light representation, or a generic Mojang-style registry runtime.

use crucible_packet_core::{PacketCodecError, PacketWriter};

const PACKED_XZ_BITS: u32 = 26;
const PACKED_Y_BITS: u32 = 12;
const PACKED_Z_SHIFT: u32 = PACKED_Y_BITS;
const PACKED_X_SHIFT: u32 = PACKED_Y_BITS + PACKED_XZ_BITS;
const PACKED_XZ_MASK: u64 = (1_u64 << PACKED_XZ_BITS) - 1;
const PACKED_Y_MASK: u64 = (1_u64 << PACKED_Y_BITS) - 1;

/// Fail-closed semantic error for the narrow reusable R2B wire seam.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R2bWireError {
    /// The bounded packet writer rejected the encoded field.
    Codec(PacketCodecError),
    /// Registry-backed IDs are non-negative; a negative value cannot name a registered entry.
    NegativeRegistryId(i32),
    /// Reference holders encode a registered ID as `id + 1`; `i32::MAX` cannot be represented.
    ReferenceHolderIdOverflow(i32),
    /// A bounded map/collection exceeded its source-admitted maximum before any bytes were written.
    CollectionTooLarge {
        /// Proposed element count.
        length: usize,
        /// Source-admitted maximum count.
        maximum: usize,
    },
    /// A collection count cannot be represented by Minecraft's signed `VarInt` length prefix.
    CollectionLengthDoesNotFitVarInt(usize),
}

impl From<PacketCodecError> for R2bWireError {
    fn from(value: PacketCodecError) -> Self {
        Self::Codec(value)
    }
}

/// Packs one block position using Minecraft 26.2's `26 / 12 / 26` X/Y/Z bit layout.
///
/// This mirrors `BlockPos.asLong`: X occupies bits 38..63, Z bits 12..37 and Y bits 0..11. Java's
/// implementation masks each signed coordinate before combining it, so this function intentionally
/// preserves the same low-bit truncation instead of imposing an invented coordinate-range policy.
#[must_use]
pub fn pack_block_pos(x: i32, y: i32, z: i32) -> i64 {
    let x_bits = i64::from(x).cast_unsigned() & PACKED_XZ_MASK;
    let y_bits = i64::from(y).cast_unsigned() & PACKED_Y_MASK;
    let z_bits = i64::from(z).cast_unsigned() & PACKED_XZ_MASK;
    ((x_bits << PACKED_X_SHIFT) | (z_bits << PACKED_Z_SHIFT) | y_bits).cast_signed()
}

/// Writes one packed `BlockPos` in network byte order.
///
/// # Errors
///
/// Returns the existing bounded packet-writer error without partially writing the field.
pub fn write_block_pos(
    writer: &mut PacketWriter,
    x: i32,
    y: i32,
    z: i32,
) -> Result<(), R2bWireError> {
    writer.write_i64(pack_block_pos(x, y, z))?;
    Ok(())
}

/// Writes one registry/id-mapper identity as a non-negative Minecraft `VarInt`.
///
/// The caller supplies a target-owned resolved ID; this helper is intentionally not a registry
/// lookup layer. A missing registry entry must fail before reaching this function.
///
/// # Errors
///
/// Rejects negative IDs and propagates bounded packet-writer failure without mutation.
pub fn write_registry_id(writer: &mut PacketWriter, id: i32) -> Result<(), R2bWireError> {
    if id < 0 {
        return Err(R2bWireError::NegativeRegistryId(id));
    }
    writer.write_var_int(id)?;
    Ok(())
}

/// Writes one registered holder reference using the source-admitted `id + 1` marker.
///
/// Marker zero is reserved by vanilla's holder codec for an inline/direct holder. R2B's selected
/// dynamic dimension path uses a resolved registry reference and therefore emits `registry_id + 1`.
///
/// # Errors
///
/// Rejects negative IDs and `i32::MAX`, then propagates bounded writer failure transactionally.
pub fn write_reference_holder_id(
    writer: &mut PacketWriter,
    registry_id: i32,
) -> Result<(), R2bWireError> {
    if registry_id < 0 {
        return Err(R2bWireError::NegativeRegistryId(registry_id));
    }
    let encoded = registry_id
        .checked_add(1)
        .ok_or(R2bWireError::ReferenceHolderIdOverflow(registry_id))?;
    writer.write_var_int(encoded)?;
    Ok(())
}

/// Writes the count prefix for a source-bounded map/collection.
///
/// This is only the reusable count law from `ByteBufCodecs.map`; key/value ordering and concrete
/// codecs remain owned by the target surface using the helper.
///
/// # Errors
///
/// Checks the semantic maximum and signed-`VarInt` representability before touching the writer.
pub fn write_bounded_collection_len(
    writer: &mut PacketWriter,
    length: usize,
    maximum: usize,
) -> Result<(), R2bWireError> {
    if length > maximum {
        return Err(R2bWireError::CollectionTooLarge { length, maximum });
    }
    let encoded = i32::try_from(length)
        .map_err(|_| R2bWireError::CollectionLengthDoesNotFitVarInt(length))?;
    writer.write_var_int(encoded)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crucible_packet_core::{PacketCodecError, PacketWriter};

    use super::{
        R2bWireError, pack_block_pos, write_block_pos, write_bounded_collection_len,
        write_reference_holder_id, write_registry_id,
    };

    #[test]
    fn packed_block_pos_matches_the_exact_bit_layout() {
        assert_eq!(pack_block_pos(0, 0, 0), 0);
        assert_eq!(pack_block_pos(1, 0, 0), 1_i64 << 38);
        assert_eq!(pack_block_pos(0, 0, 1), 1_i64 << 12);
        assert_eq!(pack_block_pos(0, 1, 0), 1);
        assert_eq!(pack_block_pos(-1, -1, -1), -1);
        assert_eq!(
            pack_block_pos(12_345, 2_047, -54_321),
            0x000c_0e7f_f2bc_f7ff_i64
        );
    }

    #[test]
    fn packed_block_pos_intentionally_masks_like_java() {
        assert_eq!(pack_block_pos(1 << 26, 0, 0), 0);
        assert_eq!(pack_block_pos(0, 1 << 12, 0), 0);
        assert_eq!(pack_block_pos(0, 0, 1 << 26), 0);
        assert_eq!(pack_block_pos(i32::MIN, i32::MIN, i32::MIN), 0);
    }

    #[test]
    fn block_pos_writer_uses_network_order_and_is_transactional() {
        let packed = pack_block_pos(-12, 64, 345);
        let mut writer = PacketWriter::new(8).expect("exact block-pos body");
        write_block_pos(&mut writer, -12, 64, 345).expect("packed position fits");
        assert_eq!(writer.as_slice(), packed.to_be_bytes());

        let mut too_small = PacketWriter::new(7).expect("non-zero bound");
        assert!(matches!(
            write_block_pos(&mut too_small, -12, 64, 345),
            Err(R2bWireError::Codec(
                PacketCodecError::PacketLimitExceeded { .. }
            ))
        ));
        assert!(too_small.is_empty());
    }

    #[test]
    fn registry_ids_and_reference_holder_markers_are_exact() {
        let mut raw = PacketWriter::new(8).expect("writer");
        write_registry_id(&mut raw, 0).expect("registry zero");
        write_registry_id(&mut raw, 127).expect("registry 127");
        assert_eq!(raw.as_slice(), &[0x00, 0x7f]);

        let mut holder = PacketWriter::new(8).expect("writer");
        write_reference_holder_id(&mut holder, 0).expect("first reference");
        write_reference_holder_id(&mut holder, 127).expect("reference 127");
        assert_eq!(holder.as_slice(), &[0x01, 0x80, 0x01]);
    }

    #[test]
    fn invalid_registry_ids_fail_before_writer_mutation() {
        let mut writer = PacketWriter::new(8).expect("writer");
        assert_eq!(
            write_registry_id(&mut writer, -1),
            Err(R2bWireError::NegativeRegistryId(-1))
        );
        assert_eq!(
            write_reference_holder_id(&mut writer, -7),
            Err(R2bWireError::NegativeRegistryId(-7))
        );
        assert_eq!(
            write_reference_holder_id(&mut writer, i32::MAX),
            Err(R2bWireError::ReferenceHolderIdOverflow(i32::MAX))
        );
        assert!(writer.is_empty());
    }

    #[test]
    fn bounded_collection_count_rejects_semantics_before_bytes() {
        let mut writer = PacketWriter::new(8).expect("writer");
        write_bounded_collection_len(&mut writer, 3, 3).expect("at bound");
        assert_eq!(writer.as_slice(), &[0x03]);

        let before = writer.as_slice().to_vec();
        assert_eq!(
            write_bounded_collection_len(&mut writer, 4, 3),
            Err(R2bWireError::CollectionTooLarge {
                length: 4,
                maximum: 3,
            })
        );
        assert_eq!(writer.as_slice(), before);
    }

    #[test]
    fn impossible_varint_collection_length_fails_without_mutation() {
        if usize::BITS <= 32 {
            return;
        }
        let length = (i32::MAX as usize) + 1;
        let mut writer = PacketWriter::new(8).expect("writer");
        assert_eq!(
            write_bounded_collection_len(&mut writer, length, length),
            Err(R2bWireError::CollectionLengthDoesNotFitVarInt(length))
        );
        assert!(writer.is_empty());
    }
}
