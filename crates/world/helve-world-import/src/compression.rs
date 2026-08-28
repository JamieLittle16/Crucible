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

use crate::{
    anvil::ChunkCompression,
    transaction::ChunkPayloadDecoder,
};

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
    #[must_use]
    pub fn with_output_limit(max_decompressed_bytes: usize) -> Self {
        Self {
            output: vec![0; max_decompressed_bytes],
            decompressor: DecompressorOxide::new(),
        }
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
        if self.output.len() < max_decompressed_bytes {
            self.output.resize(max_decompressed_bytes, 0);
        }

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
    if payload[0..2] != [0x1f, 0x8b] {
        return Err(CompressedPayloadError::InvalidGzipMagic);
    }
    if payload[2] != 8 {
        return Err(CompressedPayloadError::UnsupportedGzipMethod { method: payload[2] });
    }

    let flags = payload[3];
    if flags & GZIP_RESERVED_FLAGS != 0 {
        return Err(CompressedPayloadError::ReservedGzipFlags { flags });
    }

    let trailer_start = payload.len() - GZIP_TRAILER_BYTES;
    let mut cursor = GZIP_FIXED_HEADER_BYTES;

    if flags & GZIP_FLAG_EXTRA != 0 {
        let xlen_bytes = payload
            .get(cursor..cursor + 2)
            .filter(|_| cursor + 2 <= trailer_start)
            .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
        let xlen = usize::from(u16::from_le_bytes([xlen_bytes[0], xlen_bytes[1]]));
        cursor = cursor
            .checked_add(2)
            .and_then(|value| value.checked_add(xlen))
            .filter(|&value| value <= trailer_start)
            .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
    }
    if flags & GZIP_FLAG_NAME != 0 {
        cursor = skip_zero_terminated(payload, cursor, trailer_start)?;
    }
    if flags & GZIP_FLAG_COMMENT != 0 {
        cursor = skip_zero_terminated(payload, cursor, trailer_start)?;
    }
    if flags & GZIP_FLAG_HEADER_CRC != 0 {
        let crc_bytes = payload
            .get(cursor..cursor + 2)
            .filter(|_| cursor + 2 <= trailer_start)
            .ok_or(CompressedPayloadError::MalformedGzipHeader)?;
        let expected = u16::from_le_bytes([crc_bytes[0], crc_bytes[1]]);
        let actual = crc32(&payload[..cursor]).to_le_bytes();
        let actual = u16::from_le_bytes([actual[0], actual[1]]);
        if expected != actual {
            return Err(CompressedPayloadError::GzipHeaderCrcMismatch { expected, actual });
        }
        cursor += 2;
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

const CRC32_TABLE: [u32; 256] = [
    0x00000000, 0x77073096, 0xee0e612c, 0x990951ba, 0x076dc419, 0x706af48f, 0xe963a535, 0x9e6495a3,
    0x0edb8832, 0x79dcb8a4, 0xe0d5e91e, 0x97d2d988, 0x09b64c2b, 0x7eb17cbd, 0xe7b82d07, 0x90bf1d91,
    0x1db71064, 0x6ab020f2, 0xf3b97148, 0x84be41de, 0x1adad47d, 0x6ddde4eb, 0xf4d4b551, 0x83d385c7,
    0x136c9856, 0x646ba8c0, 0xfd62f97a, 0x8a65c9ec, 0x14015c4f, 0x63066cd9, 0xfa0f3d63, 0x8d080df5,
    0x3b6e20c8, 0x4c69105e, 0xd56041e4, 0xa2677172, 0x3c03e4d1, 0x4b04d447, 0xd20d85fd, 0xa50ab56b,
    0x35b5a8fa, 0x42b2986c, 0xdbbbc9d6, 0xacbcf940, 0x32d86ce3, 0x45df5c75, 0xdcd60dcf, 0xabd13d59,
    0x26d930ac, 0x51de003a, 0xc8d75180, 0xbfd06116, 0x21b4f4b5, 0x56b3c423, 0xcfba9599, 0xb8bda50f,
    0x2802b89e, 0x5f058808, 0xc60cd9b2, 0xb10be924, 0x2f6f7c87, 0x58684c11, 0xc1611dab, 0xb6662d3d,
    0x76dc4190, 0x01db7106, 0x98d220bc, 0xefd5102a, 0x71b18589, 0x06b6b51f, 0x9fbfe4a5, 0xe8b8d433,
    0x7807c9a2, 0x0f00f934, 0x9609a88e, 0xe10e9818, 0x7f6a0dbb, 0x086d3d2d, 0x91646c97, 0xe6635c01,
    0x6b6b51f4, 0x1c6c6162, 0x856530d8, 0xf262004e, 0x6c0695ed, 0x1b01a57b, 0x8208f4c1, 0xf50fc457,
    0x65b0d9c6, 0x12b7e950, 0x8bbeb8ea, 0xfcb9887c, 0x62dd1ddf, 0x15da2d49, 0x8cd37cf3, 0xfbd44c65,
    0x4db26158, 0x3ab551ce, 0xa3bc0074, 0xd4bb30e2, 0x4adfa541, 0x3dd895d7, 0xa4d1c46d, 0xd3d6f4fb,
    0x4369e96a, 0x346ed9fc, 0xad678846, 0xda60b8d0, 0x44042d73, 0x33031de5, 0xaa0a4c5f, 0xdd0d7cc9,
    0x5005713c, 0x270241aa, 0xbe0b1010, 0xc90c2086, 0x5768b525, 0x206f85b3, 0xb966d409, 0xce61e49f,
    0x5edef90e, 0x29d9c998, 0xb0d09822, 0xc7d7a8b4, 0x59b33d17, 0x2eb40d81, 0xb7bd5c3b, 0xc0ba6cad,
    0xedb88320, 0x9abfb3b6, 0x03b6e20c, 0x74b1d29a, 0xead54739, 0x9dd277af, 0x04db2615, 0x73dc1683,
    0xe3630b12, 0x94643b84, 0x0d6d6a3e, 0x7a6a5aa8, 0xe40ecf0b, 0x9309ff9d, 0x0a00ae27, 0x7d079eb1,
    0xf00f9344, 0x8708a3d2, 0x1e01f268, 0x6906c2fe, 0xf762575d, 0x806567cb, 0x196c3671, 0x6e6b06e7,
    0xfed41b76, 0x89d32be0, 0x10da7a5a, 0x67dd4acc, 0xf9b9df6f, 0x8ebeeff9, 0x17b7be43, 0x60b08ed5,
    0xd6d6a3e8, 0xa1d1937e, 0x38d8c2c4, 0x4fdff252, 0xd1bb67f1, 0xa6bc5767, 0x3fb506dd, 0x48b2364b,
    0xd80d2bda, 0xaf0a1b4c, 0x36034af6, 0x41047a60, 0xdf60efc3, 0xa867df55, 0x316e8eef, 0x4669be79,
    0xcb61b38c, 0xbc66831a, 0x256fd2a0, 0x5268e236, 0xcc0c7795, 0xbb0b4703, 0x220216b9, 0x5505262f,
    0xc5ba3bbe, 0xb2bd0b28, 0x2bb45a92, 0x5cb36a04, 0xc2d7ffa7, 0xb5d0cf31, 0x2cd99e8b, 0x5bdeae1d,
    0x9b64c2b0, 0xec63f226, 0x756aa39c, 0x026d930a, 0x9c0906a9, 0xeb0e363f, 0x72076785, 0x05005713,
    0x95bf4a82, 0xe2b87a14, 0x7bb12bae, 0x0cb61b38, 0x92d28e9b, 0xe5d5be0d, 0x7cdcefb7, 0x0bdbdf21,
    0x86d3d2d4, 0xf1d4e242, 0x68ddb3f8, 0x1fda836e, 0x81be16cd, 0xf6b9265b, 0x6fb077e1, 0x18b74777,
    0x88085ae6, 0xff0f6a70, 0x66063bca, 0x11010b5c, 0x8f659eff, 0xf862ae69, 0x616bffd3, 0x166ccf45,
    0xa00ae278, 0xd70dd2ee, 0x4e048354, 0x3903b3c2, 0xa7672661, 0xd06016f7, 0x4969474d, 0x3e6e77db,
    0xaed16a4a, 0xd9d65adc, 0x40df0b66, 0x37d83bf0, 0xa9bcae53, 0xdebb9ec5, 0x47b2cf7f, 0x30b5ffe9,
    0xbdbdf21c, 0xcabac28a, 0x53b39330, 0x24b4a3a6, 0xbad03605, 0xcdd70693, 0x54de5729, 0x23d967bf,
    0xb3667a2e, 0xc4614ab8, 0x5d681b02, 0x2a6f2b94, 0xb40bbe37, 0xc30c8ea1, 0x5a05df1b, 0x2d02ef8d,
];

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        let index = usize::from((crc ^ u32::from(byte)).to_le_bytes()[0]);
        crc = CRC32_TABLE[index] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::{
        CompressedPayloadError, DeflateChunkPayloadDecoder, crc32,
    };
    use crate::{ChunkCompression, ChunkPayloadDecoder};

    const PLAIN: &[u8] = b"Helve bounded codec fixture";
    const ZLIB: &[u8] = &[
        0x78, 0x9c, 0xf3, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0x48, 0x2a, 0xcd, 0x4b, 0xce, 0x4f,
        0x49, 0x55, 0x48, 0xce, 0x4f, 0x49, 0xcd, 0x2b, 0x51, 0x48, 0xcb, 0xac, 0x28, 0x29,
        0x2d, 0x4a, 0x05, 0x00, 0x87, 0xb7, 0x0a, 0x58,
    ];
    const GZIP: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0xf3, 0x48, 0xcd, 0xc9,
        0xc9, 0x57, 0x48, 0x2a, 0xcd, 0x4b, 0xce, 0x4f, 0x49, 0x55, 0x48, 0xce, 0x4f, 0x49,
        0xcd, 0x2b, 0x51, 0x48, 0xcb, 0xac, 0x28, 0x29, 0x2d, 0x4a, 0x05, 0x00, 0x7d, 0x2c,
        0x9b, 0xb4, 0x1b, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn crc32_fixture_is_stable() {
        assert_eq!(crc32(PLAIN), 0xb49b_2c7d);
    }

    #[test]
    fn zlib_and_gzip_decode_to_identical_borrowed_output() {
        let mut decoder = DeflateChunkPayloadDecoder::with_output_limit(128);
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
