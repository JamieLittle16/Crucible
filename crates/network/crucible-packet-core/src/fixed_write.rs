//! Network-order fixed-width and bounded scalar packet writes.
//!
//! These are target-neutral byte primitives only. Packet identity, field meaning, validation policy
//! and Minecraft-version semantics stay with the target codec using them. Fixed-width methods and
//! `VarLong` both commit through `PacketWriter::write_bytes`, so one byte-budget check remains the
//! transactional mutation boundary.

use super::{PacketCodecError, PacketWriter};

impl PacketWriter {
    /// Creates an empty writer with an explicit retained-capacity hint.
    ///
    /// The semantic packet-byte bound remains `maximum`; `initial_capacity` only controls the
    /// allocation retained by the writer. This is useful for finite batch construction where a
    /// scratch writer is reused across multiple packet bodies and the caller already knows a tight
    /// maximum useful scratch size.
    ///
    /// # Errors
    ///
    /// Returns [`PacketCodecError::ZeroWriteLimit`] for a zero packet bound. An initial capacity
    /// larger than that bound is rejected with [`PacketCodecError::PacketLimitExceeded`] rather than
    /// silently reserving memory that can never hold a valid packet body.
    pub fn with_capacity(
        maximum: usize,
        initial_capacity: usize,
    ) -> Result<Self, PacketCodecError> {
        if maximum == 0 {
            return Err(PacketCodecError::ZeroWriteLimit);
        }
        if initial_capacity > maximum {
            return Err(PacketCodecError::PacketLimitExceeded {
                attempted: initial_capacity,
                maximum,
            });
        }
        Ok(Self {
            bytes: Vec::with_capacity(initial_capacity),
            maximum,
        })
    }

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

    /// Appends one signed Minecraft `VarLong` without allocating.
    ///
    /// The encoded value is assembled in a ten-byte stack buffer first, then committed with one
    /// `write_bytes` call. This gives the variable-width field the same fail-before-mutation
    /// semantics as fixed-width packet primitives.
    ///
    /// # Errors
    ///
    /// Returns the existing packet-limit error before mutation when the complete encoded value
    /// would not fit.
    pub fn write_var_long(&mut self, value: i64) -> Result<(), PacketCodecError> {
        let mut remaining = value.cast_unsigned();
        let mut encoded = [0_u8; 10];
        let mut length = 0_usize;

        loop {
            let low = (remaining & 0x7f).to_le_bytes()[0];
            remaining >>= 7;
            if remaining == 0 {
                encoded[length] = low;
                length += 1;
                break;
            }
            encoded[length] = low | 0x80;
            length += 1;
        }

        self.write_bytes(&encoded[..length])
    }
}

#[cfg(test)]
mod tests {
    use super::PacketWriter;
    use crate::PacketCodecError;

    #[test]
    fn explicit_capacity_is_bounded_and_retained_for_reuse() {
        assert!(matches!(
            PacketWriter::with_capacity(0, 0),
            Err(PacketCodecError::ZeroWriteLimit)
        ));
        assert_eq!(
            PacketWriter::with_capacity(4, 5).expect_err("capacity beyond bound must fail"),
            PacketCodecError::PacketLimitExceeded {
                attempted: 5,
                maximum: 4,
            }
        );

        let mut writer = PacketWriter::with_capacity(4, 4).expect("bounded pre-sized writer");
        writer.write_i32(0x1234_5678).expect("four bytes fit");
        assert_eq!(writer.remaining_capacity(), 0);
        writer.reset();
        writer
            .write_i32(-1)
            .expect("retained capacity remains usable");
        assert_eq!(writer.as_slice(), [0xff; 4]);
    }

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
    fn canonical_var_long_vectors_match_minecraft_encoding() {
        let vectors: &[(i64, &[u8])] = &[
            (0, &[0x00]),
            (1, &[0x01]),
            (127, &[0x7f]),
            (128, &[0x80, 0x01]),
            (255, &[0xff, 0x01]),
            (
                i64::MAX,
                &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f],
            ),
            (
                -1,
                &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
            ),
            (
                i64::MIN,
                &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01],
            ),
        ];

        for &(value, expected) in vectors {
            let mut writer = PacketWriter::new(10).expect("maximum VarLong width");
            writer.write_var_long(value).expect("canonical VarLong");
            assert_eq!(writer.as_slice(), expected, "value={value}");
        }
    }

    #[test]
    fn var_long_rejection_is_transactional() {
        let mut writer = PacketWriter::new(1).expect("one-byte writer");
        writer.write_u8(0x7a).expect("first byte fits");
        let before = writer.as_slice().to_vec();

        assert!(matches!(
            writer.write_var_long(-1),
            Err(PacketCodecError::PacketLimitExceeded { .. })
        ));
        assert_eq!(writer.as_slice(), before);
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
