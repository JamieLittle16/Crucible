//! Network-order fixed-width packet writes.
//!
//! These are target-neutral byte primitives only. Packet identity, field meaning, validation policy
//! and Minecraft-version semantics stay with the target codec using them. Every method delegates to
//! `PacketWriter::write_bytes`, so the existing single byte-budget check remains the transactional
//! mutation boundary.

use super::{PacketCodecError, PacketWriter};

impl PacketWriter {
    /// Clears the current packet body while retaining the writer's allocation for reuse.
    ///
    /// This is intended for bounded batch construction where one scratch writer serializes several
    /// independent packet bodies into caller-owned storage. It changes no configured byte limit and
    /// performs no allocation itself.
    pub fn reset(&mut self) {
        self.bytes.clear();
    }

    /// Appends one unsigned byte.
    ///
    /// # Errors
    ///
    /// Returns the existing packet-limit error before mutation when one byte would not fit.
    pub fn write_u8(&mut self, value: u8) -> Result<(), PacketCodecError> {
        self.write_bytes(&[value])
    }

    /// Appends one network-order signed 32-bit scalar.
    ///
    /// # Errors
    ///
    /// Returns the existing packet-limit error before mutation when four bytes would not fit.
    pub fn write_i32(&mut self, value: i32) -> Result<(), PacketCodecError> {
        self.write_bytes(&value.to_be_bytes())
    }

    /// Appends one IEEE-754 binary32 value in network byte order.
    ///
    /// No normalization is performed: the exact Rust `f32` bit pattern is placed on the wire.
    ///
    /// # Errors
    ///
    /// Returns the existing packet-limit error before mutation when four bytes would not fit.
    pub fn write_f32(&mut self, value: f32) -> Result<(), PacketCodecError> {
        self.write_bytes(&value.to_bits().to_be_bytes())
    }

    /// Appends one IEEE-754 binary64 value in network byte order.
    ///
    /// No normalization is performed: the exact Rust `f64` bit pattern is placed on the wire.
    ///
    /// # Errors
    ///
    /// Returns the existing packet-limit error before mutation when eight bytes would not fit.
    pub fn write_f64(&mut self, value: f64) -> Result<(), PacketCodecError> {
        self.write_bytes(&value.to_bits().to_be_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::PacketWriter;
    use crate::PacketCodecError;

    #[test]
    fn reset_reuses_writer_without_changing_its_bound() {
        let mut writer = PacketWriter::new(4).expect("writer");
        writer.write_i32(0x1234_5678).expect("four bytes fit");
        assert_eq!(writer.remaining_capacity(), 0);

        writer.reset();
        assert!(writer.is_empty());
        assert_eq!(writer.remaining_capacity(), 4);
        writer.write_i32(-1).expect("same four-byte bound remains");
        assert_eq!(writer.as_slice(), [0xff; 4]);
    }

    #[test]
    fn fixed_width_writes_are_exact_network_order_bits() {
        let mut writer = PacketWriter::new(17).expect("exact fixed-width body");
        writer.write_u8(0xa5).expect("u8");
        writer.write_i32(-0x0123_4567).expect("i32");
        writer.write_f32(-13.25).expect("f32");
        writer.write_f64(0.15625).expect("f64");

        let mut expected = Vec::with_capacity(17);
        expected.push(0xa5);
        expected.extend_from_slice(&(-0x0123_4567_i32).to_be_bytes());
        expected.extend_from_slice(&(-13.25_f32).to_bits().to_be_bytes());
        expected.extend_from_slice(&(0.15625_f64).to_bits().to_be_bytes());
        assert_eq!(writer.as_slice(), expected);
    }

    #[test]
    fn floating_point_payloads_preserve_exact_bits() {
        for bits in [0_u32, 0x8000_0000, 0x7f80_0000, 0xff80_0000, 0x7fc0_1234] {
            let mut writer = PacketWriter::new(4).expect("f32 body");
            writer
                .write_f32(f32::from_bits(bits))
                .expect("four bytes fit");
            assert_eq!(writer.as_slice(), bits.to_be_bytes());
        }

        for bits in [
            0_u64,
            0x8000_0000_0000_0000,
            0x7ff0_0000_0000_0000,
            0xfff0_0000_0000_0000,
            0x7ff8_0000_0000_1234,
        ] {
            let mut writer = PacketWriter::new(8).expect("f64 body");
            writer
                .write_f64(f64::from_bits(bits))
                .expect("eight bytes fit");
            assert_eq!(writer.as_slice(), bits.to_be_bytes());
        }
    }

    #[test]
    fn every_fixed_width_rejection_is_transactional() {
        let mut writer = PacketWriter::new(1).expect("one-byte writer");
        writer.write_u8(0x7a).expect("first byte fits");
        let before = writer.as_slice().to_vec();

        for result in [
            writer.write_i32(7),
            writer.write_f32(1.0),
            writer.write_f64(1.0),
        ] {
            assert!(matches!(
                result,
                Err(PacketCodecError::PacketLimitExceeded { .. })
            ));
            assert_eq!(writer.as_slice(), before);
        }
    }
}
