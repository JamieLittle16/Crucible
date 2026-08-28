//! Allocation-free world-border projection for the R2B Minecraft Java 26.2 bootstrap.
//!
//! The world layer remains the semantic owner of mutable border state. This module receives only
//! the eight client-visible scalars used by the initial border packet and writes them in the
//! source-admitted 26.2 order. Packet identity is deliberately absent; the eventual R2B assembler
//! prefixes the generated compile-time packet ID.
//!
//! The payload contains four fixed `f64` values followed by one `VarLong` and three `VarInt`s. We
//! preflight the exact encoded payload length before the first write. The subsequent primitive
//! writes therefore cannot hit the packet bound, giving whole-payload rollback semantics without a
//! second writer, heap allocation, or target-local copy of Minecraft's integer encoders.

use helve_packet_core::{PacketCodecError, PacketWriter};

const FIXED_PREFIX_BYTES: usize = 4 * std::mem::size_of::<f64>();

/// Client-visible border state projected during initial Play bootstrap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldBorderPayload {
    /// Current border center X coordinate.
    pub center_x: f64,
    /// Current border center Z coordinate.
    pub center_z: f64,
    /// Border size at the beginning of the current interpolation.
    pub old_size: f64,
    /// Border size at the end of the current interpolation.
    pub new_size: f64,
    /// Remaining interpolation duration in milliseconds.
    pub lerp_time: i64,
    /// Absolute world-border maximum size.
    pub absolute_max_size: i32,
    /// Warning distance in blocks.
    pub warning_blocks: i32,
    /// Warning lead time in seconds.
    pub warning_time: i32,
}

impl WorldBorderPayload {
    /// Encodes the exact 26.2 initial-border scalar payload.
    ///
    /// # Errors
    ///
    /// Returns the bounded writer error before mutation when the complete payload does not fit.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        let payload_len = FIXED_PREFIX_BYTES
            + var_long_len(self.lerp_time)
            + var_int_len(self.absolute_max_size)
            + var_int_len(self.warning_blocks)
            + var_int_len(self.warning_time);
        preflight(writer, payload_len)?;

        writer.write_f64(self.center_x)?;
        writer.write_f64(self.center_z)?;
        writer.write_f64(self.old_size)?;
        writer.write_f64(self.new_size)?;
        writer.write_var_long(self.lerp_time)?;
        writer.write_var_int(self.absolute_max_size)?;
        writer.write_var_int(self.warning_blocks)?;
        writer.write_var_int(self.warning_time)
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

fn var_int_len(value: i32) -> usize {
    unsigned_var_len(u64::from(value.cast_unsigned()))
}

fn var_long_len(value: i64) -> usize {
    unsigned_var_len(value.cast_unsigned())
}

fn unsigned_var_len(value: u64) -> usize {
    let significant_bits = u64::BITS - value.leading_zeros();
    usize::try_from(significant_bits.max(1).div_ceil(7)).expect("VarInt/VarLong length fits usize")
}

#[cfg(test)]
mod tests {
    use helve_packet_core::{PacketCodecError, PacketWriter};

    use super::{WorldBorderPayload, var_int_len, var_long_len};

    const VECTOR: WorldBorderPayload = WorldBorderPayload {
        center_x: 1.0,
        center_z: -2.0,
        old_size: 3.0,
        new_size: 4.0,
        lerp_time: 300,
        absolute_max_size: 10,
        warning_blocks: 5,
        warning_time: 15,
    };

    #[test]
    fn border_payload_matches_source_field_order_and_integer_encoding() {
        let mut writer = PacketWriter::new(37).expect("exact border payload bound");
        VECTOR.encode(&mut writer).expect("vector fits exactly");

        assert_eq!(
            writer.as_slice(),
            &[
                0x3f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // center x = 1
                0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // center z = -2
                0x40, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // old size = 3
                0x40, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // new size = 4
                0xac, 0x02, // lerp time = 300
                0x0a, // absolute max size
                0x05, // warning blocks
                0x0f, // warning time
            ]
        );
    }

    #[test]
    fn exact_preflight_preserves_existing_packet_prefix_on_rejection() {
        let mut writer = PacketWriter::new(37).expect("one byte short after prefix");
        writer.write_u8(0x55).expect("existing packet id prefix");

        let error = VECTOR
            .encode(&mut writer)
            .expect_err("37-byte payload cannot fit in remaining 36 bytes");
        assert_eq!(
            error,
            PacketCodecError::PacketLimitExceeded {
                attempted: 38,
                maximum: 37,
            }
        );
        assert_eq!(writer.as_slice(), &[0x55]);
    }

    #[test]
    fn exact_preflight_accepts_payload_after_existing_prefix() {
        let mut writer = PacketWriter::new(38).expect("prefix plus exact payload");
        writer.write_u8(0x55).expect("existing packet id prefix");
        VECTOR.encode(&mut writer).expect("payload fits exactly");
        assert_eq!(writer.len(), 38);
        assert_eq!(writer.as_slice()[0], 0x55);
    }

    #[test]
    fn local_length_preflight_matches_packet_core_boundaries() {
        assert_eq!(var_int_len(0), 1);
        assert_eq!(var_int_len(127), 1);
        assert_eq!(var_int_len(128), 2);
        assert_eq!(var_int_len(i32::MAX), 5);
        assert_eq!(var_int_len(-1), 5);
        assert_eq!(var_int_len(i32::MIN), 5);

        assert_eq!(var_long_len(0), 1);
        assert_eq!(var_long_len(127), 1);
        assert_eq!(var_long_len(128), 2);
        assert_eq!(var_long_len(i64::MAX), 9);
        assert_eq!(var_long_len(-1), 10);
        assert_eq!(var_long_len(i64::MIN), 10);
    }
}
