//! Allocation-free packet-body field mechanics for Crucible's protocol spine.
//!
//! This crate operates on one already-complete packet payload. It contains no packet IDs,
//! connection states, target-version constants, authentication policy, or socket runtime. Readers
//! borrow directly from caller bytes; writers append transactionally into caller-owned storage.

#![forbid(unsafe_code)]

use crucible_protocol_core::{
    DecodeResult, WireError, decode_string, decode_var_int, encode_string, encode_var_int,
    var_int_len,
};

/// Field category used when classifying a truncated complete packet body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketField {
    /// Signed Minecraft `VarInt`.
    VarInt,
    /// Network-endian unsigned 16-bit integer.
    U16,
    /// Network-endian signed 64-bit integer.
    I64,
    /// Network-endian unsigned 64-bit integer.
    U64,
    /// Canonical one-byte boolean.
    Boolean,
    /// Minecraft length-prefixed UTF-8 string.
    String,
}

/// Fail-closed packet-body codec errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketBodyError {
    /// The outer packet was complete but this field did not have enough bytes.
    Truncated {
        field: PacketField,
        remaining: usize,
    },
    /// A boolean byte was neither zero nor one.
    InvalidBoolean(u8),
    /// Packet decoding finished with bytes that the packet law did not consume.
    TrailingBytes { remaining: usize },
    /// A writer operation would exceed its declared packet-payload byte budget.
    OutputLimitExceeded {
        written: usize,
        additional: usize,
        maximum: usize,
    },
    /// Integer arithmetic required to preserve a byte bound overflowed `usize`.
    LengthOverflow,
    /// The reusable Minecraft wire law rejected the value.
    Wire(WireError),
}

impl From<WireError> for PacketBodyError {
    fn from(value: WireError) -> Self {
        Self::Wire(value)
    }
}

/// Borrowed cursor over one complete packet payload.
///
/// A failed read leaves the cursor at its previous byte offset. `Incomplete` from the lower wire
/// decoder is converted to [`PacketBodyError::Truncated`] because no later TCP byte belongs to this
/// already-complete packet body.
#[derive(Clone, Copy, Debug)]
pub struct PacketReader<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> PacketReader<'a> {
    /// Creates a reader at the beginning of `input`.
    #[must_use]
    pub const fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    /// Number of payload bytes consumed so far.
    #[must_use]
    pub const fn consumed(&self) -> usize {
        self.offset
    }

    /// Number of payload bytes not yet consumed.
    #[must_use]
    pub fn remaining_len(&self) -> usize {
        self.input.len() - self.offset
    }

    /// Whether the packet payload has been consumed exactly.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.remaining_len() == 0
    }

    /// Reads one signed Minecraft `VarInt` transactionally.
    ///
    /// # Errors
    ///
    /// Returns a lower-level VarInt error or [`PacketBodyError::Truncated`] when the complete packet
    /// ends before the VarInt terminates.
    pub fn read_var_int(&mut self) -> Result<i32, PacketBodyError> {
        let remaining = self.remaining();
        match decode_var_int(remaining)? {
            DecodeResult::Incomplete => Err(PacketBodyError::Truncated {
                field: PacketField::VarInt,
                remaining: remaining.len(),
            }),
            DecodeResult::Complete { value, consumed } => {
                self.advance(consumed)?;
                Ok(value)
            }
        }
    }

    /// Reads one network-endian unsigned 16-bit integer.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::Truncated`] if fewer than two bytes remain.
    pub fn read_u16(&mut self) -> Result<u16, PacketBodyError> {
        let bytes = self.take_exact(2, PacketField::U16)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Reads one network-endian signed 64-bit integer.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::Truncated`] if fewer than eight bytes remain.
    pub fn read_i64(&mut self) -> Result<i64, PacketBodyError> {
        let bytes = self.take_exact(8, PacketField::I64)?;
        Ok(i64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads one network-endian unsigned 64-bit integer.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::Truncated`] if fewer than eight bytes remain.
    pub fn read_u64(&mut self) -> Result<u64, PacketBodyError> {
        let bytes = self.take_exact(8, PacketField::U64)?;
        Ok(u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads a canonical one-byte boolean (`0` or `1`).
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::Truncated`] when no byte remains and
    /// [`PacketBodyError::InvalidBoolean`] for any non-canonical byte. The cursor does not advance
    /// on either failure.
    pub fn read_bool(&mut self) -> Result<bool, PacketBodyError> {
        let Some(&byte) = self.remaining().first() else {
            return Err(PacketBodyError::Truncated {
                field: PacketField::Boolean,
                remaining: 0,
            });
        };
        let value = match byte {
            0 => false,
            1 => true,
            other => return Err(PacketBodyError::InvalidBoolean(other)),
        };
        self.advance(1)?;
        Ok(value)
    }

    /// Reads one borrowed Minecraft UTF-8 string with an explicit Java UTF-16-unit bound.
    ///
    /// # Errors
    ///
    /// Returns the lower reusable string-law error or [`PacketBodyError::Truncated`] when the
    /// already-complete packet ends during the field. The cursor advances only after success.
    pub fn read_string(&mut self, max_utf16_units: usize) -> Result<&'a str, PacketBodyError> {
        let remaining = self.remaining();
        match decode_string(remaining, max_utf16_units)? {
            DecodeResult::Incomplete => Err(PacketBodyError::Truncated {
                field: PacketField::String,
                remaining: remaining.len(),
            }),
            DecodeResult::Complete { value, consumed } => {
                self.advance(consumed)?;
                Ok(value)
            }
        }
    }

    /// Returns all unconsumed payload bytes and advances to the end.
    ///
    /// This is intended only for packet laws that explicitly define an opaque trailing byte field.
    #[must_use]
    pub fn take_remaining(&mut self) -> &'a [u8] {
        let value = self.remaining();
        self.offset = self.input.len();
        value
    }

    /// Requires exact end-of-packet consumption.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::TrailingBytes`] when bytes remain.
    pub fn finish(self) -> Result<(), PacketBodyError> {
        let remaining = self.remaining_len();
        if remaining == 0 {
            Ok(())
        } else {
            Err(PacketBodyError::TrailingBytes { remaining })
        }
    }

    fn remaining(&self) -> &'a [u8] {
        &self.input[self.offset..]
    }

    fn advance(&mut self, count: usize) -> Result<(), PacketBodyError> {
        let next = self
            .offset
            .checked_add(count)
            .ok_or(PacketBodyError::LengthOverflow)?;
        if next > self.input.len() {
            return Err(PacketBodyError::LengthOverflow);
        }
        self.offset = next;
        Ok(())
    }

    fn take_exact(
        &mut self,
        count: usize,
        field: PacketField,
    ) -> Result<&'a [u8], PacketBodyError> {
        let remaining = self.remaining_len();
        if remaining < count {
            return Err(PacketBodyError::Truncated { field, remaining });
        }
        let start = self.offset;
        let end = start
            .checked_add(count)
            .ok_or(PacketBodyError::LengthOverflow)?;
        let value = &self.input[start..end];
        self.offset = end;
        Ok(value)
    }
}

/// Bounded transactional writer for one packet payload.
///
/// The writer appends to caller-owned storage and treats `maximum` as the number of bytes this
/// writer may add after construction. Each field validates its complete encoded size before
/// mutating `output`.
#[derive(Debug)]
pub struct PacketWriter<'a> {
    output: &'a mut Vec<u8>,
    start: usize,
    maximum: usize,
}

impl<'a> PacketWriter<'a> {
    /// Creates a writer that may append at most `maximum` bytes.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::LengthOverflow`] when the absolute bound cannot be represented by
    /// `usize`.
    pub fn new(output: &'a mut Vec<u8>, maximum: usize) -> Result<Self, PacketBodyError> {
        output
            .len()
            .checked_add(maximum)
            .ok_or(PacketBodyError::LengthOverflow)?;
        let start = output.len();
        Ok(Self {
            output,
            start,
            maximum,
        })
    }

    /// Number of payload bytes appended through this writer.
    #[must_use]
    pub fn written_len(&self) -> usize {
        self.output.len() - self.start
    }

    /// Bytes appended through this writer so far.
    #[must_use]
    pub fn written(&self) -> &[u8] {
        &self.output[self.start..]
    }

    /// Appends one signed Minecraft `VarInt`.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::OutputLimitExceeded`] before mutation when it would exceed the
    /// writer budget.
    pub fn write_var_int(&mut self, value: i32) -> Result<(), PacketBodyError> {
        self.reserve_field(var_int_len(value))?;
        encode_var_int(value, self.output);
        Ok(())
    }

    /// Appends one network-endian unsigned 16-bit integer.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::OutputLimitExceeded`] before mutation when needed.
    pub fn write_u16(&mut self, value: u16) -> Result<(), PacketBodyError> {
        self.reserve_field(2)?;
        self.output.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    /// Appends one network-endian signed 64-bit integer.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::OutputLimitExceeded`] before mutation when needed.
    pub fn write_i64(&mut self, value: i64) -> Result<(), PacketBodyError> {
        self.reserve_field(8)?;
        self.output.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    /// Appends one network-endian unsigned 64-bit integer.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::OutputLimitExceeded`] before mutation when needed.
    pub fn write_u64(&mut self, value: u64) -> Result<(), PacketBodyError> {
        self.reserve_field(8)?;
        self.output.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    /// Appends one canonical one-byte boolean.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::OutputLimitExceeded`] before mutation when needed.
    pub fn write_bool(&mut self, value: bool) -> Result<(), PacketBodyError> {
        self.reserve_field(1)?;
        self.output.push(u8::from(value));
        Ok(())
    }

    /// Appends one Minecraft UTF-8 string with an explicit Java UTF-16-unit bound.
    ///
    /// # Errors
    ///
    /// Returns the reusable wire-law error or [`PacketBodyError::OutputLimitExceeded`] before output
    /// mutation.
    pub fn write_string(
        &mut self,
        value: &str,
        max_utf16_units: usize,
    ) -> Result<(), PacketBodyError> {
        let encoded_len = encoded_string_len(value, max_utf16_units)?;
        self.reserve_field(encoded_len)?;
        encode_string(value, max_utf16_units, self.output)?;
        Ok(())
    }

    /// Appends an opaque raw field.
    ///
    /// # Errors
    ///
    /// Returns [`PacketBodyError::OutputLimitExceeded`] before mutation when needed.
    pub fn write_bytes(&mut self, value: &[u8]) -> Result<(), PacketBodyError> {
        self.reserve_field(value.len())?;
        self.output.extend_from_slice(value);
        Ok(())
    }

    fn reserve_field(&self, additional: usize) -> Result<(), PacketBodyError> {
        let written = self.written_len();
        let required = written
            .checked_add(additional)
            .ok_or(PacketBodyError::LengthOverflow)?;
        if required > self.maximum {
            return Err(PacketBodyError::OutputLimitExceeded {
                written,
                additional,
                maximum: self.maximum,
            });
        }
        Ok(())
    }
}

fn encoded_string_len(value: &str, max_utf16_units: usize) -> Result<usize, PacketBodyError> {
    let utf16_units = value.encode_utf16().count();
    if utf16_units > max_utf16_units {
        return Err(WireError::StringLengthLimitExceeded {
            utf16_units,
            max: max_utf16_units,
        }
        .into());
    }
    let max_bytes = max_utf16_units
        .checked_mul(3)
        .ok_or(WireError::LengthOverflow)?;
    if value.len() > max_bytes {
        return Err(WireError::ByteLengthLimitExceeded {
            length: value.len(),
            max: max_bytes,
        }
        .into());
    }
    let signed_len = i32::try_from(value.len()).map_err(|_| WireError::LengthDoesNotFitVarInt {
        length: value.len(),
    })?;
    var_int_len(signed_len)
        .checked_add(value.len())
        .ok_or(PacketBodyError::LengthOverflow)
}

#[cfg(test)]
mod tests {
    use super::{PacketBodyError, PacketField, PacketReader, PacketWriter};
    use crucible_protocol_core::WireError;

    #[test]
    fn fixed_width_values_are_network_endian_and_exact() {
        let bytes = [
            0x12, 0x34, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe, 0x01, 0x23, 0x45,
            0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x00,
        ];
        let mut reader = PacketReader::new(&bytes);
        assert_eq!(reader.read_u16(), Ok(0x1234));
        assert_eq!(reader.read_i64(), Ok(-2));
        assert_eq!(reader.read_u64(), Ok(0x0123_4567_89ab_cdef));
        assert_eq!(reader.read_bool(), Ok(true));
        assert_eq!(reader.read_bool(), Ok(false));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn writer_and_reader_roundtrip_all_generic_fields() {
        let mut output = vec![0xaa];
        {
            let mut writer = PacketWriter::new(&mut output, 128).expect("valid writer");
            writer.write_var_int(-12_345).expect("varint");
            writer.write_u16(25_565).expect("port");
            writer.write_i64(-9_876_543_210).expect("i64");
            writer
                .write_u64(0xfedc_ba98_7654_3210)
                .expect("u64");
            writer.write_bool(true).expect("bool");
            writer.write_string("Crucible 🔥", 32).expect("string");
            writer.write_bytes(&[7, 8, 9]).expect("raw");
            assert_eq!(writer.written(), &output[1..]);
        }

        let mut reader = PacketReader::new(&output[1..]);
        assert_eq!(reader.read_var_int(), Ok(-12_345));
        assert_eq!(reader.read_u16(), Ok(25_565));
        assert_eq!(reader.read_i64(), Ok(-9_876_543_210));
        assert_eq!(reader.read_u64(), Ok(0xfedc_ba98_7654_3210));
        assert_eq!(reader.read_bool(), Ok(true));
        assert_eq!(reader.read_string(32), Ok("Crucible 🔥"));
        assert_eq!(reader.take_remaining(), [7, 8, 9]);
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn truncated_fixed_fields_do_not_advance() {
        for (field, width) in [
            (PacketField::U16, 2_usize),
            (PacketField::I64, 8),
            (PacketField::U64, 8),
        ] {
            for available in 0..width {
                let bytes = vec![0; available];
                let mut reader = PacketReader::new(&bytes);
                let result = match field {
                    PacketField::U16 => reader.read_u16().map(u64::from),
                    PacketField::I64 => reader.read_i64().map(|value| value.cast_unsigned()),
                    PacketField::U64 => reader.read_u64(),
                    _ => unreachable!("fixed-field test only"),
                };
                assert_eq!(
                    result,
                    Err(PacketBodyError::Truncated {
                        field,
                        remaining: available
                    })
                );
                assert_eq!(reader.consumed(), 0);
            }
        }
    }

    #[test]
    fn truncated_and_invalid_varints_do_not_advance() {
        let mut truncated = PacketReader::new(&[0x80, 0x80]);
        assert_eq!(
            truncated.read_var_int(),
            Err(PacketBodyError::Truncated {
                field: PacketField::VarInt,
                remaining: 2
            })
        );
        assert_eq!(truncated.consumed(), 0);

        let mut invalid = PacketReader::new(&[0x80, 0x80, 0x80, 0x80, 0x80]);
        assert_eq!(
            invalid.read_var_int(),
            Err(PacketBodyError::Wire(WireError::VarIntTooLong))
        );
        assert_eq!(invalid.consumed(), 0);
    }

    #[test]
    fn string_truncation_and_validation_leave_cursor_unchanged() {
        let mut truncated = PacketReader::new(&[0x04, b'a', b'b']);
        assert_eq!(
            truncated.read_string(8),
            Err(PacketBodyError::Truncated {
                field: PacketField::String,
                remaining: 3
            })
        );
        assert_eq!(truncated.consumed(), 0);

        let mut invalid = PacketReader::new(&[0x02, 0xff, 0xff]);
        assert_eq!(
            invalid.read_string(8),
            Err(PacketBodyError::Wire(WireError::InvalidUtf8))
        );
        assert_eq!(invalid.consumed(), 0);
    }

    #[test]
    fn boolean_is_canonical_and_transactional() {
        let mut empty = PacketReader::new(&[]);
        assert_eq!(
            empty.read_bool(),
            Err(PacketBodyError::Truncated {
                field: PacketField::Boolean,
                remaining: 0
            })
        );
        assert_eq!(empty.consumed(), 0);

        let mut invalid = PacketReader::new(&[2, 1]);
        assert_eq!(invalid.read_bool(), Err(PacketBodyError::InvalidBoolean(2)));
        assert_eq!(invalid.consumed(), 0);
        assert_eq!(invalid.take_remaining(), [2, 1]);
    }

    #[test]
    fn finish_rejects_unclaimed_trailing_bytes() {
        let mut reader = PacketReader::new(&[0x01, 0xaa]);
        assert_eq!(reader.read_bool(), Ok(true));
        assert_eq!(
            reader.finish(),
            Err(PacketBodyError::TrailingBytes { remaining: 1 })
        );
    }

    #[test]
    fn writer_bound_is_checked_before_every_mutation() {
        let mut output = vec![9, 9];
        let mut writer = PacketWriter::new(&mut output, 3).expect("writer");
        writer.write_u16(0x1234).expect("two bytes");
        assert_eq!(writer.written(), [0x12, 0x34]);
        assert_eq!(
            writer.write_u16(0x5678),
            Err(PacketBodyError::OutputLimitExceeded {
                written: 2,
                additional: 2,
                maximum: 3
            })
        );
        assert_eq!(writer.written(), [0x12, 0x34]);
        writer.write_bool(true).expect("final byte");
        assert_eq!(writer.written(), [0x12, 0x34, 0x01]);
    }

    #[test]
    fn rejected_string_does_not_mutate_writer_output() {
        let mut output = vec![0xaa];
        let mut writer = PacketWriter::new(&mut output, 64).expect("writer");
        writer.write_var_int(7).expect("prefix field");
        let before = writer.written().to_vec();
        assert_eq!(
            writer.write_string("🔥", 1),
            Err(PacketBodyError::Wire(
                WireError::StringLengthLimitExceeded {
                    utf16_units: 2,
                    max: 1
                }
            ))
        );
        assert_eq!(writer.written(), before);
    }

    #[test]
    fn long_deterministic_mixed_field_trace_roundtrips() {
        const RECORDS: usize = 10_000;
        let mut output = Vec::new();
        {
            let mut writer = PacketWriter::new(&mut output, 512_000).expect("writer");
            let mut state = 0x9e37_79b9_u32;
            for index in 0..RECORDS {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let signed = state.cast_signed();
                writer.write_var_int(signed).expect("varint");
                writer
                    .write_u16((state & 0xffff).try_into().expect("masked u16"))
                    .expect("u16");
                writer
                    .write_i64(i64::from(signed).wrapping_mul(31))
                    .expect("i64");
                writer
                    .write_bool(index.is_multiple_of(2))
                    .expect("bool");
                writer.write_string("Crucible", 16).expect("string");
            }
        }

        let mut reader = PacketReader::new(&output);
        let mut state = 0x9e37_79b9_u32;
        for index in 0..RECORDS {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let signed = state.cast_signed();
            assert_eq!(reader.read_var_int(), Ok(signed));
            assert_eq!(reader.read_u16(), Ok((state & 0xffff) as u16));
            assert_eq!(reader.read_i64(), Ok(i64::from(signed).wrapping_mul(31)));
            assert_eq!(reader.read_bool(), Ok(index.is_multiple_of(2)));
            assert_eq!(reader.read_string(16), Ok("Crucible"));
        }
        assert_eq!(reader.finish(), Ok(()));
    }
}
