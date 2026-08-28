//! End-to-end cold stored-chunk transaction from validated Anvil framing to semantic block sections.
//!
//! Compression remains an injected static mechanism. The core importer therefore stays hermetic and
//! dependency-free while still owning the complete transaction shape. Uncompressed Anvil records are
//! supported directly; a future evidence-qualified gzip/zlib implementation can implement the same
//! decoder trait without changing NBT/schema logic, block-state resolution, section construction, or
//! resident-world APIs.

use helve_types::ChunkPos;

use crate::{
    anvil::{ChunkCompression, RegionError, RegionView},
    nbt::NbtLimits,
    stored_blocks::{
        BlockSectionDecodeScratch, BlockSectionImportError, BlockStateResolver,
        ImportedBlockSectionBuilder, ImportedChunkBlocks, decode_chunk_block_sections,
    },
};

/// Explicit payload bounds owned by one cold import profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChunkPayloadLimits {
    /// Largest separately read external `.mcc` compressed payload accepted by this profile.
    pub max_external_payload_bytes: usize,
    /// Largest decompressed NBT payload accepted for one chunk.
    pub max_decompressed_bytes: usize,
}

impl ChunkPayloadLimits {
    /// Creates caller-selected payload limits.
    #[must_use]
    pub const fn new(max_external_payload_bytes: usize, max_decompressed_bytes: usize) -> Self {
        Self {
            max_external_payload_bytes,
            max_decompressed_bytes,
        }
    }
}

/// Static decompression boundary used by the stored-chunk transaction.
///
/// Implementations may borrow uncompressed input directly or return bytes backed by reusable decoder
/// scratch. The common lifetime permits both without forcing `Cow`, boxing, or an intermediate object.
pub trait ChunkPayloadDecoder {
    /// Decoder-specific fail-closed error.
    type Error;

    /// Produces bounded decompressed NBT for one Anvil compression identity.
    ///
    /// # Errors
    ///
    /// Returns a decoder-specific error for unsupported/malformed compression, output-limit failure,
    /// checksum disagreement, trailing bytes, or any other mechanism-specific admission failure.
    fn decode<'a>(
        &'a mut self,
        compression: ChunkCompression,
        payload: &'a [u8],
        max_decompressed_bytes: usize,
    ) -> Result<&'a [u8], Self::Error>;
}

/// Dependency-free decoder for the Anvil uncompressed identity only.
///
/// This is a real selected profile for deterministic/synthetic import qualification, not a fallback:
/// gzip/zlib/LZ4 fail closed until a separately qualified decoder is supplied.
#[derive(Clone, Copy, Debug, Default)]
pub struct UncompressedChunkPayloadDecoder;

/// Errors from [`UncompressedChunkPayloadDecoder`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UncompressedPayloadError {
    /// The record uses a compression identity this decoder intentionally does not admit.
    CompressionNotAdmitted { compression: ChunkCompression },
    /// Borrowed raw NBT exceeds the selected decompressed bound.
    PayloadExceedsLimit { actual: usize, limit: usize },
}

impl ChunkPayloadDecoder for UncompressedChunkPayloadDecoder {
    type Error = UncompressedPayloadError;

    fn decode<'a>(
        &'a mut self,
        compression: ChunkCompression,
        payload: &'a [u8],
        max_decompressed_bytes: usize,
    ) -> Result<&'a [u8], Self::Error> {
        if compression != ChunkCompression::Uncompressed {
            return Err(UncompressedPayloadError::CompressionNotAdmitted { compression });
        }
        if payload.len() > max_decompressed_bytes {
            return Err(UncompressedPayloadError::PayloadExceedsLimit {
                actual: payload.len(),
                limit: max_decompressed_bytes,
            });
        }
        Ok(payload)
    }
}

/// One separately bounded external Anvil chunk payload.
///
/// External-file naming/path resolution and file I/O remain outside the parser. This wrapper records
/// only the bytes that were read under caller-owned bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalChunkPayload<'a> {
    /// Exact bytes from `c.<x>.<z>.mcc` after the caller's bounded file read.
    pub bytes: &'a [u8],
}

/// Metadata retained from the persisted transaction without becoming live world authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredChunkSourceMetadata {
    /// Region timestamp-table value for diagnostics/evidence only.
    pub region_timestamp: u32,
    /// Compression identity selected by Anvil framing.
    pub compression: ChunkCompression,
    /// Whether source bytes came from a separately read external `.mcc` payload.
    pub external: bool,
}

/// Successful cold transaction result before resident installation.
#[derive(Debug, Eq, PartialEq)]
pub struct ImportedStoredChunk<Section> {
    /// Exact semantic block-section result.
    pub blocks: ImportedChunkBlocks<Section>,
    /// Non-authoritative persisted-source metadata useful for qualification/diagnostics.
    pub source: StoredChunkSourceMetadata,
}

/// Fail-closed errors spanning the complete persisted block import transaction.
#[derive(Debug, Eq, PartialEq)]
pub enum StoredChunkImportError<DecodeError> {
    /// Region framing or region-slot access failed.
    Region(RegionError),
    /// Requested local region slot is empty.
    EmptyRegionSlot { local_x: u8, local_z: u8 },
    /// An external Anvil record requires separately bounded `.mcc` bytes.
    MissingExternalPayload { position: ChunkPos },
    /// External bytes exceed the profile bound before the decoder sees them.
    ExternalPayloadExceedsLimit {
        position: ChunkPos,
        actual: usize,
        limit: usize,
    },
    /// Compression/decompression mechanism rejected the stored payload.
    Decode(DecodeError),
    /// Bounded NBT or exact semantic block-section decoding failed.
    Blocks(BlockSectionImportError),
}

impl<DecodeError> From<RegionError> for StoredChunkImportError<DecodeError> {
    fn from(value: RegionError) -> Self {
        Self::Region(value)
    }
}

impl<DecodeError> From<BlockSectionImportError> for StoredChunkImportError<DecodeError> {
    fn from(value: BlockSectionImportError) -> Self {
        Self::Blocks(value)
    }
}

/// Imports one local region slot through the complete persisted block transaction.
///
/// The caller supplies external bytes only when the Anvil record carries the external flag. Inline
/// records ignore `external_payload`. Semantic section objects are returned uninstalled so any failure
/// remains transactional and cannot partially mutate resident world authority.
///
/// # Errors
///
/// Returns an explicit error for empty/malformed region framing, missing/oversized external payload,
/// decoder failure, bounded NBT failure, target/version/coordinate mismatch, or invalid block-state
/// palette/data semantics.
pub fn import_region_chunk_blocks<R, B, D>(
    region: &RegionView<'_>,
    local_x: u8,
    local_z: u8,
    external_payload: Option<ExternalChunkPayload<'_>>,
    payload_limits: ChunkPayloadLimits,
    nbt_limits: NbtLimits,
    decoder: &mut D,
    resolver: &R,
    builder: &mut B,
    section_scratch: &mut BlockSectionDecodeScratch<R::State>,
) -> Result<ImportedStoredChunk<B::Section>, StoredChunkImportError<D::Error>>
where
    R: BlockStateResolver,
    B: ImportedBlockSectionBuilder<R::State>,
    D: ChunkPayloadDecoder,
{
    let Some(chunk) = region.chunk(local_x, local_z)? else {
        return Err(StoredChunkImportError::EmptyRegionSlot { local_x, local_z });
    };

    let payload = if chunk.external {
        let external_payload = external_payload.ok_or(
            StoredChunkImportError::MissingExternalPayload {
                position: chunk.position,
            },
        )?;
        if external_payload.bytes.len() > payload_limits.max_external_payload_bytes {
            return Err(StoredChunkImportError::ExternalPayloadExceedsLimit {
                position: chunk.position,
                actual: external_payload.bytes.len(),
                limit: payload_limits.max_external_payload_bytes,
            });
        }
        external_payload.bytes
    } else {
        debug_assert!(chunk.inline_payload.is_some());
        chunk.inline_payload.unwrap_or_default()
    };

    let decompressed = decoder
        .decode(
            chunk.compression,
            payload,
            payload_limits.max_decompressed_bytes,
        )
        .map_err(StoredChunkImportError::Decode)?;
    let blocks = decode_chunk_block_sections(
        decompressed,
        chunk.position,
        nbt_limits,
        resolver,
        builder,
        section_scratch,
    )?;

    Ok(ImportedStoredChunk {
        blocks,
        source: StoredChunkSourceMetadata {
            region_timestamp: chunk.timestamp,
            compression: chunk.compression,
            external: chunk.external,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ChunkPayloadDecoder, ChunkPayloadLimits, ExternalChunkPayload, ImportedStoredChunk,
        StoredChunkImportError, StoredChunkSourceMetadata, UncompressedChunkPayloadDecoder,
        UncompressedPayloadError, import_region_chunk_blocks,
    };
    use crate::{
        anvil::{
            ChunkCompression, REGION_HEADER_BYTES, RegionLimits, RegionView, SECTOR_BYTES,
        },
        nbt::{NbtLimits, TagType},
        stored_blocks::{
            BlockProperty, BlockSectionDecodeScratch, BlockStateResolver,
            ImportedBlockSectionBuilder,
        },
    };
    use helve_types::ChunkPos;

    const REGION_LIMITS: RegionLimits = RegionLimits::new(64 * SECTOR_BYTES, 4 * SECTOR_BYTES);
    const PAYLOAD_LIMITS: ChunkPayloadLimits = ChunkPayloadLimits::new(4 * SECTOR_BYTES, 64 * 1024);

    struct Resolver;

    impl BlockStateResolver for Resolver {
        type State = u16;

        fn resolve(&self, name: &str, properties: &[BlockProperty<'_>]) -> Option<Self::State> {
            match (name, properties) {
                ("minecraft:air", []) => Some(0),
                ("minecraft:stone", []) => Some(1),
                _ => None,
            }
        }
    }

    struct VecBuilder;

    impl ImportedBlockSectionBuilder<u16> for VecBuilder {
        type Section = Vec<u16>;

        fn build_uniform(&mut self, state: u16) -> Self::Section {
            vec![state; 4096]
        }

        fn build_states(&mut self, states: &[u16]) -> Self::Section {
            states.to_vec()
        }
    }

    fn nbt_limits() -> NbtLimits {
        NbtLimits::new(256, 4096, 1024, 16).expect("valid test NBT limits")
    }

    fn name(bytes: &mut Vec<u8>, value: &str) {
        let length = u16::try_from(value.len()).expect("test name fits u16");
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn named_header(bytes: &mut Vec<u8>, tag_type: TagType, field: &str) {
        bytes.push(tag_type as u8);
        name(bytes, field);
    }

    fn int_field(bytes: &mut Vec<u8>, field: &str, value: i32) {
        named_header(bytes, TagType::Int, field);
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn chunk_nbt(position: ChunkPos, state_name: &str) -> Vec<u8> {
        let mut bytes = vec![TagType::Compound as u8, 0, 0];
        int_field(&mut bytes, "DataVersion", 4903);
        int_field(&mut bytes, "xPos", position.x);
        int_field(&mut bytes, "zPos", position.z);
        named_header(&mut bytes, TagType::List, "sections");
        bytes.push(TagType::Compound as u8);
        bytes.extend_from_slice(&1_i32.to_be_bytes());
        named_header(&mut bytes, TagType::Byte, "Y");
        bytes.push(0);
        named_header(&mut bytes, TagType::Compound, "block_states");
        named_header(&mut bytes, TagType::List, "palette");
        bytes.push(TagType::Compound as u8);
        bytes.extend_from_slice(&1_i32.to_be_bytes());
        named_header(&mut bytes, TagType::String, "Name");
        name(&mut bytes, state_name);
        bytes.push(TagType::End as u8); // palette entry
        bytes.push(TagType::End as u8); // block_states
        bytes.push(TagType::End as u8); // section
        bytes.push(TagType::End as u8); // root
        bytes
    }

    fn set_location(bytes: &mut [u8], slot: usize, offset: u32, count: u8) {
        let raw = (offset << 8) | u32::from(count);
        bytes[slot * 4..slot * 4 + 4].copy_from_slice(&raw.to_be_bytes());
    }

    fn write_record(bytes: &mut [u8], compression: u8, payload: &[u8]) {
        let start = 2 * SECTOR_BYTES;
        let length = u32::try_from(payload.len() + 1).expect("test payload length fits u32");
        bytes[start..start + 4].copy_from_slice(&length.to_be_bytes());
        bytes[start + 4] = compression;
        bytes[start + 5..start + 5 + payload.len()].copy_from_slice(payload);
    }

    fn region_bytes(compression: u8, inline_payload: &[u8], timestamp: u32) -> Vec<u8> {
        let mut bytes = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
        set_location(&mut bytes, 0, 2, 1);
        bytes[SECTOR_BYTES..SECTOR_BYTES + 4].copy_from_slice(&timestamp.to_be_bytes());
        write_record(&mut bytes, compression, inline_payload);
        bytes
    }

    #[test]
    fn uncompressed_region_to_semantic_section_is_one_transaction() {
        let position = ChunkPos { x: 0, z: 0 };
        let nbt = chunk_nbt(position, "minecraft:stone");
        let bytes = region_bytes(3, &nbt, 77);
        let region = RegionView::new(&bytes, 0, 0, REGION_LIMITS).expect("valid region");
        let result = import_region_chunk_blocks(
            &region,
            0,
            0,
            None,
            PAYLOAD_LIMITS,
            nbt_limits(),
            &mut UncompressedChunkPayloadDecoder,
            &Resolver,
            &mut VecBuilder,
            &mut BlockSectionDecodeScratch::new(),
        )
        .expect("complete uncompressed import");

        assert_eq!(result.blocks.header.position, position);
        assert_eq!(result.blocks.header.data_version, 4903);
        assert_eq!(result.blocks.header.stored_section_count, 1);
        assert_eq!(result.blocks.sections.len(), 1);
        assert!(result.blocks.sections[0].section.iter().all(|&state| state == 1));
        assert_eq!(
            result.source,
            StoredChunkSourceMetadata {
                region_timestamp: 77,
                compression: ChunkCompression::Uncompressed,
                external: false,
            }
        );
    }

    #[test]
    fn external_record_uses_separately_bounded_payload_without_aliasing_region_padding() {
        let position = ChunkPos { x: 0, z: 0 };
        let nbt = chunk_nbt(position, "minecraft:air");
        let bytes = region_bytes(0x80 | 3, b"inline padding is not payload", 9);
        let region = RegionView::new(&bytes, 0, 0, REGION_LIMITS).expect("valid external record");
        let result = import_region_chunk_blocks(
            &region,
            0,
            0,
            Some(ExternalChunkPayload { bytes: &nbt }),
            PAYLOAD_LIMITS,
            nbt_limits(),
            &mut UncompressedChunkPayloadDecoder,
            &Resolver,
            &mut VecBuilder,
            &mut BlockSectionDecodeScratch::new(),
        )
        .expect("external payload imported");
        assert!(result.blocks.sections[0].section.iter().all(|&state| state == 0));
        assert!(result.source.external);
    }

    #[test]
    fn missing_and_oversized_external_payloads_fail_before_semantic_decode() {
        let bytes = region_bytes(0x80 | 3, b"padding", 0);
        let region = RegionView::new(&bytes, 0, 0, REGION_LIMITS).expect("valid external record");
        let missing = import_region_chunk_blocks(
            &region,
            0,
            0,
            None,
            PAYLOAD_LIMITS,
            nbt_limits(),
            &mut UncompressedChunkPayloadDecoder,
            &Resolver,
            &mut VecBuilder,
            &mut BlockSectionDecodeScratch::new(),
        );
        assert_eq!(
            missing,
            Err(StoredChunkImportError::MissingExternalPayload {
                position: ChunkPos { x: 0, z: 0 },
            })
        );

        let external = [0_u8; 9];
        let oversized = import_region_chunk_blocks(
            &region,
            0,
            0,
            Some(ExternalChunkPayload { bytes: &external }),
            ChunkPayloadLimits::new(8, 64),
            nbt_limits(),
            &mut UncompressedChunkPayloadDecoder,
            &Resolver,
            &mut VecBuilder,
            &mut BlockSectionDecodeScratch::new(),
        );
        assert_eq!(
            oversized,
            Err(StoredChunkImportError::ExternalPayloadExceedsLimit {
                position: ChunkPos { x: 0, z: 0 },
                actual: 9,
                limit: 8,
            })
        );
    }

    #[test]
    fn compression_identity_is_passed_to_static_decoder_and_can_fail_closed() {
        let position = ChunkPos { x: 0, z: 0 };
        let nbt = chunk_nbt(position, "minecraft:air");
        let bytes = region_bytes(2, &nbt, 0);
        let region = RegionView::new(&bytes, 0, 0, REGION_LIMITS).expect("valid zlib framing");
        let result = import_region_chunk_blocks(
            &region,
            0,
            0,
            None,
            PAYLOAD_LIMITS,
            nbt_limits(),
            &mut UncompressedChunkPayloadDecoder,
            &Resolver,
            &mut VecBuilder,
            &mut BlockSectionDecodeScratch::new(),
        );
        assert_eq!(
            result,
            Err(StoredChunkImportError::Decode(
                UncompressedPayloadError::CompressionNotAdmitted {
                    compression: ChunkCompression::Zlib,
                }
            ))
        );
    }

    #[derive(Default)]
    struct RecordingDecoder {
        observed: Option<ChunkCompression>,
    }

    impl ChunkPayloadDecoder for RecordingDecoder {
        type Error = core::convert::Infallible;

        fn decode<'a>(
            &'a mut self,
            compression: ChunkCompression,
            payload: &'a [u8],
            _max_decompressed_bytes: usize,
        ) -> Result<&'a [u8], Self::Error> {
            self.observed = Some(compression);
            Ok(payload)
        }
    }

    #[test]
    fn decoder_mechanism_is_static_and_does_not_change_semantic_transaction() {
        let position = ChunkPos { x: 0, z: 0 };
        let nbt = chunk_nbt(position, "minecraft:stone");
        let bytes = region_bytes(2, &nbt, 0);
        let region = RegionView::new(&bytes, 0, 0, REGION_LIMITS).expect("valid region framing");
        let mut decoder = RecordingDecoder::default();
        let result: ImportedStoredChunk<Vec<u16>> = import_region_chunk_blocks(
            &region,
            0,
            0,
            None,
            PAYLOAD_LIMITS,
            nbt_limits(),
            &mut decoder,
            &Resolver,
            &mut VecBuilder,
            &mut BlockSectionDecodeScratch::new(),
        )
        .expect("injected decoder transaction");
        assert_eq!(decoder.observed, Some(ChunkCompression::Zlib));
        assert!(result.blocks.sections[0].section.iter().all(|&state| state == 1));
    }
}
