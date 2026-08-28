//! Bounded reusable DEFLATE-backed Anvil payload decoding.
//!
//! The selected mechanism uses the allocation-free `miniz_oxide` inflate core directly. Helve owns
//! the reusable output buffer and all gzip framing/checksum policy. This keeps compressed persisted
//! bytes at the cold import boundary without adding an allocator, streaming dictionary, trait-object,
//! or live-world dependency to the codec.

use core::fmt;

use miniz_oxide::inflate::{
    TINFLStatus,
    core::{
        DecompressorOxide, decompress,
        inflate_flags::{
            TINFL_FLAG_COMPUTE_ADLER32, TINFL_FLAG_PARSE_ZLIB_HEADER,
            TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
        },
    },
};

use crate::{anvil::ChunkCompression, transaction::ChunkPayloadDecoder};

const GZIP_FIXED_HEADER_BYTES: usize = 10;
const GZIP_TRAILER_BYTES: usize = 8;
const GZIP_FLAG_HEADER_CRC: u8 = 0x02;
const GZIP_FLAG_EXTRA: u8 = 0x04;
const GZIP_FLAG_NAME: u8 = 0x08;
const GZIP_FLAG_COMMENT: u8 = 0x10;
const GZIP_RESERVED_FLAGS: u8 = 0xe0;

/// Production candidate for gzip/zlib/uncompressed Anvil payloads.
///
/// The decoder retains one initialized output buffer at its historical high-water mark and one
/// allocation-free DEFLATE state. Construct it once per cold loading session and reuse it across
/// chunks. LZ4 remains deliberately unadmitted.
pub struct DeflateChunkPayloadDecoder {
    output: Vec<u8>,
    decompressor: DecompressorOxide,
}

impl DeflateChunkPayloadDecoder {
    /// Creates an empty decoder. The first compressed decode grows the retained output buffer to the
    /// requested decompressed limit; later decodes at or below that limit allocate nothing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            output: Vec::new(),
            decompressor: DecompressorOxide::new(),
        }
    }

    /// Creates a decoder with an already-initialized reusable output buffer.
    ///
    /// This is the preferred production construction when the import profile's decompressed bound is
    /// known up front because the first chunk then performs no buffer growth.
    ///
    /// # Errors
    ///
    /// Returns [`CompressedPayloadError::OutputBufferAllocationFailed`] if the requested retained
    /// buffer cannot be reserved.
    pub fn try_with_output_limit(
        max_decompressed_bytes: usize,
    ) -> Result<Self, CompressedPayloadError> {
        let mut decoder = Self::new();
        decoder.ensure_output(max_decompressed_bytes)?;
        Ok(decoder)
    }

    /// Returns bytes currently initialized and retained for decompression output.
    #[must_use]
    pub fn retained_output_bytes(&self) -> usize {
        self.output.len()
    }

    /// Returns allocated output capacity retained by this decoder.
    #[must_use]
    pub fn retained_output_capacity(&self) -> usize {
        self.output.capacity()
    }

    fn ensure_output(&mut self, required: usize) -> Result<(), CompressedPayloadError> {
        if self.output.len() >= required {
            return Ok(());
        }
        let additional = required - self.output.len();
        self.output
            .try_reserve_exact(additional)
            .map_err(|_| CompressedPayloadError::OutputBufferAllocationFailed {
                requested: required,
            })?;
        self.output.resize(required, 0);
        Ok(())
    }

    fn decode_zlib<'a>(
        &'a mut self,
        payload: &[u8],
        max_decompressed_bytes: usize,
    ) -> Result<&'a [u8], CompressedPayloadError> {
        self.inflate_exact(
            payload,
            max_decompressed_bytes,
            TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
                | TINFL_FLAG_PARSE_ZLIB_HEADER
                | TINFL_FLAG_COMPUTE_ADLER32,
            WrapperKind::Zlib,
        )
    }

    fn decode_gzip<'a>(
        &'a mut self,
        payload: &[u8],
        max_decompressed_bytes: usize,
    ) -> Result<&'a [u8], CompressedPayloadError> {
        let member = parse_gzip_member(payload)?;
        let body = payload
            .get(member.body_start..member.trailer_start)
            .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
        let output = self.inflate_exact(
            body,
            max_decompressed_bytes,
            TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
            WrapperKind::Gzip,
        )?;

        let actual_crc = crc32(output);
        if actual_crc != member.expected_crc32 {
            return Err(CompressedPayloadError::GzipCrcMismatch {
                expected: member.expected_crc32,
                actual: actual_crc,
            });
        }
        let actual_size = u32::try_from(output.len()).map_err(|_| {
            CompressedPayloadError::GzipSizeOutsideU32 {
                actual: output.len(),
            }
        })?;
        if actual_size != member.expected_size {
            return Err(CompressedPayloadError::GzipSizeMismatch {
                expected: member.expected_size,
                actual: actual_size,
            });
        }
        Ok(output)
    }

    fn inflate_exact<'a>(
        &'a mut self,
        payload: &[u8],
        max_decompressed_bytes: usize,
        flags: u32,
        wrapper: WrapperKind,
    ) -> Result<&'a [u8], CompressedPayloadError> {
        if max_decompressed_bytes == 0 {
            return Err(CompressedPayloadError::OutputExceedsLimit { limit: 0 });
        }
        self.ensure_output(max_decompressed_bytes)?;

        self.decompressor.init();
        let output = self
            .output
            .get_mut(..max_decompressed_bytes)
            .ok_or(CompressedPayloadError::OutputExceedsLimit {
                limit: max_decompressed_bytes,
            })?;
        let (status, consumed, written) =
            decompress(&mut self.decompressor, payload, output, 0, flags);

        match status {
            TINFLStatus::Done => {
                if consumed != payload.len() {
                    return Err(CompressedPayloadError::TrailingCompressedBytes {
                        trailing: payload.len() - consumed,
                    });
                }
                self.output
                    .get(..written)
                    .ok_or(CompressedPayloadError::MalformedDeflate { wrapper })
            }
            TINFLStatus::HasMoreOutput => Err(CompressedPayloadError::OutputExceedsLimit {
                limit: max_decompressed_bytes,
            }),
            TINFLStatus::Adler32Mismatch => Err(CompressedPayloadError::ZlibAdlerMismatch),
            TINFLStatus::FailedCannotMakeProgress
            | TINFLStatus::BadParam
            | TINFLStatus::Failed
            | TINFLStatus::NeedsMoreInput => {
                Err(CompressedPayloadError::MalformedDeflate { wrapper })
            }
        }
    }
}

impl Default for DeflateChunkPayloadDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for DeflateChunkPayloadDecoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeflateChunkPayloadDecoder")
            .field("retained_output_bytes", &self.output.len())
            .field("retained_output_capacity", &self.output.capacity())
            .finish_non_exhaustive()
    }
}

impl ChunkPayloadDecoder for DeflateChunkPayloadDecoder {
    type Error = CompressedPayloadError;

    fn decode<'a>(
        &'a mut self,
        compression: ChunkCompression,
        payload: &'a [u8],
        max_decompressed_bytes: usize,
    ) -> Result<&'a [u8], Self::Error> {
        match compression {
            ChunkCompression::Uncompressed => {
                if payload.len() > max_decompressed_bytes {
                    return Err(CompressedPayloadError::PayloadExceedsLimit {
                        actual: payload.len(),
                        limit: max_decompressed_bytes,
                    });
                }
                Ok(payload)
            }
            ChunkCompression::Zlib => self.decode_zlib(payload, max_decompressed_bytes),
            ChunkCompression::Gzip => self.decode_gzip(payload, max_decompressed_bytes),
            ChunkCompression::Lz4 => Err(CompressedPayloadError::CompressionNotAdmitted {
                compression,
            }),
        }
    }
}

/// Wrapper identity used only to classify a raw DEFLATE failure without leaking third-party status
/// types into Helve's public API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WrapperKind {
    /// RFC 1950 zlib framing.
    Zlib,
    /// RFC 1952 gzip framing around a raw RFC 1951 DEFLATE body.
    Gzip,
}

/// Fail-closed errors from [`DeflateChunkPayloadDecoder`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressedPayloadError {
    /// The Anvil compression identity is intentionally not supported by this decoder.
    CompressionNotAdmitted { compression: ChunkCompression },
    /// An uncompressed payload exceeds the configured decompressed bound.
    PayloadExceedsLimit { actual: usize, limit: usize },
    /// The compressed stream requires more output than the configured hard bound.
    OutputExceedsLimit { limit: usize },
    /// The reusable output buffer could not grow to the selected profile bound.
    OutputBufferAllocationFailed { requested: usize },
    /// The DEFLATE/zlib stream is truncated or malformed.
    MalformedDeflate { wrapper: WrapperKind },
    /// A zlib stream's Adler-32 checksum disagrees with its trailer.
    ZlibAdlerMismatch,
    /// The decompressor reached stream end before consuming the exact admitted compressed slice.
    TrailingCompressedBytes { trailing: usize },
    /// Gzip framing is shorter than the fixed header plus trailer.
    GzipTooShort { actual: usize },
    /// Gzip magic bytes are invalid.
    InvalidGzipMagic,
    /// Gzip compression method is not DEFLATE.
    UnsupportedGzipMethod { method: u8 },
    /// Reserved gzip flag bits are set.
    ReservedGzipFlags { flags: u8 },
    /// An optional gzip header field is truncated or unterminated.
    MalformedGzipHeader,
    /// Optional gzip header CRC16 does not match the low 16 bits of the header CRC32.
    GzipHeaderCrcMismatch { expected: u16, actual: u16 },
    /// Gzip trailer CRC32 does not match decompressed bytes.
    GzipCrcMismatch { expected: u32, actual: u32 },
    /// Decompressed output cannot be represented by the gzip ISIZE field.
    GzipSizeOutsideU32 { actual: usize },
    /// Gzip ISIZE disagrees with decompressed byte count.
    GzipSizeMismatch { expected: u32, actual: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GzipMember {
    body_start: usize,
    trailer_start: usize,
    expected_crc32: u32,
    expected_size: u32,
}

fn parse_gzip_member(payload: &[u8]) -> Result<GzipMember, CompressedPayloadError> {
    let minimum = GZIP_FIXED_HEADER_BYTES + GZIP_TRAILER_BYTES;
    if payload.len() < minimum {
        return Err(CompressedPayloadError::GzipTooShort {
            actual: payload.len(),
        });
    }
    if payload.get(..2) != Some(&[0x1f, 0x8b][..]) {
        return Err(CompressedPayloadError::InvalidGzipMagic);
    }
    let method = payload[2];
    if method != 8 {
        return Err(CompressedPayloadError::UnsupportedGzipMethod { method });
    }

    let flags = payload[3];
    if flags & GZIP_RESERVED_FLAGS != 0 {
        return Err(CompressedPayloadError::ReservedGzipFlags { flags });
    }

    let trailer_start = payload.len() - GZIP_TRAILER_BYTES;
    let mut cursor = GZIP_FIXED_HEADER_BYTES;

    if flags & GZIP_FLAG_EXTRA != 0 {
        let length_end = cursor
            .checked_add(2)
            .filter(|&end| end <= trailer_start)
            .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
        let xlen_bytes = payload
            .get(cursor..length_end)
            .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
        let xlen = usize::from(u16::from_le_bytes([xlen_bytes[0], xlen_bytes[1]]));
        cursor = length_end
            .checked_add(xlen)
            .filter(|&end| end <= trailer_start)
            .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
    }
    if flags & GZIP_FLAG_NAME != 0 {
        cursor = skip_zero_terminated(payload, cursor, trailer_start)?;
    }
    if flags & GZIP_FLAG_COMMENT != 0 {
        cursor = skip_zero_terminated(payload, cursor, trailer_start)?;
    }
    if flags & GZIP_FLAG_HEADER_CRC != 0 {
        let crc_end = cursor
            .checked_add(2)
            .filter(|&end| end <= trailer_start)
            .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
        let crc_bytes = payload
            .get(cursor..crc_end)
            .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
        let expected = u16::from_le_bytes([crc_bytes[0], crc_bytes[1]]);
        let actual_crc = crc32(&payload[..cursor]).to_le_bytes();
        let actual = u16::from_le_bytes([actual_crc[0], actual_crc[1]]);
        if expected != actual {
            return Err(CompressedPayloadError::GzipHeaderCrcMismatch { expected, actual });
        }
        cursor = crc_end;
    }

    let trailer = payload
        .get(trailer_start..)
        .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
    let expected_crc32 = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let expected_size = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);

    Ok(GzipMember {
        body_start: cursor,
        trailer_start,
        expected_crc32,
        expected_size,
    })
}

fn skip_zero_terminated(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<usize, CompressedPayloadError> {
    let bytes = payload
        .get(start..end)
        .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
    let terminator = bytes
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
    start
        .checked_add(terminator)
        .and_then(|value| value.checked_add(1))
        .ok_or(CompressedPayloadError::MalformedGzipHeader)
}

// Two table lookups per byte keep gzip CRC inexpensive without a third dependency or a 1 KiB table.
const CRC32_NIBBLE_TABLE: [u32; 16] = [
    0x0000_0000,
    0x1db7_1064,
    0x3b6e_20c8,
    0x26d9_30ac,
    0x76dc_4190,
    0x6b6b_51f4,
    0x4db2_6158,
    0x5005_713c,
    0xedb8_8320,
    0xf00f_9344,
    0xd6d6_a3e8,
    0xcb61_b38c,
    0x9b64_c2b0,
    0x86d3_d2d4,
    0xa00a_e278,
    0xbdbd_f21c,
];

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        let low = usize::from(crc.to_le_bytes()[0] & 0x0f);
        crc = CRC32_NIBBLE_TABLE[low] ^ (crc >> 4);
        let high = usize::from(crc.to_le_bytes()[0] & 0x0f);
        crc = CRC32_NIBBLE_TABLE[high] ^ (crc >> 4);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::{CompressedPayloadError, DeflateChunkPayloadDecoder, GZIP_FLAG_NAME, crc32};
    use crate::{ChunkCompression, ChunkPayloadDecoder};

    const PLAIN: &[u8] = b"Helve bounded codec fixture";
    const ZLIB: &[u8] = &[
        0x78, 0x9c, 0xf3, 0x48, 0xcd, 0x29, 0x4b, 0x55, 0x48, 0xca, 0x2f, 0xcd, 0x4b, 0x49,
        0x4d, 0x51, 0x48, 0xce, 0x4f, 0x49, 0x4d, 0x56, 0x48, 0xcb, 0xac, 0x28, 0x29, 0x2d,
        0x4a, 0x05, 0x00, 0x8c, 0x19, 0x0a, 0x3b,
    ];
    const GZIP: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xf3, 0x48, 0xcd, 0x29,
        0x4b, 0x55, 0x48, 0xca, 0x2f, 0xcd, 0x4b, 0x49, 0x4d, 0x51, 0x48, 0xce, 0x4f, 0x49,
        0x4d, 0x56, 0x48, 0xcb, 0xac, 0x28, 0x29, 0x2d, 0x4a, 0x05, 0x00, 0xe5, 0x69, 0x81,
        0x33, 0x1b, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn crc32_fixture_is_stable() {
        assert_eq!(crc32(PLAIN), 0x3381_69e5);
    }

    #[test]
    fn zlib_and_gzip_decode_to_identical_borrowed_output() {
        let mut decoder =
            DeflateChunkPayloadDecoder::try_with_output_limit(128).expect("test output buffer");
        assert_eq!(
            decoder
                .decode(ChunkCompression::Zlib, ZLIB, 128)
                .expect("valid zlib"),
            PLAIN
        );
        assert_eq!(
            decoder
                .decode(ChunkCompression::Gzip, GZIP, 128)
                .expect("valid gzip"),
            PLAIN
        );
        assert_eq!(decoder.retained_output_bytes(), 128);
    }

    #[test]
    fn gzip_optional_name_is_bounded_and_accepted() {
        let mut named = Vec::with_capacity(GZIP.len() + 6);
        named.extend_from_slice(&GZIP[..3]);
        named.push(GZIP_FLAG_NAME);
        named.extend_from_slice(&GZIP[4..10]);
        named.extend_from_slice(b"chunk\0");
        named.extend_from_slice(&GZIP[10..]);

        let mut decoder = DeflateChunkPayloadDecoder::new();
        assert_eq!(
            decoder
                .decode(ChunkCompression::Gzip, &named, 128)
                .expect("valid named gzip"),
            PLAIN
        );
    }

    #[test]
    fn high_water_buffer_is_reused_and_only_grows() {
        let mut decoder = DeflateChunkPayloadDecoder::new();
        decoder
            .decode(ChunkCompression::Zlib, ZLIB, 64)
            .expect("first decode");
        let first_capacity = decoder.retained_output_capacity();
        decoder
            .decode(ChunkCompression::Zlib, ZLIB, 32)
            .expect("smaller decode");
        assert_eq!(decoder.retained_output_capacity(), first_capacity);
        assert_eq!(decoder.retained_output_bytes(), 64);
    }

    #[test]
    fn output_bound_and_trailing_bytes_fail_closed() {
        let mut decoder = DeflateChunkPayloadDecoder::new();
        assert_eq!(
            decoder.decode(ChunkCompression::Zlib, ZLIB, 8),
            Err(CompressedPayloadError::OutputExceedsLimit { limit: 8 })
        );

        let mut trailing = ZLIB.to_vec();
        trailing.push(0);
        assert_eq!(
            decoder.decode(ChunkCompression::Zlib, &trailing, 128),
            Err(CompressedPayloadError::TrailingCompressedBytes { trailing: 1 })
        );
    }

    #[test]
    fn gzip_crc_and_size_are_verified() {
        let mut bad_crc = GZIP.to_vec();
        let crc_index = bad_crc.len() - 8;
        bad_crc[crc_index] ^= 1;
        let mut decoder = DeflateChunkPayloadDecoder::new();
        assert!(matches!(
            decoder.decode(ChunkCompression::Gzip, &bad_crc, 128),
            Err(CompressedPayloadError::GzipCrcMismatch { .. })
        ));

        let mut bad_size = GZIP.to_vec();
        let size_index = bad_size.len() - 4;
        bad_size[size_index] ^= 1;
        assert!(matches!(
            decoder.decode(ChunkCompression::Gzip, &bad_size, 128),
            Err(CompressedPayloadError::GzipSizeMismatch { .. })
        ));
    }

    #[test]
    fn lz4_remains_explicitly_unadmitted() {
        let mut decoder = DeflateChunkPayloadDecoder::new();
        assert_eq!(
            decoder.decode(ChunkCompression::Lz4, b"not lz4", 128),
            Err(CompressedPayloadError::CompressionNotAdmitted {
                compression: ChunkCompression::Lz4,
            })
        );
    }
}
