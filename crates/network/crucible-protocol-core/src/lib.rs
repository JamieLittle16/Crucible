//! Allocation-conscious wire primitives for Crucible's Minecraft protocol spine.
//!
//! This crate contains no target-version packet IDs or connection-state transitions. It owns the
//! reusable byte-level laws that sit below versioned packet semantics.

#![forbid(unsafe_code)]

use core::str;

/// Maximum encoded width of a Minecraft signed 32-bit `VarInt`.
pub const MAX_VAR_INT_BYTES: usize = 5;
/// Maximum encoded width of Minecraft's outer packet-length `VarInt21`.
pub const MAX_FRAME_LENGTH_BYTES: usize = 3;
/// Largest packet body accepted by the vanilla `VarInt21` framing layer.
pub const MAX_FRAME_BODY_LEN: usize = (1_usize << 21) - 1;

/// Fail-closed wire errors. Incomplete input is represented separately by [`DecodeResult`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireError {
    /// A `VarInt` still carried its continuation bit after five bytes.
    VarIntTooLong,
    /// A frame length still carried its continuation bit after three bytes.
    FrameLengthTooWide,
    /// Vanilla's remote framing layer rejects a zero-byte packet body.
    ZeroLengthFrame,
    /// A length prefix decoded to a negative signed value.
    NegativeLength(i32),
    /// A decoded or encoded byte length exceeded the applicable explicit limit.
    ByteLengthLimitExceeded { length: usize, max: usize },
    /// A decoded or encoded UTF-8 string exceeded its UTF-16 code-unit limit.
    StringLengthLimitExceeded { utf16_units: usize, max: usize },
    /// A byte length cannot be represented by the signed 32-bit `VarInt` wire format.
    LengthDoesNotFitVarInt { length: usize },
    /// A complete length-prefixed string contained invalid UTF-8.
    InvalidUtf8,
    /// Integer arithmetic required to identify a complete value overflowed `usize`.
    LengthOverflow,
}

/// Result of parsing from a potentially fragmented byte stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeResult<T> {
    /// More bytes are required before the value can be classified as valid or invalid.
    Incomplete,
    /// A complete value and the exact number of input bytes consumed.
    Complete { value: T, consumed: usize },
}

/// Encodes one signed 32-bit value using Minecraft's `VarInt` representation.
///
/// The output vector is appended to; existing bytes are preserved.
pub fn encode_var_int(value: i32, output: &mut Vec<u8>) {
    let mut remaining = value.cast_unsigned();
    loop {
        if remaining & !0x7f == 0 {
            output.push(remaining.to_le_bytes()[0]);
            return;
        }
        output.push(((remaining & 0x7f) | 0x80).to_le_bytes()[0]);
        remaining >>= 7;
    }
}

/// Returns the encoded width of a signed 32-bit Minecraft `VarInt`.
#[must_use]
pub const fn var_int_len(value: i32) -> usize {
    let mut remaining = value.cast_unsigned();
    let mut bytes = 1;
    while remaining & !0x7f != 0 {
        bytes += 1;
        remaining >>= 7;
    }
    bytes
}

/// Decodes one signed 32-bit Minecraft `VarInt` from the beginning of `input`.
///
/// # Errors
///
/// Returns [`WireError::VarIntTooLong`] once five continuation bytes have been observed. A short
/// prefix that could still become valid returns [`DecodeResult::Incomplete`] instead.
#[must_use]
pub fn decode_var_int(input: &[u8]) -> Result<DecodeResult<i32>, WireError> {
    let mut value = 0_u32;
    let mut index = 0_usize;
    while index < MAX_VAR_INT_BYTES {
        let Some(&byte) = input.get(index) else {
            return Ok(DecodeResult::Incomplete);
        };
        value |= u32::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Ok(DecodeResult::Complete {
                value: value.cast_signed(),
                consumed: index + 1,
            });
        }
        index += 1;
    }
    Err(WireError::VarIntTooLong)
}

/// Decodes Minecraft's outer `VarInt21` packet frame without allocating or copying its body.
///
/// The frame body includes the packet ID and packet payload. Vanilla accepts at most three bytes
/// for this length prefix and rejects a zero-length body before packet decoding.
///
/// # Errors
///
/// Returns an error for a three-byte continuation prefix, a zero frame, a length above either the
/// vanilla `VarInt21` ceiling or `max_body_len`, or arithmetic overflow. A valid fragmented frame
/// returns [`DecodeResult::Incomplete`].
#[must_use]
pub fn decode_frame(input: &[u8], max_body_len: usize) -> Result<DecodeResult<&[u8]>, WireError> {
    let mut body_len = 0_usize;
    for index in 0..MAX_FRAME_LENGTH_BYTES {
        let Some(&byte) = input.get(index) else {
            return Ok(DecodeResult::Incomplete);
        };
        body_len |= usize::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            if body_len == 0 {
                return Err(WireError::ZeroLengthFrame);
            }
            let max = max_body_len.min(MAX_FRAME_BODY_LEN);
            if body_len > max {
                return Err(WireError::ByteLengthLimitExceeded {
                    length: body_len,
                    max,
                });
            }
            let header_len = index + 1;
            let frame_end = header_len
                .checked_add(body_len)
                .ok_or(WireError::LengthOverflow)?;
            let Some(body) = input.get(header_len..frame_end) else {
                return Ok(DecodeResult::Incomplete);
            };
            return Ok(DecodeResult::Complete {
                value: body,
                consumed: frame_end,
            });
        }
    }
    Err(WireError::FrameLengthTooWide)
}

/// Appends one Minecraft `VarInt21` packet frame to `output`.
///
/// Validation happens before the output vector is modified, so rejected frames leave it unchanged.
///
/// # Errors
///
/// Returns an error for a zero-length body or when the body exceeds either the vanilla framing
/// ceiling or `max_body_len`.
pub fn encode_frame(
    body: &[u8],
    max_body_len: usize,
    output: &mut Vec<u8>,
) -> Result<(), WireError> {
    if body.is_empty() {
        return Err(WireError::ZeroLengthFrame);
    }
    let max = max_body_len.min(MAX_FRAME_BODY_LEN);
    if body.len() > max {
        return Err(WireError::ByteLengthLimitExceeded {
            length: body.len(),
            max,
        });
    }
    let signed_len = i32::try_from(body.len())
        .map_err(|_| WireError::LengthDoesNotFitVarInt { length: body.len() })?;
    output.reserve(var_int_len(signed_len) + body.len());
    encode_var_int(signed_len, output);
    output.extend_from_slice(body);
    Ok(())
}

/// Decodes a Minecraft `Utf8String` value using the vanilla UTF-16 code-unit bound.
///
/// Mojang's `Utf8String` prefixes the encoded byte count with a signed `VarInt`, rejects negative
/// lengths, caps the byte count at three bytes per allowed UTF-16 code unit, then checks the decoded
/// Java-string length. Rust therefore measures the semantic limit with `encode_utf16().count()`.
/// The returned string borrows directly from `input`.
///
/// Crucible deliberately rejects malformed UTF-8 rather than accepting replacement decoding. This
/// is a fail-closed input policy and does not affect valid vanilla-client byte streams.
///
/// # Errors
///
/// Returns an error for invalid length prefixes, byte/UTF-16 limits, arithmetic overflow, or
/// invalid UTF-8. Fragmented but otherwise potentially valid input returns
/// [`DecodeResult::Incomplete`].
#[must_use]
pub fn decode_string(
    input: &[u8],
    max_utf16_units: usize,
) -> Result<DecodeResult<&str>, WireError> {
    let DecodeResult::Complete {
        value: signed_len,
        consumed: header_len,
    } = decode_var_int(input)?
    else {
        return Ok(DecodeResult::Incomplete);
    };
    if signed_len < 0 {
        return Err(WireError::NegativeLength(signed_len));
    }
    let byte_len = usize::try_from(signed_len).map_err(|_| WireError::LengthOverflow)?;
    let max_bytes = max_utf16_units
        .checked_mul(3)
        .ok_or(WireError::LengthOverflow)?;
    if byte_len > max_bytes {
        return Err(WireError::ByteLengthLimitExceeded {
            length: byte_len,
            max: max_bytes,
        });
    }
    let string_end = header_len
        .checked_add(byte_len)
        .ok_or(WireError::LengthOverflow)?;
    let Some(bytes) = input.get(header_len..string_end) else {
        return Ok(DecodeResult::Incomplete);
    };
    let value = str::from_utf8(bytes).map_err(|_| WireError::InvalidUtf8)?;
    let utf16_units = value.encode_utf16().count();
    if utf16_units > max_utf16_units {
        return Err(WireError::StringLengthLimitExceeded {
            utf16_units,
            max: max_utf16_units,
        });
    }
    Ok(DecodeResult::Complete {
        value,
        consumed: string_end,
    })
}

/// Appends one Minecraft `Utf8String` value to `output`.
///
/// Validation happens before the output vector is modified.
///
/// # Errors
///
/// Returns an error when the string exceeds the caller's UTF-16 code-unit bound, exceeds the
/// corresponding three-bytes-per-unit encoded ceiling, or its byte count cannot fit an `i32`.
pub fn encode_string(
    value: &str,
    max_utf16_units: usize,
    output: &mut Vec<u8>,
) -> Result<(), WireError> {
    let utf16_units = value.encode_utf16().count();
    if utf16_units > max_utf16_units {
        return Err(WireError::StringLengthLimitExceeded {
            utf16_units,
            max: max_utf16_units,
        });
    }
    let max_bytes = max_utf16_units
        .checked_mul(3)
        .ok_or(WireError::LengthOverflow)?;
    if value.len() > max_bytes {
        return Err(WireError::ByteLengthLimitExceeded {
            length: value.len(),
            max: max_bytes,
        });
    }
    let signed_len = i32::try_from(value.len()).map_err(|_| WireError::LengthDoesNotFitVarInt {
        length: value.len(),
    })?;
    output.reserve(var_int_len(signed_len) + value.len());
    encode_var_int(signed_len, output);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DecodeResult, MAX_FRAME_BODY_LEN, WireError, decode_frame, decode_string, decode_var_int,
        encode_frame, encode_string, encode_var_int, var_int_len,
    };

    const VAR_INT_VECTORS: &[(i32, &[u8])] = &[
        (0, &[0x00]),
        (1, &[0x01]),
        (2, &[0x02]),
        (127, &[0x7f]),
        (128, &[0x80, 0x01]),
        (255, &[0xff, 0x01]),
        (2_147_483_647, &[0xff, 0xff, 0xff, 0xff, 0x07]),
        (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
        (-2_147_483_648, &[0x80, 0x80, 0x80, 0x80, 0x08]),
    ];

    #[test]
    fn canonical_var_int_vectors_match() {
        for &(value, expected) in VAR_INT_VECTORS {
            let mut encoded = Vec::new();
            encode_var_int(value, &mut encoded);
            assert_eq!(encoded, expected, "value={value}");
            assert_eq!(var_int_len(value), expected.len(), "value={value}");
            assert_eq!(
                decode_var_int(expected),
                Ok(DecodeResult::Complete {
                    value,
                    consumed: expected.len()
                }),
                "value={value}"
            );
        }
    }

    #[test]
    fn var_int_roundtrip_large_deterministic_corpus() {
        let mut state = 0xA5A5_1F3D_u32;
        for _ in 0..200_000 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let value = state.cast_signed();
            let mut encoded = Vec::with_capacity(5);
            encode_var_int(value, &mut encoded);
            assert_eq!(encoded.len(), var_int_len(value));
            assert_eq!(
                decode_var_int(&encoded),
                Ok(DecodeResult::Complete {
                    value,
                    consumed: encoded.len()
                })
            );
        }
    }

    #[test]
    fn var_int_fragmentation_is_incomplete_until_terminal_byte() {
        for &(_, encoded) in VAR_INT_VECTORS {
            for split in 0..encoded.len() {
                assert_eq!(
                    decode_var_int(&encoded[..split]),
                    Ok(DecodeResult::Incomplete),
                    "encoded={encoded:?} split={split}"
                );
            }
        }
    }

    #[test]
    fn var_int_rejects_five_continuation_bytes() {
        assert_eq!(
            decode_var_int(&[0x80, 0x80, 0x80, 0x80, 0x80]),
            Err(WireError::VarIntTooLong)
        );
    }

    #[test]
    fn frame_fragmentation_is_incomplete_at_every_boundary() {
        let payload: Vec<u8> = (0_u16..300)
            .map(|value| (value % 256).to_le_bytes()[0])
            .collect();
        let mut encoded = Vec::new();
        encode_frame(&payload, 1_024, &mut encoded).expect("valid frame");

        for split in 0..encoded.len() {
            assert_eq!(
                decode_frame(&encoded[..split], 1_024),
                Ok(DecodeResult::Incomplete),
                "split={split}"
            );
        }
        assert_eq!(
            decode_frame(&encoded, 1_024),
            Ok(DecodeResult::Complete {
                value: payload.as_slice(),
                consumed: encoded.len()
            })
        );
    }

    #[test]
    fn frame_rejects_zero_and_wider_than_varint21() {
        assert_eq!(decode_frame(&[0x00], 32), Err(WireError::ZeroLengthFrame));
        assert_eq!(
            decode_frame(&[0x80, 0x80, 0x80], MAX_FRAME_BODY_LEN),
            Err(WireError::FrameLengthTooWide)
        );
    }

    #[test]
    fn frame_rejects_limit_before_payload_access() {
        assert_eq!(
            decode_frame(&[0x0a], 9),
            Err(WireError::ByteLengthLimitExceeded { length: 10, max: 9 })
        );
    }

    #[test]
    fn frame_consumption_supports_coalesced_streams() {
        let mut encoded = Vec::new();
        encode_frame(b"abc", 32, &mut encoded).expect("first frame");
        let second_offset = encoded.len();
        encode_frame(b"d", 32, &mut encoded).expect("second frame");

        let first = decode_frame(&encoded, 32).expect("valid first frame");
        assert_eq!(
            first,
            DecodeResult::Complete {
                value: b"abc".as_slice(),
                consumed: second_offset
            }
        );
        let DecodeResult::Complete { consumed, .. } = first else {
            panic!("complete frame expected");
        };
        assert_eq!(
            decode_frame(&encoded[consumed..], 32),
            Ok(DecodeResult::Complete {
                value: b"d".as_slice(),
                consumed: 2
            })
        );
    }

    #[test]
    fn rejected_frame_encode_is_transactional() {
        let mut output = vec![7, 8, 9];
        assert_eq!(
            encode_frame(b"", 3, &mut output),
            Err(WireError::ZeroLengthFrame)
        );
        assert_eq!(output, [7, 8, 9]);
        assert_eq!(
            encode_frame(b"abcd", 3, &mut output),
            Err(WireError::ByteLengthLimitExceeded { length: 4, max: 3 })
        );
        assert_eq!(output, [7, 8, 9]);
    }

    #[test]
    fn string_roundtrip_preserves_utf8_and_exact_consumption() {
        let value = "Crucible 🔥 世界";
        let mut encoded = Vec::new();
        encode_string(value, 32, &mut encoded).expect("valid string");
        assert_eq!(
            decode_string(&encoded, 32),
            Ok(DecodeResult::Complete {
                value,
                consumed: encoded.len()
            })
        );
        for split in 0..encoded.len() {
            assert_eq!(
                decode_string(&encoded[..split], 32),
                Ok(DecodeResult::Incomplete),
                "split={split}"
            );
        }
    }

    #[test]
    fn string_uses_java_utf16_length_semantics() {
        let mut encoded = Vec::new();
        assert_eq!(
            encode_string("🔥", 1, &mut encoded),
            Err(WireError::StringLengthLimitExceeded {
                utf16_units: 2,
                max: 1
            })
        );
        assert!(encoded.is_empty());

        encode_string("🔥a", 3, &mut encoded).expect("three Java UTF-16 units");
        assert_eq!(
            decode_string(&encoded, 2),
            Err(WireError::StringLengthLimitExceeded {
                utf16_units: 3,
                max: 2
            })
        );
    }

    #[test]
    fn string_rejects_invalid_utf8_and_encoded_byte_limit() {
        assert_eq!(
            decode_string(&[0x02, 0xff, 0xff], 8),
            Err(WireError::InvalidUtf8)
        );
        assert_eq!(
            decode_string(&[0x04], 1),
            Err(WireError::ByteLengthLimitExceeded { length: 4, max: 3 })
        );
    }

    #[test]
    fn rejected_string_encode_is_transactional() {
        let mut output = vec![1, 2, 3];
        assert_eq!(
            encode_string("abcd", 3, &mut output),
            Err(WireError::StringLengthLimitExceeded {
                utf16_units: 4,
                max: 3
            })
        );
        assert_eq!(output, [1, 2, 3]);
    }

    #[test]
    fn zero_length_string_is_valid() {
        let mut encoded = Vec::new();
        encode_string("", 0, &mut encoded).expect("empty string");
        assert_eq!(encoded, [0]);
        assert_eq!(
            decode_string(&encoded, 0),
            Ok(DecodeResult::Complete {
                value: "",
                consumed: 1
            })
        );
    }
}
