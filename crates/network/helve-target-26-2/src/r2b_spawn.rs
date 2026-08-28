//! Allocation-free default-spawn projection for the R2B Minecraft Java 26.2 bootstrap.
//!
//! Vanilla serializes `LevelData.RespawnData` as `GlobalPos + yaw + pitch`. Crucible keeps no
//! `GlobalPos`/`RespawnData` object graph in networking: the world/level owner supplies one already
//! validated dimension identifier, three block coordinates and the two client-visible angles.
//!
//! The target writes the admitted 32767-UTF-16-unit Identifier string law, Minecraft's packed
//! `BlockPos`, then two network-order `f32` values. An exact non-mutating preflight covers the entire
//! payload, so the caller-owned bounded writer cannot be left with a partial spawn packet.

use helve_packet_core::{PacketCodecError, PacketWriter};

use crate::r2b_wire::pack_block_pos;

const IDENTIFIER_MAX_UTF16_UNITS: usize = 32_767;
const FIXED_SUFFIX_BYTES: usize = 8 + 4 + 4;

/// Client-visible default respawn state needed by the 26.2 bootstrap packet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DefaultSpawnPayload<'a> {
    /// Canonical dimension identifier already validated by the semantic/composition owner.
    pub dimension: &'a str,
    /// Spawn block X coordinate.
    pub x: i32,
    /// Spawn block Y coordinate.
    pub y: i32,
    /// Spawn block Z coordinate.
    pub z: i32,
    /// Client-visible respawn yaw.
    pub yaw: f32,
    /// Client-visible respawn pitch.
    pub pitch: f32,
}

impl DefaultSpawnPayload<'_> {
    /// Encodes the exact 26.2 `RespawnData` wire projection.
    ///
    /// # Errors
    ///
    /// Rejects an over-budget complete payload before mutation. Identifier UTF validation is then
    /// delegated to packet-core's already-admitted bounded string writer, which is itself
    /// transactional.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        let identifier_bytes = string_wire_len(self.dimension)?;
        let payload_len = identifier_bytes
            .checked_add(FIXED_SUFFIX_BYTES)
            .ok_or(PacketCodecError::LengthOverflow)?;
        preflight(writer, payload_len)?;

        writer.write_string(self.dimension, IDENTIFIER_MAX_UTF16_UNITS)?;
        writer.write_i64(pack_block_pos(self.x, self.y, self.z))?;
        writer.write_f32(self.yaw)?;
        writer.write_f32(self.pitch)
    }
}

fn preflight(writer: &PacketWriter, additional: usize) -> Result<(), PacketCodecError> {
    if additional <= writer.remaining_capacity() {
        return Ok(());
    }

    let attempted = writer
        .len()
        .checked_add(additional)
        .ok_or(PacketCodecError::LengthOverflow)?;
    let maximum = writer
        .len()
        .checked_add(writer.remaining_capacity())
        .ok_or(PacketCodecError::LengthOverflow)?;
    Err(PacketCodecError::PacketLimitExceeded { attempted, maximum })
}

fn string_wire_len(value: &str) -> Result<usize, PacketCodecError> {
    let byte_len = i32::try_from(value.len()).map_err(|_| PacketCodecError::LengthOverflow)?;
    let significant_bits = i32::BITS - byte_len.cast_unsigned().leading_zeros();
    let prefix_len = usize::try_from(significant_bits.max(1).div_ceil(7))
        .expect("non-negative VarInt prefix length fits usize");
    prefix_len
        .checked_add(value.len())
        .ok_or(PacketCodecError::LengthOverflow)
}

#[cfg(test)]
mod tests {
    use helve_packet_core::{PacketCodecError, PacketWriter};

    use super::DefaultSpawnPayload;

    const DEFAULT: DefaultSpawnPayload<'static> = DefaultSpawnPayload {
        dimension: "minecraft:overworld",
        x: 0,
        y: 0,
        z: 0,
        yaw: 0.0,
        pitch: 0.0,
    };

    #[test]
    fn selected_default_respawn_matches_exact_wire_bytes() {
        let mut writer = PacketWriter::new(36).expect("exact default-spawn payload bound");
        DEFAULT.encode(&mut writer).expect("default spawn fits");

        let mut expected = Vec::with_capacity(36);
        expected.push(0x13); // UTF-8 byte length of minecraft:overworld.
        expected.extend_from_slice(b"minecraft:overworld");
        expected.extend_from_slice(&[0; 8]); // packed BlockPos.ZERO.
        expected.extend_from_slice(&[0; 4]); // yaw = +0.0F.
        expected.extend_from_slice(&[0; 4]); // pitch = +0.0F.
        assert_eq!(writer.as_slice(), expected);
    }

    #[test]
    fn whole_payload_preflight_preserves_existing_packet_prefix() {
        let mut writer = PacketWriter::new(36).expect("one byte short after prefix");
        writer.write_u8(0x55).expect("existing packet-id prefix");

        let error = DEFAULT
            .encode(&mut writer)
            .expect_err("36-byte payload cannot fit in remaining 35 bytes");
        assert_eq!(
            error,
            PacketCodecError::PacketLimitExceeded {
                attempted: 37,
                maximum: 36,
            }
        );
        assert_eq!(writer.as_slice(), &[0x55]);
    }

    #[test]
    fn packed_position_precedes_angles_without_intermediate_allocation() {
        let payload = DefaultSpawnPayload {
            dimension: "minecraft:the_nether",
            x: 1,
            y: 64,
            z: -1,
            yaw: 90.0,
            pitch: -30.0,
        };
        let mut writer = PacketWriter::new(64).expect("bounded spawn writer");
        payload.encode(&mut writer).expect("spawn fits");

        let bytes = writer.as_slice();
        assert_eq!(bytes[0], 0x14); // minecraft:the_nether is 20 UTF-8 bytes.
        assert_eq!(&bytes[1..21], b"minecraft:the_nether");
        assert_eq!(&bytes[21..29], &0x0000_007f_ffff_f040_i64.to_be_bytes());
        assert_eq!(&bytes[29..33], &90.0_f32.to_be_bytes());
        assert_eq!(&bytes[33..37], &(-30.0_f32).to_be_bytes());
    }

    #[test]
    fn overlong_identifier_rejection_leaves_writer_unchanged() {
        let dimension = "a".repeat(32_768);
        let mut writer = PacketWriter::new(40_000).expect("large bounded writer");
        writer.write_u8(0x55).expect("existing prefix");

        assert!(
            DEFAULT
                .encode(&mut PacketWriter::new(36).expect("control writer"))
                .is_ok()
        );
        let error = DefaultSpawnPayload {
            dimension: &dimension,
            ..DEFAULT
        }
        .encode(&mut writer)
        .expect_err("Identifier string exceeds 32767 UTF-16 units");

        assert!(matches!(error, PacketCodecError::Wire(_)));
        assert_eq!(writer.as_slice(), &[0x55]);
    }
}
