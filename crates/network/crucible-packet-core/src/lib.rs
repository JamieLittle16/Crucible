//! Allocation-free packet-body reads and bounded transactional writes for Crucible.
//!
//! This layer sits above Minecraft frame extraction and below target-version packet semantics. It
//! contains no packet IDs, protocol-version constants, connection-state transitions or runtime
//! policy. Variable-length decoded data borrows directly from the caller's frame payload.

#![forbid(unsafe_code)]

use crucible_protocol_core::{
    DecodeResult, WireError, decode_string, decode_var_int, encode_string, encode_var_int,
    var_int_len,
};

/// Generic packet field category used by fail-closed truncation diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketField {
    /// Signed Minecraft `VarInt`.
    VarInt,
    /// Network-order unsigned 16-bit scalar.
    U16,
    /// Network-order signed 64-bit scalar.
    I64,
    /// Network-order unsigned 64-bit scalar.
    U64,
    /// Canonical one-byte boolean.
    Boolean,
    /// Minecraft length-prefixed UTF-8 string.
    String,
}

/// Fail-closed packet-body codec errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketCodecError {
    /// A complete packet body ended before the requested field completed.
    Truncated {
        field: PacketField,
        remaining: usize,
    },
    /// A boolean byte was neither canonical false (`0`) nor canonical true (`1`).
    InvalidBoolean(u8),
    /// Packet decoding finished before all payload bytes were consumed.
    TrailingBytes { remaining: usize },
    /// A packet writer was configured with a zero-byte bound.
    ZeroWriteLimit,
    /// Appending a field would exceed the configured packet-body byte bound.
    PacketLimitExceeded { attempted: usize, maximum: usize },
    /// Integer arithmetic needed to preserve a bound overflowed `usize`.
    LengthOverflow,
    /// The underlying Minecraft wire primitive rejected a field.
    Wire(WireError),
}

impl From<WireError> for PacketCodecError {
    fn from(value: WireError) -> Self {
        Self::Wire(value)
    }
}

/// Borrowed cursor over one already-complete Minecraft packet payload.
///
/// Every method is transactional with respect to cursor position: malformed or truncated fields
/// leave the cursor unchanged. Strings and trailing raw bytes borrow directly from `payload`.
#[derive(Clone, Copy, Debug)]
pub struct PacketReader<'a> {
    payload: &'a [u8],
    cursor: usize,
}

impl<'a> PacketReader<'a> {
    /// Starts reading at the beginning of `payload`.
    #[must_use]
    pub const fn new(payload: &'a [u8]) -> Self {
        Self { payload, cursor: 0 }
    }

    /// Number of bytes successfully consumed so far.
    #[must_use]
    pub const fn consumed_len(&self) -> usize {
        self.cursor
    }

    /// Number of payload bytes not yet consumed.
    #[must_use]
    pub const fn remaining_len(&self) -> usize {
        self.payload.len() - self.cursor
    }

    /// Whether every payload byte has been consumed.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.cursor == self.payload.len()
    }

    /// Reads one signed Minecraft `VarInt`.
    ///
    /// # Errors
    ///
    /// Returns a wire error for an overlong value or a truncation error when this complete packet
    /// ends before the `VarInt` terminates. Cursor position changes only on success.
    pub fn read_var_int(&mut self) -> Result<i32, PacketCodecError> {
        let remaining = self.remaining();
        let DecodeResult::Complete { value, consumed } = decode_var_int(remaining)? else {
            return Err(self.truncated(PacketField::VarInt));
        };
        self.cursor += consumed;
        Ok(value)
    }

    /// Reads one network-order unsigned 16-bit scalar.
    ///
    /// # Errors
    ///
    /// Returns a truncation error without advancing when fewer than two bytes remain.
    pub fn read_u16(&mut self) -> Result<u16, PacketCodecError> {
        let bytes = self.fixed(PacketField::U16, 2)?;
        let value = u16::from_be_bytes([bytes[0], bytes[1]]);
        self.cursor += 2;
        Ok(value)
    }

    /// Reads one network-order signed 64-bit scalar.
    ///
    /// # Errors
    ///
    /// Returns a truncation error without advancing when fewer than eight bytes remain.
    pub fn read_i64(&mut self) -> Result<i64, PacketCodecError> {
        let bytes = self.fixed(PacketField::I64, 8)?;
        let mut raw = [0_u8; 8];
        raw.copy_from_slice(bytes);
        self.cursor += 8;
        Ok(i64::from_be_bytes(raw))
    }

    /// Reads one network-order unsigned 64-bit scalar.
    ///
    /// # Errors
    ///
    /// Returns a truncation error without advancing when fewer than eight bytes remain.
    pub fn read_u64(&mut self) -> Result<u64, PacketCodecError> {
        let bytes = self.fixed(PacketField::U64, 8)?;
        let mut raw = [0_u8; 8];
        raw.copy_from_slice(bytes);
        self.cursor += 8;
        Ok(u64::from_be_bytes(raw))
    }

    /// Reads one canonical Minecraft boolean byte.
    ///
    /// # Errors
    ///
    /// Returns a truncation error for no remaining byte and [`PacketCodecError::InvalidBoolean`]
    /// for values other than `0` or `1`. Cursor position changes only on success.
    pub fn read_bool(&mut self) -> Result<bool, PacketCodecError> {
        let Some(&byte) = self.remaining().first() else {
            return Err(self.truncated(PacketField::Boolean));
        };
        let value = match byte {
            0 => false,
            1 => true,
            value => return Err(PacketCodecError::InvalidBoolean(value)),
        };
        self.cursor += 1;
        Ok(value)
    }

    /// Reads one Minecraft UTF-8 string with a caller-supplied UTF-16-unit bound.
    ///
    /// The returned string borrows directly from the packet payload.
    ///
    /// # Errors
    ///
    /// Returns wire validation errors or a packet truncation error. Cursor position changes only on
    /// success.
    pub fn read_string(&mut self, max_utf16_units: usize) -> Result<&'a str, PacketCodecError> {
        let remaining: &'a [u8] = self.remaining();
        let DecodeResult::Complete { value, consumed } = decode_string(remaining, max_utf16_units)?
        else {
            return Err(self.truncated(PacketField::String));
        };
        self.cursor += consumed;
        Ok(value)
    }

    /// Borrows every remaining payload byte and advances to the packet end.
    pub fn read_remaining(&mut self) -> &'a [u8] {
        let start = self.cursor;
        self.cursor = self.payload.len();
        &self.payload[start..]
    }

    /// Requires exact end-of-packet consumption.
    ///
    /// # Errors
    ///
    /// Returns [`PacketCodecError::TrailingBytes`] when unconsumed payload remains.
    pub const fn finish(&self) -> Result<(), PacketCodecError> {
        let remaining = self.remaining_len();
        if remaining == 0 {
            Ok(())
        } else {
            Err(PacketCodecError::TrailingBytes { remaining })
        }
    }

    fn remaining(&self) -> &'a [u8] {
        &self.payload[self.cursor..]
    }

    fn fixed(&self, field: PacketField, bytes: usize) -> Result<&'a [u8], PacketCodecError> {
        self.remaining()
            .get(..bytes)
            .ok_or_else(|| self.truncated(field))
    }

    const fn truncated(&self, field: PacketField) -> PacketCodecError {
        PacketCodecError::Truncated {
            field,
            remaining: self.remaining_len(),
        }
    }
}

/// Bounded owned packet-body builder.
///
/// Every field method checks the packet byte budget before mutating the output. Semantic field
/// rejection also leaves the output byte-for-byte unchanged.
#[derive(Debug)]
pub struct PacketWriter {
    bytes: Vec<u8>,
    maximum: usize,
}

impl PacketWriter {
    /// Creates an empty writer with a non-zero packet-body byte bound.
    ///
    /// # Errors
    ///
    /// Returns [`PacketCodecError::ZeroWriteLimit`] for a zero-byte bound.
    pub fn new(maximum: usize) -> Result<Self, PacketCodecError> {
        if maximum == 0 {
            return Err(PacketCodecError::ZeroWriteLimit);
        }
        Ok(Self {
            bytes: Vec::new(),
            maximum,
        })
    }

    /// Number of bytes currently encoded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether no field bytes have been encoded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Remaining packet byte budget.
    #[must_use]
    pub fn remaining_capacity(&self) -> usize {
        self.maximum - self.bytes.len()
    }

    /// Encoded packet body built so far.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    /// Consumes the writer and returns its encoded packet body.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Appends one signed Minecraft `VarInt`.
    ///
    /// # Errors
    ///
    /// Returns a packet-limit error before mutation when the encoded value would not fit.
    pub fn write_var_int(&mut self, value: i32) -> Result<(), PacketCodecError> {
        self.reserve_field(var_int_len(value))?;
        encode_var_int(value, &mut self.bytes);
        Ok(())
    }

    /// Appends one network-order unsigned 16-bit scalar.
    ///
    /// # Errors
    ///
    /// Returns a packet-limit error before mutation when two bytes would not fit.
    pub fn write_u16(&mut self, value: u16) -> Result<(), PacketCodecError> {
        self.reserve_field(2)?;
        self.bytes.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    /// Appends one network-order signed 64-bit scalar.
    ///
    /// # Errors
    ///
    /// Returns a packet-limit error before mutation when eight bytes would not fit.
    pub fn write_i64(&mut self, value: i64) -> Result<(), PacketCodecError> {
        self.reserve_field(8)?;
        self.bytes.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    /// Appends one network-order unsigned 64-bit scalar.
    ///
    /// # Errors
    ///
    /// Returns a packet-limit error before mutation when eight bytes would not fit.
    pub fn write_u64(&mut self, value: u64) -> Result<(), PacketCodecError> {
        self.reserve_field(8)?;
        self.bytes.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    /// Appends one canonical boolean byte.
    ///
    /// # Errors
    ///
    /// Returns a packet-limit error before mutation when one byte would not fit.
    pub fn write_bool(&mut self, value: bool) -> Result<(), PacketCodecError> {
        self.reserve_field(1)?;
        self.bytes.push(u8::from(value));
        Ok(())
    }

    /// Appends one Minecraft UTF-8 string under a UTF-16-unit bound.
    ///
    /// # Errors
    ///
    /// Returns packet-budget or underlying string-law errors. Both rejection paths leave the writer
    /// unchanged.
    pub fn write_string(
        &mut self,
        value: &str,
        max_utf16_units: usize,
    ) -> Result<(), PacketCodecError> {
        let signed_len = i32::try_from(value.len()).map_err(|_| {
            PacketCodecError::Wire(WireError::LengthDoesNotFitVarInt {
                length: value.len(),
            })
        })?;
        let encoded_len = var_int_len(signed_len)
            .checked_add(value.len())
            .ok_or(PacketCodecError::LengthOverflow)?;
        self.reserve_field(encoded_len)?;
        encode_string(value, max_utf16_units, &mut self.bytes)?;
        Ok(())
    }

    /// Appends caller-supplied raw packet bytes.
    ///
    /// # Errors
    ///
    /// Returns a packet-limit error before mutation when `value` does not fit.
    pub fn write_bytes(&mut self, value: &[u8]) -> Result<(), PacketCodecError> {
        self.reserve_field(value.len())?;
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    fn reserve_field(&self, additional: usize) -> Result<(), PacketCodecError> {
        let attempted = self
            .bytes
            .len()
            .checked_add(additional)
            .ok_or(PacketCodecError::LengthOverflow)?;
        if attempted > self.maximum {
            return Err(PacketCodecError::PacketLimitExceeded {
                attempted,
                maximum: self.maximum,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crucible_protocol_core::{WireError, encode_string, encode_var_int};

    use super::{PacketCodecError, PacketField, PacketReader, PacketWriter};

    #[test]
    fn fixed_width_fields_use_network_byte_order() {
        let payload = [
            0x12, 0x34,
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe,
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
            0x01,
        ];
        let mut reader = PacketReader::new(&payload);
        assert_eq!(reader.read_u16(), Ok(0x1234));
        assert_eq!(reader.read_i64(), Ok(-2));
        assert_eq!(reader.read_u64(), Ok(0x0123_4567_89ab_cdef));
        assert_eq!(reader.read_bool(), Ok(true));
        assert_eq!(reader.finish(), Ok(()));

        let mut writer = PacketWriter::new(payload.len()).expect("valid writer");
        writer.write_u16(0x1234).expect("u16");
        writer.write_i64(-2).expect("i64");
        writer.write_u64(0x0123_4567_89ab_cdef).expect("u64");
        writer.write_bool(true).expect("bool");
        assert_eq!(writer.as_slice(), payload);
    }

    #[test]
    fn signed_varints_roundtrip_through_packet_cursor() {
        for value in [0, 1, 127, 128, i32::MAX, -1, i32::MIN] {
            let mut encoded = Vec::new();
            encode_var_int(value, &mut encoded);
            let mut reader = PacketReader::new(&encoded);
            assert_eq!(reader.read_var_int(), Ok(value));
            assert_eq!(reader.finish(), Ok(()));

            let mut writer = PacketWriter::new(5).expect("valid writer");
            writer.write_var_int(value).expect("value fits");
            assert_eq!(writer.as_slice(), encoded);
        }
    }

    #[test]
    fn strings_borrow_and_keep_java_utf16_bounds() {
        let mut encoded = Vec::new();
        encode_string("A😀Z", 4, &mut encoded).expect("four UTF-16 units");
        let mut reader = PacketReader::new(&encoded);
        let value = reader.read_string(4).expect("valid bounded string");
        assert_eq!(value, "A😀Z");
        assert_eq!(reader.finish(), Ok(()));

        let mut writer = PacketWriter::new(encoded.len()).expect("exact writer");
        writer.write_string("A😀Z", 4).expect("valid string");
        assert_eq!(writer.as_slice(), encoded);

        let before = writer.as_slice().to_vec();
        assert!(matches!(
            writer.write_string("😀😀", 3),
            Err(
                PacketCodecError::PacketLimitExceeded { .. }
                    | PacketCodecError::Wire(WireError::StringLengthLimitExceeded { .. })
            )
        ));
        assert_eq!(writer.as_slice(), before);
    }

    #[test]
    fn malformed_boolean_is_rejected_without_cursor_advance() {
        let mut reader = PacketReader::new(&[2, 1]);
        assert_eq!(reader.read_bool(), Err(PacketCodecError::InvalidBoolean(2)));
        assert_eq!(reader.consumed_len(), 0);
        assert_eq!(reader.read_remaining(), &[2, 1]);
    }

    #[test]
    fn every_field_truncation_boundary_rolls_back_cursor() {
        let mut varint = Vec::new();
        encode_var_int(-1, &mut varint);
        for split in 0..varint.len() {
            let mut reader = PacketReader::new(&varint[..split]);
            assert!(matches!(
                reader.read_var_int(),
                Err(PacketCodecError::Truncated {
                    field: PacketField::VarInt,
                    ..
                })
            ));
            assert_eq!(reader.consumed_len(), 0);
        }

        for split in 0..2 {
            let bytes = vec![0; split];
            let mut reader = PacketReader::new(&bytes);
            assert!(matches!(
                reader.read_u16(),
                Err(PacketCodecError::Truncated {
                    field: PacketField::U16,
                    ..
                })
            ));
            assert_eq!(reader.consumed_len(), 0);
        }
        for split in 0..8 {
            let bytes = vec![0; split];
            let mut signed_reader = PacketReader::new(&bytes);
            assert!(matches!(
                signed_reader.read_i64(),
                Err(PacketCodecError::Truncated {
                    field: PacketField::I64,
                    ..
                })
            ));
            assert_eq!(signed_reader.consumed_len(), 0);

            let mut unsigned_reader = PacketReader::new(&bytes);
            assert!(matches!(
                unsigned_reader.read_u64(),
                Err(PacketCodecError::Truncated {
                    field: PacketField::U64,
                    ..
                })
            ));
            assert_eq!(unsigned_reader.consumed_len(), 0);
        }

        let mut string = Vec::new();
        encode_string("abc", 3, &mut string).expect("valid string");
        for split in 0..string.len() {
            let mut reader = PacketReader::new(&string[..split]);
            assert!(matches!(
                reader.read_string(3),
                Err(PacketCodecError::Truncated {
                    field: PacketField::String,
                    ..
                })
            ));
            assert_eq!(reader.consumed_len(), 0);
        }

        let mut bool_reader = PacketReader::new(&[]);
        assert!(matches!(
            bool_reader.read_bool(),
            Err(PacketCodecError::Truncated {
                field: PacketField::Boolean,
                ..
            })
        ));
        assert_eq!(bool_reader.consumed_len(), 0);
    }

    #[test]
    fn finish_rejects_trailing_bytes_exactly() {
        let mut reader = PacketReader::new(&[0, 7, 8]);
        assert_eq!(reader.read_bool(), Ok(false));
        assert_eq!(
            reader.finish(),
            Err(PacketCodecError::TrailingBytes { remaining: 2 })
        );
        assert_eq!(reader.read_remaining(), &[7, 8]);
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn writer_limit_rejection_is_transactional() {
        assert!(matches!(
            PacketWriter::new(0),
            Err(PacketCodecError::ZeroWriteLimit)
        ));
        let mut writer = PacketWriter::new(3).expect("valid writer");
        writer.write_u16(0x1234).expect("two bytes fit");
        let before = writer.as_slice().to_vec();
        assert_eq!(
            writer.write_u16(0x5678),
            Err(PacketCodecError::PacketLimitExceeded {
                attempted: 4,
                maximum: 3,
            })
        );
        assert_eq!(writer.as_slice(), before);
        writer.write_bool(true).expect("last byte fits");
        assert_eq!(writer.remaining_capacity(), 0);
    }

    #[test]
    fn long_mixed_field_corpus_roundtrips_exactly() {
        const RECORDS: usize = 20_000;
        let mut writer = PacketWriter::new(1_500_000).expect("bounded corpus writer");
        let mut state = 0xA5A5_1F3D_u32;
        let mut expected = Vec::with_capacity(RECORDS);

        for index in 0..RECORDS {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let varint = state.cast_signed();
            let short = u16::try_from(state & 0xffff).expect("masked value fits u16");
            let signed = i64::from(varint)
                .wrapping_mul(0x1_0001)
                .wrapping_add(i64::try_from(index).expect("bounded index"));
            let unsigned = signed.cast_unsigned().rotate_left(17);
            let boolean = index.is_multiple_of(2);
            writer.write_var_int(varint).expect("varint");
            writer.write_u16(short).expect("u16");
            writer.write_i64(signed).expect("i64");
            writer.write_u64(unsigned).expect("u64");
            writer.write_bool(boolean).expect("bool");
            writer.write_string("Cβ", 2).expect("string");
            expected.push((varint, short, signed, unsigned, boolean));
        }

        let bytes = writer.into_bytes();
        let mut reader = PacketReader::new(&bytes);
        for expected in expected {
            assert_eq!(reader.read_var_int(), Ok(expected.0));
            assert_eq!(reader.read_u16(), Ok(expected.1));
            assert_eq!(reader.read_i64(), Ok(expected.2));
            assert_eq!(reader.read_u64(), Ok(expected.3));
            assert_eq!(reader.read_bool(), Ok(expected.4));
            assert_eq!(reader.read_string(2), Ok("Cβ"));
        }
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn raw_remaining_bytes_are_borrowed_without_copy() {
        let payload = [0x01, 0xaa, 0xbb, 0xcc];
        let mut reader = PacketReader::new(&payload);
        assert_eq!(reader.read_bool(), Ok(true));
        let remaining = reader.read_remaining();
        assert_eq!(remaining, &[0xaa, 0xbb, 0xcc]);
        assert_eq!(remaining.as_ptr(), payload[1..].as_ptr());
        assert_eq!(reader.finish(), Ok(()));
    }
}
