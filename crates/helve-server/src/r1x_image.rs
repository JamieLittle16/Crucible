//! Strict cold-path loader for the compact experimental R1X replay image.
//!
//! The Python packer performs the cryptographic verification against the pinned source-free JSON
//! before composing the current Helve server-brand body. This loader independently rejects wrong
//! format/version/source/capture commitments, incoherent counts/byte totals, oversized bodies and
//! trailing data before constructing the process-owned immutable R1X context. No JSON parser or
//! dynamic packet registry exists in the server runtime.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::Path;

use helve_target_26_2::{R1xContextError, Target26_2R1xContext};

const MAGIC: [u8; 8] = *b"CRR1X001";
const EXPECTED_PROTOCOL: u32 = 776;
const EXPECTED_CONFIGURATION_COUNT: usize = 34;
const EXPECTED_CONFIGURATION_BYTES: usize = 44_430;
const MAX_PLAY_COUNT: usize = 2_331;
const MAX_PLAY_BYTES: usize = 6_135_522;
const MAX_BODY_BYTES: usize = 65_536;
const HEADER_BYTES: u64 = 100;
const MAX_IMAGE_BYTES: u64 = HEADER_BYTES
    + ((EXPECTED_CONFIGURATION_COUNT + MAX_PLAY_COUNT) as u64 * 4)
    + EXPECTED_CONFIGURATION_BYTES as u64
    + MAX_PLAY_BYTES as u64;

type PacketBody = Box<[u8]>;
type LoadedBodies = (Vec<PacketBody>, usize);

const EXPECTED_SOURCE_SHA256: [u8; 32] = [
    0x1e, 0x9b, 0xca, 0x3d, 0xff, 0x83, 0xcd, 0x83, 0xe7, 0x90, 0x5f, 0x88, 0x10, 0xf1, 0xec, 0x98,
    0x99, 0x36, 0x1f, 0xa2, 0xdc, 0x83, 0xfe, 0x89, 0x3b, 0xb4, 0x8b, 0xee, 0xb0, 0x4d, 0xf7, 0x50,
];
const EXPECTED_CAPTURE_SHA256: [u8; 32] = [
    0x11, 0xea, 0xd8, 0xde, 0x74, 0xdf, 0x70, 0xb4, 0x0d, 0x7f, 0xb0, 0x45, 0xff, 0x95, 0x61, 0xf0,
    0x6f, 0x6e, 0x24, 0x23, 0x87, 0x65, 0xd4, 0x14, 0x1a, 0x1d, 0x09, 0x0c, 0xab, 0x54, 0x6b, 0x57,
];

/// Fail-closed compact-image load error.
#[derive(Debug)]
pub enum R1xImageError {
    /// Filesystem access failed.
    Io {
        /// Stable I/O classification.
        kind: io::ErrorKind,
        /// Cold-path diagnostic detail.
        message: String,
    },
    /// Replay image path resolved to a symbolic link.
    Symlink,
    /// Replay image path was not a regular file.
    NotFile,
    /// File size exceeded the maximum possible pinned capture image.
    FileTooLarge { observed: u64, maximum: u64 },
    /// Header magic did not identify the R1X compact format.
    Magic,
    /// Protocol did not match Minecraft Java 26.2.
    Protocol { observed: u32 },
    /// Source archive commitment differed from the pinned 26.2 source archive.
    SourceCommitment,
    /// Capture commitment differed from the pinned source-free capture.
    CaptureCommitment,
    /// Configuration count differed from the sealed selected route.
    ConfigurationCount { observed: usize },
    /// Configuration aggregate body bytes differed from the selected runtime route.
    ConfigurationBytes { observed: usize },
    /// Selected Play prefix exceeded the pinned full capture count.
    PlayCount { observed: usize },
    /// Selected Play prefix exceeded the pinned full capture byte total.
    PlayBytes { observed: usize },
    /// One encoded body length was zero or exceeded the finite R1X packet-body bound.
    BodyLength {
        section: R1xImageSection,
        index: usize,
        observed: usize,
    },
    /// Actual decoded body bytes differed from the header aggregate.
    AggregateMismatch {
        section: R1xImageSection,
        declared: usize,
        observed: usize,
    },
    /// Bytes remained after the declared body sequence.
    TrailingData,
    /// Target-level immutable-image validation rejected the decoded bodies.
    Context(R1xContextError),
}

/// Compact-image body section used by diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R1xImageSection {
    Configuration,
    Play,
}

impl fmt::Display for R1xImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { kind, message } => write!(formatter, "replay image I/O {kind:?}: {message}"),
            Self::Symlink => formatter.write_str("replay image must not be a symlink"),
            Self::NotFile => formatter.write_str("replay image must be a regular file"),
            Self::FileTooLarge { observed, maximum } => write!(
                formatter,
                "replay image is {observed} bytes, exceeding the {maximum}-byte pinned bound"
            ),
            Self::Magic => formatter.write_str("replay image magic mismatch"),
            Self::Protocol { observed } => write!(
                formatter,
                "replay image protocol mismatch: expected {EXPECTED_PROTOCOL}, got {observed}"
            ),
            Self::SourceCommitment => {
                formatter.write_str("replay image source commitment mismatch")
            }
            Self::CaptureCommitment => {
                formatter.write_str("replay image capture commitment mismatch")
            }
            Self::ConfigurationCount { observed } => write!(
                formatter,
                "replay image Configuration count mismatch: expected {EXPECTED_CONFIGURATION_COUNT}, got {observed}"
            ),
            Self::ConfigurationBytes { observed } => write!(
                formatter,
                "replay image Configuration bytes mismatch: expected {EXPECTED_CONFIGURATION_BYTES}, got {observed}"
            ),
            Self::PlayCount { observed } => write!(
                formatter,
                "replay image Play count {observed} exceeds pinned maximum {MAX_PLAY_COUNT}"
            ),
            Self::PlayBytes { observed } => write!(
                formatter,
                "replay image Play bytes {observed} exceeds pinned maximum {MAX_PLAY_BYTES}"
            ),
            Self::BodyLength {
                section,
                index,
                observed,
            } => write!(
                formatter,
                "replay image {section:?} body {index} has invalid {observed}-byte length"
            ),
            Self::AggregateMismatch {
                section,
                declared,
                observed,
            } => write!(
                formatter,
                "replay image {section:?} aggregate mismatch: declared {declared}, decoded {observed}"
            ),
            Self::TrailingData => formatter.write_str("replay image contains trailing data"),
            Self::Context(error) => write!(
                formatter,
                "replay image target validation failed: {error:?}"
            ),
        }
    }
}

impl From<R1xContextError> for R1xImageError {
    fn from(value: R1xContextError) -> Self {
        Self::Context(value)
    }
}

/// Loads one compact R1X image into immutable process-owned target context.
///
/// The file is streamed directly into exact-size packet-body allocations. The loader never retains a
/// second whole-file copy and never performs per-connection decoding or reconstruction.
///
/// # Errors
///
/// Returns a precise fail-closed format, bound, filesystem or target-context error.
pub fn load_r1x_image(
    path: &Path,
    status_json: &str,
) -> Result<Target26_2R1xContext, R1xImageError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(&error))?;
    if metadata.file_type().is_symlink() {
        return Err(R1xImageError::Symlink);
    }
    if !metadata.is_file() {
        return Err(R1xImageError::NotFile);
    }
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(R1xImageError::FileTooLarge {
            observed: metadata.len(),
            maximum: MAX_IMAGE_BYTES,
        });
    }

    let file = File::open(path).map_err(|error| io_error(&error))?;
    let mut reader = BufReader::new(file);
    decode_image(&mut reader, status_json)
}

fn decode_image<R: Read>(
    reader: &mut R,
    status_json: &str,
) -> Result<Target26_2R1xContext, R1xImageError> {
    if read_array::<8, _>(reader)? != MAGIC {
        return Err(R1xImageError::Magic);
    }
    let protocol = read_u32(reader)?;
    if protocol != EXPECTED_PROTOCOL {
        return Err(R1xImageError::Protocol { observed: protocol });
    }
    if read_array::<32, _>(reader)? != EXPECTED_SOURCE_SHA256 {
        return Err(R1xImageError::SourceCommitment);
    }
    if read_array::<32, _>(reader)? != EXPECTED_CAPTURE_SHA256 {
        return Err(R1xImageError::CaptureCommitment);
    }

    let configuration_count = usize_from_u32(read_u32(reader)?)?;
    let play_count = usize_from_u32(read_u32(reader)?)?;
    let configuration_bytes = usize_from_u64(read_u64(reader)?)?;
    let play_bytes = usize_from_u64(read_u64(reader)?)?;

    if configuration_count != EXPECTED_CONFIGURATION_COUNT {
        return Err(R1xImageError::ConfigurationCount {
            observed: configuration_count,
        });
    }
    if configuration_bytes != EXPECTED_CONFIGURATION_BYTES {
        return Err(R1xImageError::ConfigurationBytes {
            observed: configuration_bytes,
        });
    }
    if play_count > MAX_PLAY_COUNT {
        return Err(R1xImageError::PlayCount {
            observed: play_count,
        });
    }
    if play_bytes > MAX_PLAY_BYTES {
        return Err(R1xImageError::PlayBytes {
            observed: play_bytes,
        });
    }

    let (configuration, observed_configuration_bytes) =
        read_bodies(reader, R1xImageSection::Configuration, configuration_count)?;
    if observed_configuration_bytes != configuration_bytes {
        return Err(R1xImageError::AggregateMismatch {
            section: R1xImageSection::Configuration,
            declared: configuration_bytes,
            observed: observed_configuration_bytes,
        });
    }

    let (play, observed_play_bytes) = read_bodies(reader, R1xImageSection::Play, play_count)?;
    if observed_play_bytes != play_bytes {
        return Err(R1xImageError::AggregateMismatch {
            section: R1xImageSection::Play,
            declared: play_bytes,
            observed: observed_play_bytes,
        });
    }

    let mut trailing = [0_u8; 1];
    match reader
        .read(&mut trailing)
        .map_err(|error| io_error(&error))?
    {
        0 => {}
        _ => return Err(R1xImageError::TrailingData),
    }

    Target26_2R1xContext::new(status_json.into(), configuration, play).map_err(Into::into)
}

fn read_bodies<R: Read>(
    reader: &mut R,
    section: R1xImageSection,
    count: usize,
) -> Result<LoadedBodies, R1xImageError> {
    let mut bodies = Vec::with_capacity(count);
    let mut total = 0_usize;
    for index in 0..count {
        let length = usize_from_u32(read_u32(reader)?)?;
        if length == 0 || length > MAX_BODY_BYTES {
            return Err(R1xImageError::BodyLength {
                section,
                index,
                observed: length,
            });
        }
        total = total
            .checked_add(length)
            .ok_or(R1xImageError::AggregateMismatch {
                section,
                declared: usize::MAX,
                observed: usize::MAX,
            })?;
        let mut body = vec![0_u8; length].into_boxed_slice();
        reader
            .read_exact(&mut body)
            .map_err(|error| io_error(&error))?;
        bodies.push(body);
    }
    Ok((bodies, total))
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, R1xImageError> {
    Ok(u32::from_le_bytes(read_array::<4, _>(reader)?))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, R1xImageError> {
    Ok(u64::from_le_bytes(read_array::<8, _>(reader)?))
}

fn read_array<const N: usize, R: Read>(reader: &mut R) -> Result<[u8; N], R1xImageError> {
    let mut bytes = [0_u8; N];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| io_error(&error))?;
    Ok(bytes)
}

fn usize_from_u32(value: u32) -> Result<usize, R1xImageError> {
    usize::try_from(value).map_err(|_| R1xImageError::FileTooLarge {
        observed: u64::from(value),
        maximum: usize::MAX as u64,
    })
}

fn usize_from_u64(value: u64) -> Result<usize, R1xImageError> {
    usize::try_from(value).map_err(|_| R1xImageError::FileTooLarge {
        observed: value,
        maximum: usize::MAX as u64,
    })
}

fn io_error(error: &io::Error) -> R1xImageError {
    R1xImageError::Io {
        kind: error.kind(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{
        EXPECTED_CAPTURE_SHA256, EXPECTED_CONFIGURATION_BYTES, EXPECTED_PROTOCOL,
        EXPECTED_SOURCE_SHA256, MAGIC, R1xImageError, decode_image,
    };

    const PRODUCT_BRAND_BODY: &[u8] = b"\x01\x0fminecraft:brand\x05Helve";
    const SIZES: [usize; 34] = [
        23, 20, 22, 1_612, 224, 327, 227, 184, 149, 77, 80, 78, 233, 66, 66, 77, 70, 81, 73, 980,
        282, 116, 1_143, 1_036, 968, 416, 237, 48, 49, 94, 64, 103, 35_204, 1,
    ];

    fn valid_image(play: &[&[u8]]) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(&MAGIC);
        output.extend_from_slice(&EXPECTED_PROTOCOL.to_le_bytes());
        output.extend_from_slice(&EXPECTED_SOURCE_SHA256);
        output.extend_from_slice(&EXPECTED_CAPTURE_SHA256);
        output.extend_from_slice(&34_u32.to_le_bytes());
        output.extend_from_slice(
            &u32::try_from(play.len())
                .expect("small play count")
                .to_le_bytes(),
        );
        output.extend_from_slice(
            &u64::try_from(EXPECTED_CONFIGURATION_BYTES)
                .expect("config bytes fit")
                .to_le_bytes(),
        );
        output.extend_from_slice(
            &u64::try_from(play.iter().map(|body| body.len()).sum::<usize>())
                .expect("play bytes fit")
                .to_le_bytes(),
        );

        for (index, size) in SIZES.into_iter().enumerate() {
            let packet_id = match index {
                0 => 1,
                1 => 12,
                2 => 14,
                3..=31 => 7,
                32 => 13,
                33 => 3,
                _ => unreachable!(),
            };
            output.extend_from_slice(&u32::try_from(size).expect("size fits").to_le_bytes());
            let mut body = if index == 0 {
                PRODUCT_BRAND_BODY.to_vec()
            } else {
                vec![0_u8; size]
            };
            body[0] = packet_id;
            output.extend_from_slice(&body);
        }
        for body in play {
            output.extend_from_slice(
                &u32::try_from(body.len())
                    .expect("body length fits")
                    .to_le_bytes(),
            );
            output.extend_from_slice(body);
        }
        output
    }

    #[test]
    fn compact_image_decodes_without_whole_file_copy() {
        let bytes = valid_image(&[&[0x10, 0xaa], &[0x11]]);
        let context = decode_image(&mut Cursor::new(bytes), "{}").expect("valid compact image");
        assert_eq!(context.play_frame_count(), 2);
        assert_eq!(context.play_body_bytes(), 3);
    }

    #[test]
    fn trailing_data_is_rejected() {
        let mut bytes = valid_image(&[]);
        bytes.push(0xff);
        assert!(matches!(
            decode_image(&mut Cursor::new(bytes), "{}"),
            Err(R1xImageError::TrailingData)
        ));
    }

    #[test]
    fn wrong_commitment_is_rejected_before_body_allocation() {
        let mut bytes = valid_image(&[]);
        bytes[12] ^= 0xff;
        assert!(matches!(
            decode_image(&mut Cursor::new(bytes), "{}"),
            Err(R1xImageError::SourceCommitment)
        ));
    }
}
