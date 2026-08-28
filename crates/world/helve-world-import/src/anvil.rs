//! Bounded region-file framing for cold pregenerated-world import.
//!
//! This module validates the Anvil location table and exposes compressed chunk records without
//! choosing a decompressor or constructing an intermediate world model.

use helve_types::ChunkPos;

/// Bytes in one Anvil sector.
pub const SECTOR_BYTES: usize = 4096;
/// Fixed region header size: location table plus timestamp table.
pub const REGION_HEADER_BYTES: usize = SECTOR_BYTES * 2;
const LOCATION_COUNT: usize = 1024;
const CHUNKS_PER_REGION_AXIS: i32 = 32;

/// Caller-selected cold-input bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionLimits {
    /// Largest region file accepted by this import profile.
    pub max_region_bytes: usize,
    /// Largest inline compressed chunk payload accepted before decompression.
    pub max_inline_chunk_payload_bytes: usize,
}

impl RegionLimits {
    /// Creates explicit import bounds. The caller owns profile selection.
    #[must_use]
    pub const fn new(max_region_bytes: usize, max_inline_chunk_payload_bytes: usize) -> Self {
        Self {
            max_region_bytes,
            max_inline_chunk_payload_bytes,
        }
    }
}

/// Compression identity encoded by an Anvil chunk record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkCompression {
    /// Gzip-compressed NBT payload.
    Gzip,
    /// Zlib/DEFLATE-compressed NBT payload.
    Zlib,
    /// Uncompressed NBT payload.
    Uncompressed,
    /// LZ4-compressed NBT payload.
    Lz4,
}

impl ChunkCompression {
    fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(Self::Gzip),
            2 => Some(Self::Zlib),
            3 => Some(Self::Uncompressed),
            4 => Some(Self::Lz4),
            _ => None,
        }
    }
}

/// One validated region chunk record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionChunk<'a> {
    /// Semantic chunk-column position implied by region coordinates and location-table slot.
    pub position: ChunkPos,
    /// Region timestamp-table value. This is metadata, not semantic world authority.
    pub timestamp: u32,
    /// Compression identity after stripping the external-payload flag.
    pub compression: ChunkCompression,
    /// Whether payload bytes live in the external `c.<x>.<z>.mcc` file.
    pub external: bool,
    /// Inline compressed payload when `external == false`.
    pub inline_payload: Option<&'a [u8]>,
}

/// Fail-closed Anvil framing errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionError {
    /// Region bytes exceed the caller's explicit cold-input limit.
    RegionExceedsLimit { actual: usize, limit: usize },
    /// Region data cannot contain both mandatory header sectors.
    RegionTooSmall { actual: usize },
    /// Region files are sector aligned.
    RegionNotSectorAligned { actual: usize },
    /// A location-table entry points into the header or outside the file.
    InvalidLocation {
        slot: usize,
        offset_sectors: usize,
        sector_count: usize,
        file_sectors: usize,
    },
    /// Two occupied location-table entries claim at least one common sector.
    OverlappingLocations {
        first_slot: usize,
        second_slot: usize,
    },
    /// Region-to-absolute chunk-coordinate arithmetic overflowed `i32`.
    ChunkPositionOverflow,
    /// Stored chunk length is empty or exceeds its allocated sectors.
    InvalidChunkLength {
        position: ChunkPos,
        length: usize,
        allocation_bytes: usize,
    },
    /// Inline compressed payload exceeds the caller-selected bound.
    InlineChunkExceedsLimit {
        position: ChunkPos,
        actual: usize,
        limit: usize,
    },
    /// Compression ID is not one of the Anvil schemes understood by this boundary.
    UnsupportedCompression { position: ChunkPos, id: u8 },
    /// Requested local region coordinates are outside `0..32`.
    LocalChunkOutsideRegion { local_x: u8, local_z: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Location {
    offset_sectors: usize,
    sector_count: usize,
}

impl Location {
    fn end_sector(self) -> usize {
        self.offset_sectors + self.sector_count
    }

    fn overlaps(self, other: Self) -> bool {
        self.offset_sectors < other.end_sector() && other.offset_sectors < self.end_sector()
    }
}

/// Validated zero-copy view over one already-bounded Anvil region file.
#[derive(Clone, Copy, Debug)]
pub struct RegionView<'a> {
    bytes: &'a [u8],
    region_x: i32,
    region_z: i32,
    limits: RegionLimits,
}

impl<'a> RegionView<'a> {
    /// Validates region framing and every location-table allocation.
    ///
    /// The caller must have bounded the file read before supplying `bytes`; `limits` makes that
    /// bound executable here as a second line of defence. Validation uses no heap allocation.
    ///
    /// # Errors
    ///
    /// Returns a fail-closed error for oversized, truncated, misaligned, overlapping, or otherwise
    /// invalid region framing.
    pub fn new(
        bytes: &'a [u8],
        region_x: i32,
        region_z: i32,
        limits: RegionLimits,
    ) -> Result<Self, RegionError> {
        if bytes.len() > limits.max_region_bytes {
            return Err(RegionError::RegionExceedsLimit {
                actual: bytes.len(),
                limit: limits.max_region_bytes,
            });
        }
        if bytes.len() < REGION_HEADER_BYTES {
            return Err(RegionError::RegionTooSmall {
                actual: bytes.len(),
            });
        }
        if !bytes.len().is_multiple_of(SECTOR_BYTES) {
            return Err(RegionError::RegionNotSectorAligned {
                actual: bytes.len(),
            });
        }

        let file_sectors = bytes.len() / SECTOR_BYTES;
        for slot in 0..LOCATION_COUNT {
            let Some(location) = parse_location(bytes, file_sectors, slot)? else {
                continue;
            };
            for first_slot in 0..slot {
                let Some(first) = parse_location(bytes, file_sectors, first_slot)? else {
                    continue;
                };
                if first.overlaps(location) {
                    return Err(RegionError::OverlappingLocations {
                        first_slot,
                        second_slot: slot,
                    });
                }
            }
        }

        Ok(Self {
            bytes,
            region_x,
            region_z,
            limits,
        })
    }

    /// Returns one occupied local region slot or `None` when the slot is empty.
    ///
    /// External records intentionally expose no bytes: the caller must perform a separately bounded
    /// read of `c.<x>.<z>.mcc` before decompression.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid local coordinates, malformed stored length, oversized inline
    /// payload, coordinate overflow, or unknown compression identity.
    pub fn chunk(&self, local_x: u8, local_z: u8) -> Result<Option<RegionChunk<'a>>, RegionError> {
        if local_x >= 32 || local_z >= 32 {
            return Err(RegionError::LocalChunkOutsideRegion { local_x, local_z });
        }
        let slot = usize::from(local_z) * 32 + usize::from(local_x);
        let file_sectors = self.bytes.len() / SECTOR_BYTES;
        let Some(location) = parse_location(self.bytes, file_sectors, slot)? else {
            return Ok(None);
        };
        let position = self.chunk_position(local_x, local_z)?;
        let start = location.offset_sectors * SECTOR_BYTES;
        let allocation_bytes = location.sector_count * SECTOR_BYTES;
        let length = usize::try_from(u32::from_be_bytes(
            self.bytes[start..start + 4]
                .try_into()
                .expect("validated sector contains four length bytes"),
        ))
        .expect("u32 chunk length fits usize on supported targets");
        if length < 1 || length > allocation_bytes - 4 {
            return Err(RegionError::InvalidChunkLength {
                position,
                length,
                allocation_bytes,
            });
        }

        let compression_byte = self.bytes[start + 4];
        let external = compression_byte & 0x80 != 0;
        let compression_id = compression_byte & 0x7f;
        let compression = ChunkCompression::from_id(compression_id).ok_or(
            RegionError::UnsupportedCompression {
                position,
                id: compression_id,
            },
        )?;

        let inline_payload = if external {
            None
        } else {
            let payload_len = length - 1;
            if payload_len > self.limits.max_inline_chunk_payload_bytes {
                return Err(RegionError::InlineChunkExceedsLimit {
                    position,
                    actual: payload_len,
                    limit: self.limits.max_inline_chunk_payload_bytes,
                });
            }
            let end = start + 4 + length;
            Some(&self.bytes[start + 5..end])
        };

        let timestamp_start = SECTOR_BYTES + slot * 4;
        let timestamp = u32::from_be_bytes(
            self.bytes[timestamp_start..timestamp_start + 4]
                .try_into()
                .expect("validated header contains timestamp bytes"),
        );
        Ok(Some(RegionChunk {
            position,
            timestamp,
            compression,
            external,
            inline_payload,
        }))
    }

    fn chunk_position(&self, local_x: u8, local_z: u8) -> Result<ChunkPos, RegionError> {
        let base_x = self
            .region_x
            .checked_mul(CHUNKS_PER_REGION_AXIS)
            .ok_or(RegionError::ChunkPositionOverflow)?;
        let base_z = self
            .region_z
            .checked_mul(CHUNKS_PER_REGION_AXIS)
            .ok_or(RegionError::ChunkPositionOverflow)?;
        let x = base_x
            .checked_add(i32::from(local_x))
            .ok_or(RegionError::ChunkPositionOverflow)?;
        let z = base_z
            .checked_add(i32::from(local_z))
            .ok_or(RegionError::ChunkPositionOverflow)?;
        Ok(ChunkPos { x, z })
    }
}

fn parse_location(
    bytes: &[u8],
    file_sectors: usize,
    slot: usize,
) -> Result<Option<Location>, RegionError> {
    let start = slot * 4;
    let raw = u32::from_be_bytes(
        bytes[start..start + 4]
            .try_into()
            .expect("validated region header contains location bytes"),
    );
    if raw == 0 {
        return Ok(None);
    }
    let offset_sectors = usize::try_from(raw >> 8).expect("24-bit offset fits usize");
    let sector_count = usize::try_from(raw & 0xff).expect("8-bit sector count fits usize");
    let valid = offset_sectors >= 2
        && sector_count > 0
        && offset_sectors
            .checked_add(sector_count)
            .is_some_and(|end| end <= file_sectors);
    if !valid {
        return Err(RegionError::InvalidLocation {
            slot,
            offset_sectors,
            sector_count,
            file_sectors,
        });
    }
    Ok(Some(Location {
        offset_sectors,
        sector_count,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        ChunkCompression, REGION_HEADER_BYTES, RegionError, RegionLimits, RegionView, SECTOR_BYTES,
    };
    use helve_types::ChunkPos;

    const LIMITS: RegionLimits = RegionLimits::new(64 * SECTOR_BYTES, 2 * SECTOR_BYTES);

    fn set_location(bytes: &mut [u8], slot: usize, offset: u32, count: u8) {
        let raw = (offset << 8) | u32::from(count);
        bytes[slot * 4..slot * 4 + 4].copy_from_slice(&raw.to_be_bytes());
    }

    fn write_chunk(bytes: &mut [u8], sector: usize, compression: u8, payload: &[u8]) {
        let start = sector * SECTOR_BYTES;
        let length = u32::try_from(payload.len() + 1).expect("test payload fits u32");
        bytes[start..start + 4].copy_from_slice(&length.to_be_bytes());
        bytes[start + 4] = compression;
        bytes[start + 5..start + 5 + payload.len()].copy_from_slice(payload);
    }

    #[test]
    fn validates_and_exposes_inline_chunk_without_copying_payload() {
        let mut bytes = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
        set_location(&mut bytes, 33, 2, 1);
        let timestamp_offset = SECTOR_BYTES + 33 * 4;
        bytes[timestamp_offset..timestamp_offset + 4].copy_from_slice(&123_u32.to_be_bytes());
        write_chunk(&mut bytes, 2, 3, b"nbt");

        let region = RegionView::new(&bytes, -1, 2, LIMITS).expect("valid region");
        let chunk = region.chunk(1, 1).expect("valid slot").expect("occupied");
        assert_eq!(chunk.position, ChunkPos { x: -31, z: 65 });
        assert_eq!(chunk.timestamp, 123);
        assert_eq!(chunk.compression, ChunkCompression::Uncompressed);
        assert!(!chunk.external);
        assert_eq!(chunk.inline_payload, Some(&b"nbt"[..]));
        assert!(region.chunk(0, 0).expect("valid empty slot").is_none());
    }

    #[test]
    fn rejects_overlapping_sector_allocations() {
        let mut bytes = vec![0_u8; REGION_HEADER_BYTES + 2 * SECTOR_BYTES];
        set_location(&mut bytes, 0, 2, 2);
        set_location(&mut bytes, 1, 3, 1);
        assert!(matches!(
            RegionView::new(&bytes, 0, 0, LIMITS),
            Err(RegionError::OverlappingLocations {
                first_slot: 0,
                second_slot: 1,
            })
        ));
    }

    #[test]
    fn rejects_header_locations_and_out_of_file_allocations() {
        let mut header = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
        set_location(&mut header, 0, 1, 1);
        assert!(matches!(
            RegionView::new(&header, 0, 0, LIMITS),
            Err(RegionError::InvalidLocation { slot: 0, .. })
        ));

        let mut beyond = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
        set_location(&mut beyond, 0, 2, 2);
        assert!(matches!(
            RegionView::new(&beyond, 0, 0, LIMITS),
            Err(RegionError::InvalidLocation { slot: 0, .. })
        ));
    }

    #[test]
    fn rejects_invalid_lengths_and_unknown_compression() {
        let mut bytes = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
        set_location(&mut bytes, 0, 2, 1);
        let region = RegionView::new(&bytes, 0, 0, LIMITS).expect("framing is valid");
        assert!(matches!(
            region.chunk(0, 0),
            Err(RegionError::InvalidChunkLength { .. })
        ));

        write_chunk(&mut bytes, 2, 17, b"x");
        let region = RegionView::new(&bytes, 0, 0, LIMITS).expect("framing is valid");
        assert_eq!(
            region.chunk(0, 0),
            Err(RegionError::UnsupportedCompression {
                position: ChunkPos { x: 0, z: 0 },
                id: 17,
            })
        );
    }

    #[test]
    fn external_chunk_records_never_alias_inline_padding_as_payload() {
        let mut bytes = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
        set_location(&mut bytes, 0, 2, 1);
        write_chunk(&mut bytes, 2, 0x80 | 2, b"ignored-inline-padding");
        let region = RegionView::new(&bytes, 0, 0, LIMITS).expect("valid region");
        let chunk = region.chunk(0, 0).expect("valid slot").expect("occupied");
        assert!(chunk.external);
        assert_eq!(chunk.compression, ChunkCompression::Zlib);
        assert_eq!(chunk.inline_payload, None);
    }

    #[test]
    fn explicit_bounds_fail_closed() {
        let bytes = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
        assert!(matches!(
            RegionView::new(
                &bytes,
                0,
                0,
                RegionLimits::new(REGION_HEADER_BYTES, SECTOR_BYTES),
            ),
            Err(RegionError::RegionExceedsLimit {
                actual,
                limit: REGION_HEADER_BYTES,
            }) if actual == REGION_HEADER_BYTES + SECTOR_BYTES
        ));

        let short = vec![0_u8; REGION_HEADER_BYTES - 1];
        assert!(matches!(
            RegionView::new(&short, 0, 0, LIMITS),
            Err(RegionError::RegionTooSmall { actual }) if actual == REGION_HEADER_BYTES - 1
        ));
    }

    #[test]
    fn local_coordinates_are_checked_before_slot_math() {
        let bytes = vec![0_u8; REGION_HEADER_BYTES];
        let region = RegionView::new(&bytes, 0, 0, LIMITS).expect("empty region is valid");
        assert_eq!(
            region.chunk(32, 0),
            Err(RegionError::LocalChunkOutsideRegion {
                local_x: 32,
                local_z: 0,
            })
        );
    }
}
