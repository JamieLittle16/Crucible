//! Allocation-conscious wire primitives for Crucible's Minecraft protocol spine.
//!
//! This crate deliberately contains no packet IDs or target-version state transitions. It owns
//! only reusable byte-level laws that can be exhaustively qualified before versioned packet
//! semantics are introduced.

#![forbid(unsafe_code)]

use core::str;

/// Maximum encoded width of a Minecraft-style signed 32-bit `VarInt`.
pub const MAX_VAR_INT_BYTES: usize = 5;

/// Fail-closed wire errors. Incomplete input is represented separately by [`DecodeResult`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireError {
    /// A `VarInt` still carried its continuation bit after five bytes.
    VarIntTooLong,
    /// A length prefix decoded to a negative signed value.
    NegativeLength(i32),
    /// A decoded or encoded byte length exceeded the caller's explicit limit.
    ByteLengthLimitExceeded { length: usize, max: usize },
    /// A decoded or encoded string exceeded the caller's explicit character limit.
    CharacterLimitExceeded { characters: usize, max: usize },
    /// A byte length cannot be represented by the signed 32-bit `VarInt` wire format.
    LengthDoesNotFitVarInt { length: usize },
    /// A complete length-prefixed string contained invalid UTF-8.
    InvalidUtf8,
    /// Integer arithmetic required to identify a complete frame overflowed `usize`.
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
            output.push(u8::try_from(remaining).expect("terminal VarInt byte fits u8"));
            return;
        }
        output.push(
            u8::try_from((remaining & 0x7f) | 0x80).expect("masked VarInt byte fits u8"),
        );
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

/// Decodes one length-delimited frame without allocating or copying its payload.
///
/// The returned slice borrows directly from `input`. The caller supplies the maximum accepted
/// payload length, allowing the networking layer to reject oversized frames before payload access.
///
/// # Errors
///
/// Returns an error for overlong `VarInt` values, negative lengths, lengths above
/// `max_payload_len`, or arithmetic overflow. A valid but fragmented frame returns
/// [`DecodeResult::Incomplete`].
pub fn decode_frame(
    input: &[u8],
    max_payload_len: usize,
) -> Result<DecodeResult<&[u8]>, WireError> {
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
    let payload_len = usize::try_from(signed_len).map_err(|_| WireError::LengthOverflow)?;
    if payload_len > max_payload_len {
        return Err(WireError::ByteLengthLimitExceeded {
            length: payload_len,
            max: max_payload_len,
        });
    }
    let frame_end = header_len
        .checked_add(payload_len)
        .ok_or(WireError::LengthOverflow)?;
    let Some(payload) = input.get(header_len..frame_end) else {
        return Ok(DecodeResult::Incomplete);
    };
    Ok(DecodeResult::Complete {
        value: payload,
        consumed: frame_end,
    })
}

/// Appends one length-delimited frame to `output`.
///
/// Validation happens before the output vector is modified, so rejected frames leave it unchanged.
///
/// # Errors
///
/// Returns an error when the payload exceeds `max_payload_len` or cannot be represented by the
/// signed 32-bit `VarInt` length field.
pub fn encode_frame(
    payload: &[u8],
    max_payload_len: usize,
    output: &mut Vec<u8>,
) -> Result<(), WireError> {
    validate_encoded_len(payload.len(), max_payload_len)?;
    let signed_len =
        i32::try_from(payload.len()).map_err(|_| WireError::LengthDoesNotFitVarInt {
            length: payload.len(),
        })?;
    output.reserve(var_int_len(signed_len) + payload.len());
    encode_var_int(signed_len, output);
    output.extend_from_slice(payload);
    Ok(())
}

/// Decodes one `VarInt`-length-prefixed UTF-8 string using caller-supplied byte and character
/// limits.
///
/// The returned string borrows directly from `input` and therefore performs no payload allocation.
///
/// # Errors
///
/// Returns an error for invalid length prefixes, byte/character limit violations, arithmetic
/// overflow, or invalid UTF-8. Fragmented but otherwise potentially valid input returns
/// [`DecodeResult::Incomplete`].
pub fn decode_string(
    input: &[u8],
    max_bytes: usize,
    max_characters: usize,
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
    let characters = value.chars().count();
    if characters > max_characters {
        return Err(WireError::CharacterLimitExceeded {
            characters,
            max: max_characters,
        });
    }
    Ok(DecodeResult::Complete {
        value,
        consumed: string_end,
    })
}

/// Appends one `VarInt`-length-prefixed UTF-8 string to `output`.
///
/// Validation happens before the output vector is modified.
///
/// # Errors
///
/// Returns an error when the string exceeds either caller-supplied bound or when its byte length
/// cannot be represented by the signed 32-bit `VarInt` length field.
pub fn encode_string(
    value: &str,
    max_bytes: usize,
    max_characters: usize,
    output: &mut Vec<u8>,
) -> Result<(), WireError> {
    validate_encoded_len(value.len(), max_bytes)?;
    let characters = value.chars().count();
    if characters > max_characters {
        return Err(WireError::CharacterLimitExceeded {
            characters,
            max: max_characters,
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

fn validate_encoded_len(length: usize, max: usize) -> Result<(), WireError> {
    if length > max {
        return Err(WireError::ByteLengthLimitExceeded { length, max });
    }
    if length > i32::MAX as usize {
        return Err(WireError::LengthDoesNotFitVarInt { length });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DecodeResult, WireError, decode_frame, decode_string, decode_var_int, encode_frame,
        encode_string, encode_var_int, var_int_len,
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
        let mut state = 0xD1B5_4A32_D192_ED03_u64;
        for _ in 0..200_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let lower = u32::try_from(state & u64::from(u32::MAX))
                .expect("masked deterministic state fits u32");
            let value = lower.cast_signed();
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
            .map(|value| u8::try_from(value % 256).expect("modulo 256 fits u8"))
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
    fn frame_rejects_negative_and_oversized_lengths_before_payload() {
        assert_eq!(
            decode_frame(&[0xff, 0xff, 0xff, 0xff, 0x0f], 1_024),
            Err(WireError::NegativeLength(-1))
        );
        assert_eq!(
            decode_frame(&[0x0a], 9),
            Err(WireError::ByteLengthLimitExceeded { length: 10, max: 9 })
        );
    }

    #[test]
    fn frame_consumption_supports_coalesced_streams_and_zero_length() {
        let mut encoded = Vec::new();
        encode_frame(b"abc", 32, &mut encoded).expect("first frame");
        let second_offset = encoded.len();
        encode_frame(b"", 32, &mut encoded).expect("second frame");

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
                value: b"".as_slice(),
                consumed: 1
            })
        );
    }

    #[test]
    fn rejected_frame_encode_is_transactional() {
        let mut output = vec![7, 8, 9];
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
        encode_string(value, 128, 32, &mut encoded).expect("valid string");
        assert_eq!(
            decode_string(&encoded, 128, 32),
            Ok(DecodeResult::Complete {
                value,
                consumed: encoded.len()
            })
        );
        for split in 0..encoded.len() {
            assert_eq!(
                decode_string(&encoded[..split], 128, 32),
                Ok(DecodeResult::Incomplete),
                "split={split}"
            );
        }
    }

    #[test]
    fn string_rejects_invalid_utf8_and_explicit_limits() {
        assert_eq!(
            decode_string(&[0x02, 0xff, 0xff], 8, 8),
            Err(WireError::InvalidUtf8)
        );
        assert_eq!(
            decode_string(&[0x04], 3, 8),
            Err(WireError::ByteLengthLimitExceeded { length: 4, max: 3 })
        );

        let mut encoded = Vec::new();
        encode_string("é", 2, 1, &mut encoded).expect("one Unicode scalar");
        assert_eq!(
            decode_string(&encoded, 2, 0),
            Err(WireError::CharacterLimitExceeded {
                characters: 1,
                max: 0
            })
        );
    }

    #[test]
    fn rejected_string_encode_is_transactional() {
        let mut output = vec![1, 2, 3];
        assert_eq!(
            encode_string("abcd", 3, 8, &mut output),
            Err(WireError::ByteLengthLimitExceeded { length: 4, max: 3 })
        );
        assert_eq!(output, [1, 2, 3]);

        assert_eq!(
            encode_string("é", 8, 0, &mut output),
            Err(WireError::CharacterLimitExceeded {
                characters: 1,
                max: 0
            })
        );
        assert_eq!(output, [1, 2, 3]);
    }

    #[test]
    fn zero_length_string_is_valid() {
        let mut encoded = Vec::new();
        encode_string("", 0, 0, &mut encoded).expect("empty string");
        assert_eq!(encoded, [0]);
        assert_eq!(
            decode_string(&encoded, 0, 0),
            Ok(DecodeResult::Complete {
                value: "",
                consumed: 1
            })
        );
    }
}
