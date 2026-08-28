//! Bounded zero-copy Java NBT cursor for cold world import.
//!
//! The cursor intentionally exposes schema-directed primitives rather than a generic object tree.
//! Callers consume admitted fields and skip irrelevant payloads under explicit length/depth limits.

use core::str;

const HARD_MAX_DEPTH: usize = 64;

/// Standard Java NBT tag identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TagType {
    /// Compound/list terminator; it has no payload.
    End = 0,
    /// Signed 8-bit integer.
    Byte = 1,
    /// Signed 16-bit integer.
    Short = 2,
    /// Signed 32-bit integer.
    Int = 3,
    /// Signed 64-bit integer.
    Long = 4,
    /// IEEE-754 32-bit float.
    Float = 5,
    /// IEEE-754 64-bit float.
    Double = 6,
    /// Length-prefixed raw byte array.
    ByteArray = 7,
    /// Length-prefixed UTF-8 string.
    String = 8,
    /// Homogeneous list.
    List = 9,
    /// Named-tag compound.
    Compound = 10,
    /// Signed 32-bit integer array.
    IntArray = 11,
    /// Signed 64-bit integer array.
    LongArray = 12,
}

impl TryFrom<u8> for TagType {
    type Error = NbtError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::End),
            1 => Ok(Self::Byte),
            2 => Ok(Self::Short),
            3 => Ok(Self::Int),
            4 => Ok(Self::Long),
            5 => Ok(Self::Float),
            6 => Ok(Self::Double),
            7 => Ok(Self::ByteArray),
            8 => Ok(Self::String),
            9 => Ok(Self::List),
            10 => Ok(Self::Compound),
            11 => Ok(Self::IntArray),
            12 => Ok(Self::LongArray),
            id => Err(NbtError::InvalidTagType { id }),
        }
    }
}

/// Explicit NBT resource bounds for one import profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NbtLimits {
    /// Maximum UTF-8 byte length of one NBT string/name.
    pub max_string_bytes: usize,
    /// Maximum element count of one list.
    pub max_list_elements: usize,
    /// Maximum element count of one byte/int/long array.
    pub max_array_elements: usize,
    /// Maximum recursively skipped container depth.
    pub max_depth: usize,
}

impl NbtLimits {
    /// Creates explicit limits, rejecting a stack-risking depth policy.
    ///
    /// # Errors
    ///
    /// Returns an error when `max_depth` is zero or exceeds the parser's hard recursion ceiling.
    pub fn new(
        max_string_bytes: usize,
        max_list_elements: usize,
        max_array_elements: usize,
        max_depth: usize,
    ) -> Result<Self, NbtError> {
        if max_depth == 0 || max_depth > HARD_MAX_DEPTH {
            return Err(NbtError::InvalidDepthLimit {
                requested: max_depth,
                hard_max: HARD_MAX_DEPTH,
            });
        }
        Ok(Self {
            max_string_bytes,
            max_list_elements,
            max_array_elements,
            max_depth,
        })
    }
}

/// One named compound field. The payload begins at the reader's current offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NamedTag<'a> {
    /// Field payload type.
    pub tag_type: TagType,
    /// Borrowed UTF-8 field name.
    pub name: &'a str,
}

/// Homogeneous list framing. Element payloads follow immediately.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListHeader {
    /// Element payload type. `End` is legal only for an empty list.
    pub element_type: TagType,
    /// Number of element payloads following the header.
    pub len: usize,
}

/// Fail-closed bounded NBT parse errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NbtError {
    /// Parser needed more bytes than remain in the supplied decompressed payload.
    UnexpectedEof { offset: usize, needed: usize },
    /// Encountered a tag ID outside the Java NBT set.
    InvalidTagType { id: u8 },
    /// Java chunk NBT must begin with a named compound root.
    RootNotCompound { actual: TagType },
    /// NBT name/string bytes are not valid UTF-8.
    InvalidUtf8,
    /// Signed list/array length was negative.
    NegativeLength { kind: &'static str, value: i32 },
    /// One explicit import bound was exceeded.
    LimitExceeded {
        kind: &'static str,
        actual: usize,
        limit: usize,
    },
    /// Configured parser depth would risk unbounded recursion.
    InvalidDepthLimit { requested: usize, hard_max: usize },
    /// Nested container exceeded the configured import depth.
    DepthLimitExceeded { depth: usize, limit: usize },
    /// A non-empty list cannot use `TAG_End` as its element type.
    NonEmptyEndList { len: usize },
    /// `TAG_End` cannot be consumed as a standalone payload.
    EndHasNoPayload,
    /// Length multiplication/addition could not be represented safely.
    ArithmeticOverflow,
    /// Bytes remain after the root compound terminator.
    TrailingBytes { remaining: usize },
}

/// Zero-copy cursor over one already-decompressed NBT payload.
#[derive(Clone, Debug)]
pub struct NbtReader<'a> {
    data: &'a [u8],
    offset: usize,
    limits: NbtLimits,
}

impl<'a> NbtReader<'a> {
    /// Starts a cursor over one bounded decompressed payload.
    #[must_use]
    pub const fn new(data: &'a [u8], limits: NbtLimits) -> Self {
        Self {
            data,
            offset: 0,
            limits,
        }
    }

    /// Current byte offset, useful for cold diagnostic evidence.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Consumes the root tag header and returns its borrowed name.
    ///
    /// # Errors
    ///
    /// Returns an error for truncation, invalid tag identity/type, invalid UTF-8, or string limits.
    pub fn begin_root_compound(&mut self) -> Result<&'a str, NbtError> {
        let actual = TagType::try_from(self.read_u8()?)?;
        if actual != TagType::Compound {
            return Err(NbtError::RootNotCompound { actual });
        }
        self.read_string()
    }

    /// Consumes the next named field header inside a compound.
    ///
    /// `Ok(None)` means the compound's `TAG_End` terminator was consumed. The payload of a returned
    /// field remains unread so schema code can decode or skip it directly.
    pub fn next_compound_field(&mut self) -> Result<Option<NamedTag<'a>>, NbtError> {
        let tag_type = TagType::try_from(self.read_u8()?)?;
        if tag_type == TagType::End {
            return Ok(None);
        }
        let name = self.read_string()?;
        Ok(Some(NamedTag { tag_type, name }))
    }

    /// Requires the root compound to consume the complete payload.
    ///
    /// # Errors
    ///
    /// Returns an error when trailing bytes remain.
    pub fn finish_root(&self) -> Result<(), NbtError> {
        if self.offset == self.data.len() {
            Ok(())
        } else {
            Err(NbtError::TrailingBytes {
                remaining: self.data.len() - self.offset,
            })
        }
    }

    /// Reads one signed byte payload.
    pub fn read_i8(&mut self) -> Result<i8, NbtError> {
        Ok(i8::from_be_bytes([self.read_u8()?]))
    }

    /// Reads one signed short payload.
    pub fn read_i16(&mut self) -> Result<i16, NbtError> {
        Ok(i16::from_be_bytes(self.take_array()?))
    }

    /// Reads one signed integer payload.
    pub fn read_i32(&mut self) -> Result<i32, NbtError> {
        Ok(i32::from_be_bytes(self.take_array()?))
    }

    /// Reads one signed long payload.
    pub fn read_i64(&mut self) -> Result<i64, NbtError> {
        Ok(i64::from_be_bytes(self.take_array()?))
    }

    /// Reads one float payload.
    pub fn read_f32(&mut self) -> Result<f32, NbtError> {
        Ok(f32::from_bits(u32::from_be_bytes(self.take_array()?)))
    }

    /// Reads one double payload.
    pub fn read_f64(&mut self) -> Result<f64, NbtError> {
        Ok(f64::from_bits(u64::from_be_bytes(self.take_array()?)))
    }

    /// Reads one borrowed NBT string payload.
    pub fn read_string(&mut self) -> Result<&'a str, NbtError> {
        let length = usize::from(u16::from_be_bytes(self.take_array()?));
        self.check_limit("string bytes", length, self.limits.max_string_bytes)?;
        let bytes = self.take(length)?;
        str::from_utf8(bytes).map_err(|_| NbtError::InvalidUtf8)
    }

    /// Reads one borrowed byte-array payload without copying it.
    pub fn read_byte_array(&mut self) -> Result<&'a [u8], NbtError> {
        let length = self.read_bounded_length("byte array", self.limits.max_array_elements)?;
        self.take(length)
    }

    /// Reads homogeneous-list framing. Element payloads remain unread.
    pub fn read_list_header(&mut self) -> Result<ListHeader, NbtError> {
        let element_type = TagType::try_from(self.read_u8()?)?;
        let len = self.read_bounded_length("list", self.limits.max_list_elements)?;
        if element_type == TagType::End && len != 0 {
            return Err(NbtError::NonEmptyEndList { len });
        }
        Ok(ListHeader { element_type, len })
    }

    /// Reads an int-array length. Elements remain unread for schema-directed consumption.
    pub fn read_int_array_len(&mut self) -> Result<usize, NbtError> {
        self.read_bounded_length("int array", self.limits.max_array_elements)
    }

    /// Reads a long-array length. Elements remain unread for schema-directed consumption.
    pub fn read_long_array_len(&mut self) -> Result<usize, NbtError> {
        self.read_bounded_length("long array", self.limits.max_array_elements)
    }

    /// Skips exactly one payload without allocating a generic NBT value.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed/truncated nested payloads or any configured resource limit.
    pub fn skip_payload(&mut self, tag_type: TagType) -> Result<(), NbtError> {
        self.skip_payload_at(tag_type, 0)
    }

    fn skip_payload_at(&mut self, tag_type: TagType, depth: usize) -> Result<(), NbtError> {
        match tag_type {
            TagType::End => Err(NbtError::EndHasNoPayload),
            TagType::Byte => self.advance(1),
            TagType::Short => self.advance(2),
            TagType::Int | TagType::Float => self.advance(4),
            TagType::Long | TagType::Double => self.advance(8),
            TagType::ByteArray => {
                let length =
                    self.read_bounded_length("byte array", self.limits.max_array_elements)?;
                self.advance(length)
            }
            TagType::String => self.read_string().map(|_| ()),
            TagType::List => {
                let nested = self.enter_depth(depth)?;
                let header = self.read_list_header()?;
                for _ in 0..header.len {
                    self.skip_payload_at(header.element_type, nested)?;
                }
                Ok(())
            }
            TagType::Compound => {
                let nested = self.enter_depth(depth)?;
                while let Some(field) = self.next_compound_field()? {
                    self.skip_payload_at(field.tag_type, nested)?;
                }
                Ok(())
            }
            TagType::IntArray => {
                let length = self.read_int_array_len()?;
                let bytes = length.checked_mul(4).ok_or(NbtError::ArithmeticOverflow)?;
                self.advance(bytes)
            }
            TagType::LongArray => {
                let length = self.read_long_array_len()?;
                let bytes = length.checked_mul(8).ok_or(NbtError::ArithmeticOverflow)?;
                self.advance(bytes)
            }
        }
    }

    fn enter_depth(&self, depth: usize) -> Result<usize, NbtError> {
        let nested = depth.checked_add(1).ok_or(NbtError::ArithmeticOverflow)?;
        if nested > self.limits.max_depth {
            return Err(NbtError::DepthLimitExceeded {
                depth: nested,
                limit: self.limits.max_depth,
            });
        }
        Ok(nested)
    }

    fn read_bounded_length(
        &mut self,
        kind: &'static str,
        limit: usize,
    ) -> Result<usize, NbtError> {
        let value = self.read_i32()?;
        if value < 0 {
            return Err(NbtError::NegativeLength { kind, value });
        }
        let length = usize::try_from(value).map_err(|_| NbtError::ArithmeticOverflow)?;
        self.check_limit(kind, length, limit)?;
        Ok(length)
    }

    fn check_limit(
        &self,
        kind: &'static str,
        actual: usize,
        limit: usize,
    ) -> Result<(), NbtError> {
        if actual > limit {
            Err(NbtError::LimitExceeded {
                kind,
                actual,
                limit,
            })
        } else {
            Ok(())
        }
    }

    fn read_u8(&mut self) -> Result<u8, NbtError> {
        Ok(self.take(1)?[0])
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], NbtError> {
        let bytes = self.take(N)?;
        let mut array = [0_u8; N];
        array.copy_from_slice(bytes);
        Ok(array)
    }

    fn advance(&mut self, size: usize) -> Result<(), NbtError> {
        self.take(size).map(|_| ())
    }

    fn take(&mut self, size: usize) -> Result<&'a [u8], NbtError> {
        let end = self
            .offset
            .checked_add(size)
            .ok_or(NbtError::ArithmeticOverflow)?;
        if end > self.data.len() {
            return Err(NbtError::UnexpectedEof {
                offset: self.offset,
                needed: size,
            });
        }
        let value = &self.data[self.offset..end];
        self.offset = end;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{NbtError, NbtLimits, NbtReader, TagType};

    fn limits() -> NbtLimits {
        NbtLimits::new(128, 64, 64, 8).expect("valid test limits")
    }

    #[test]
    fn reads_schema_directed_root_without_allocating_tree() {
        let mut bytes = vec![10, 0, 0, 3, 0, 11];
        bytes.extend_from_slice(b"DataVersion");
        bytes.extend_from_slice(&4903_i32.to_be_bytes());
        bytes.push(0);

        let mut reader = NbtReader::new(&bytes, limits());
        assert_eq!(reader.begin_root_compound().expect("root"), "");
        let field = reader
            .next_compound_field()
            .expect("field header")
            .expect("field");
        assert_eq!(field.name, "DataVersion");
        assert_eq!(field.tag_type, TagType::Int);
        assert_eq!(reader.read_i32().expect("int payload"), 4903);
        assert_eq!(reader.next_compound_field().expect("end"), None);
        assert_eq!(reader.finish_root(), Ok(()));
    }

    #[test]
    fn skips_nested_compound_and_list_under_limits() {
        let bytes = [
            10, 0, 0, // root
            10, 0, 1, b'x', // compound x
            9, 0, 1, b'l', // list l
            3, 0, 0, 0, 2, // int list, len 2
            0, 0, 0, 7, 0, 0, 0, 8, // two ints
            0, // end x
            3, 0, 1, b'v', 0, 0, 0, 9, // root int v
            0, // end root
        ];
        let mut reader = NbtReader::new(&bytes, limits());
        reader.begin_root_compound().expect("root");
        let nested = reader
            .next_compound_field()
            .expect("nested header")
            .expect("nested");
        assert_eq!(nested.tag_type, TagType::Compound);
        reader.skip_payload(nested.tag_type).expect("nested skip");
        let value = reader
            .next_compound_field()
            .expect("value header")
            .expect("value");
        assert_eq!(value.name, "v");
        assert_eq!(reader.read_i32().expect("value"), 9);
        assert_eq!(reader.next_compound_field().expect("root end"), None);
        assert_eq!(reader.finish_root(), Ok(()));
    }

    #[test]
    fn negative_and_oversized_lengths_fail_closed() {
        let negative = [0, 0, 0, 0xff, 0xff, 0xff, 0xff];
        let mut reader = NbtReader::new(&negative, limits());
        assert_eq!(
            reader.read_list_header(),
            Err(NbtError::NegativeLength {
                kind: "list",
                value: -1,
            })
        );

        let oversized = [3, 0, 0, 0, 65];
        let mut reader = NbtReader::new(&oversized, limits());
        assert_eq!(
            reader.read_list_header(),
            Err(NbtError::LimitExceeded {
                kind: "list",
                actual: 65,
                limit: 64,
            })
        );
    }

    #[test]
    fn non_empty_end_list_is_rejected() {
        let bytes = [0, 0, 0, 0, 1];
        let mut reader = NbtReader::new(&bytes, limits());
        assert_eq!(
            reader.read_list_header(),
            Err(NbtError::NonEmptyEndList { len: 1 })
        );
    }

    #[test]
    fn configured_depth_is_enforced_during_skip() {
        let bytes = [10, 0, 0, 10, 0, 0, 0, 0];
        let shallow = NbtLimits::new(128, 64, 64, 1).expect("valid shallow limit");
        let mut reader = NbtReader::new(&bytes, shallow);
        let first = reader.skip_payload(TagType::Compound);
        assert_eq!(
            first,
            Err(NbtError::DepthLimitExceeded { depth: 2, limit: 1 })
        );
    }

    #[test]
    fn truncation_and_trailing_bytes_are_visible() {
        let mut reader = NbtReader::new(&[10, 0], limits());
        assert!(matches!(
            reader.begin_root_compound(),
            Err(NbtError::UnexpectedEof { .. })
        ));

        let bytes = [10, 0, 0, 0, 99];
        let mut reader = NbtReader::new(&bytes, limits());
        reader.begin_root_compound().expect("root");
        assert_eq!(reader.next_compound_field().expect("end"), None);
        assert_eq!(reader.finish_root(), Err(NbtError::TrailingBytes { remaining: 1 }));
    }

    #[test]
    fn depth_configuration_has_hard_stack_ceiling() {
        assert_eq!(
            NbtLimits::new(1, 1, 1, 0),
            Err(NbtError::InvalidDepthLimit {
                requested: 0,
                hard_max: 64,
            })
        );
        assert_eq!(
            NbtLimits::new(1, 1, 1, 65),
            Err(NbtError::InvalidDepthLimit {
                requested: 65,
                hard_max: 64,
            })
        );
    }
}
