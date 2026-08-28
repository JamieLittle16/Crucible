//! R2C cold import -> resident-world qualification support.
//!
//! This crate is evidence infrastructure only. It deliberately uses the transparent direct section
//! oracle so the first end-to-end baseline has unambiguous semantics. Timing from this reference
//! builder must not select the production section mechanism; M0.3D remains the authority for that
//! decision.

#![forbid(unsafe_code)]

use std::time::Instant;

use helve_generated::{AIR, BlockStateId, GeneratedStateFacts};
use helve_types::{BlockPos, ChunkPos, DimensionId, DimensionTypeId};
use helve_world_contract::{BlockSection, SectionBlockPos};
use helve_world_import::{
    BlockProperty, BlockSectionDecodeScratch, BlockStateResolver, ChunkPayloadLimits,
    DeflateChunkPayloadDecoder, ImportedBlockSectionBuilder, NbtLimits, RegionLimits, RegionView,
    StoredBlockImporter, StoredChunkImportError, Target262BlockStateResolver,
};
use helve_world_load::{InstalledStoredChunk, ResidentInstallError, install_imported_chunk};
use helve_world_reference::DirectBlockSection;
use helve_world_runtime::{DimensionInstance, DimensionRuntimeProfile, ResidentChunkAccessError};
use miniz_oxide::deflate::compress_to_vec_zlib;

const REGION_HEADER_BYTES: usize = 8 * 1024;
const SECTOR_BYTES: usize = 4 * 1024;
const MAX_REGION_BYTES: usize = 64 * SECTOR_BYTES;
const MAX_INLINE_COMPRESSED_BYTES: usize = 4 * SECTOR_BYTES;
const MAX_EXTERNAL_COMPRESSED_BYTES: usize = 4 * SECTOR_BYTES;
const MAX_DECOMPRESSED_BYTES: usize = 256 * 1024;
const MAX_NBT_STRING_BYTES: usize = 8 * 1024;
const MAX_NBT_LIST_ELEMENTS: usize = 64 * 1024;
const MAX_NBT_ARRAY_ELEMENTS: usize = 512 * 1024;
const MAX_NBT_DEPTH: usize = 32;
const TARGET_SECTION_Y: i8 = 0;

/// Section type used by the transparent end-to-end baseline.
pub type ReferenceSection = DirectBlockSection<BlockStateId>;
/// Resident dimension type exercised by the baseline.
pub type ReferenceDimension = DimensionInstance<BlockStateId, ReferenceSection>;

/// Counts work introduced specifically by the transparent section builder.
///
/// These counters are structural evidence: they make it impossible to confuse direct-oracle cell
/// materialization with the importer parser or the sparse-to-contiguous resident installation seam.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BuilderCounters {
    /// Uniform section constructions.
    pub uniform_sections: u64,
    /// Dense section constructions.
    pub dense_sections: u64,
    /// Dense cell writes performed by the reference builder.
    pub dense_cell_writes: u64,
}

/// Transparent imported-section builder backed by [`DirectBlockSection`].
#[derive(Debug, Default)]
pub struct ReferenceSectionBuilder {
    counters: BuilderCounters,
}

impl ReferenceSectionBuilder {
    /// Current structural counters.
    #[must_use]
    pub const fn counters(&self) -> BuilderCounters {
        self.counters
    }
}

impl ImportedBlockSectionBuilder<BlockStateId> for ReferenceSectionBuilder {
    type Section = ReferenceSection;

    fn build_uniform(&mut self, state: BlockStateId) -> Self::Section {
        self.counters.uniform_sections += 1;
        DirectBlockSection::filled(state, &GeneratedStateFacts)
    }

    fn build_states(&mut self, states: &[BlockStateId]) -> Self::Section {
        self.counters.dense_sections += 1;
        self.counters.dense_cell_writes += u64::try_from(states.len()).unwrap_or(u64::MAX);
        let mut section = DirectBlockSection::filled(AIR, &GeneratedStateFacts);
        for (index, &state) in states.iter().enumerate() {
            let x = u8::try_from(index & 0x0f).unwrap_or_default();
            let z = u8::try_from((index >> 4) & 0x0f).unwrap_or_default();
            let y = u8::try_from((index >> 8) & 0x0f).unwrap_or_default();
            let Some(pos) = SectionBlockPos::new(x, y, z) else {
                continue;
            };
            section.replace(pos, state, &GeneratedStateFacts);
        }
        section
    }
}

/// Immutable synthetic zlib Anvil fixture for the exact 26.2 block-state import path.
#[derive(Clone, Debug)]
pub struct ColdPathFixture {
    region_bytes: Vec<u8>,
    expected_state: BlockStateId,
    position: ChunkPos,
    nbt_limits: NbtLimits,
    payload_limits: ChunkPayloadLimits,
    region_limits: RegionLimits,
}

impl ColdPathFixture {
    /// Builds a one-chunk zlib region containing one uniform stone section.
    ///
    /// The fixture is created outside measured regions. Production code under measurement begins at
    /// `RegionView::chunk`/`StoredBlockImporter` and ends at successful resident installation.
    ///
    /// # Errors
    ///
    /// Returns an error if the exact 26.2 resolver cannot resolve the fixture state or if the fixed
    /// qualification limits are internally invalid.
    pub fn new() -> Result<Self, String> {
        let resolver = Target262BlockStateResolver;
        let expected_state = resolver
            .resolve("minecraft:stone", &[] as &[BlockProperty<'_>])
            .ok_or_else(|| "target 26.2 resolver does not contain minecraft:stone".to_owned())?;
        let position = ChunkPos { x: 0, z: 0 };
        let nbt = uniform_chunk_nbt(position, "minecraft:stone");
        let compressed = compress_to_vec_zlib(&nbt, 6);
        if compressed.len() + 5 > SECTOR_BYTES {
            return Err("qualification fixture no longer fits one Anvil sector".to_owned());
        }
        let region_bytes = region_bytes(&compressed, 29)?;
        let nbt_limits = NbtLimits::new(
            MAX_NBT_STRING_BYTES,
            MAX_NBT_LIST_ELEMENTS,
            MAX_NBT_ARRAY_ELEMENTS,
            MAX_NBT_DEPTH,
        )
        .map_err(|error| format!("invalid NBT qualification limits: {error:?}"))?;
        Ok(Self {
            region_bytes,
            expected_state,
            position,
            nbt_limits,
            payload_limits: ChunkPayloadLimits::new(
                MAX_EXTERNAL_COMPRESSED_BYTES,
                MAX_DECOMPRESSED_BYTES,
            ),
            region_limits: RegionLimits::new(MAX_REGION_BYTES, MAX_INLINE_COMPRESSED_BYTES),
        })
    }

    /// Expected semantic state in the stored section.
    #[must_use]
    pub const fn expected_state(&self) -> BlockStateId {
        self.expected_state
    }

    /// Stored chunk position.
    #[must_use]
    pub const fn position(&self) -> ChunkPos {
        self.position
    }

    fn region(&self) -> Result<RegionView<'_>, String> {
        RegionView::new(&self.region_bytes, 0, 0, self.region_limits)
            .map_err(|error| format!("qualification region rejected: {error:?}"))
    }
}

/// Reusable cold-path mechanisms retained across benchmark samples.
#[derive(Debug)]
pub struct ColdPathSession {
    decoder: DeflateChunkPayloadDecoder,
    builder: ReferenceSectionBuilder,
    scratch: BlockSectionDecodeScratch<BlockStateId>,
}

impl ColdPathSession {
    /// Allocates the reusable decompression scratch once for the selected bounded profile.
    ///
    /// # Errors
    ///
    /// Returns an error if the bounded decoder scratch cannot be allocated.
    pub fn new() -> Result<Self, String> {
        let decoder = DeflateChunkPayloadDecoder::try_with_output_limit(MAX_DECOMPRESSED_BYTES)
            .map_err(|error| format!("could not allocate decoder scratch: {error:?}"))?;
        Ok(Self {
            decoder,
            builder: ReferenceSectionBuilder::default(),
            scratch: BlockSectionDecodeScratch::new(),
        })
    }

    /// Current transparent-builder counters.
    #[must_use]
    pub const fn builder_counters(&self) -> BuilderCounters {
        self.builder.counters()
    }

    fn import(
        &mut self,
        fixture: &ColdPathFixture,
    ) -> Result<helve_world_import::ImportedStoredChunk<ReferenceSection>, String> {
        let region = fixture.region()?;
        let resolver = Target262BlockStateResolver;
        StoredBlockImporter::new(
            fixture.payload_limits,
            fixture.nbt_limits,
            &mut self.decoder,
            &resolver,
            &mut self.builder,
            &mut self.scratch,
        )
        .import_region_chunk(&region, 0, 0, None)
        .map_err(|error| import_error(&error))
    }
}

/// One split timing sample of the real reference cold path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColdPathSample {
    /// Stored region -> decoded final semantic sections.
    pub import_ns: u128,
    /// Sparse imported sections -> live `DimensionInstance` residency.
    pub install_ns: u128,
    /// Import and install measured as one uninterrupted transaction.
    pub combined_ns: u128,
}

/// Builds the selected Overworld-style 24-section resident profile.
///
/// This intentionally preserves the real selected vertical span even though the fixture contains one
/// block-bearing section. Missing sections therefore exercise the caller-owned empty-section factory.
///
/// # Errors
///
/// Returns an error if the frozen qualification dimension lattice cannot be represented by the
/// resident-world profile contract.
pub fn reference_dimension() -> Result<ReferenceDimension, String> {
    let profile = DimensionRuntimeProfile::new(DimensionTypeId(0), -64, 384, true)
        .map_err(|error| format!("invalid qualification dimension profile: {error:?}"))?;
    Ok(DimensionInstance::new(DimensionId(0), profile))
}

/// Executes one split sample and validates the resident result before returning timing evidence.
///
/// Import and install split timings are taken from separate transactions so neither phase's timer
/// contains setup for the other. The combined timing is a third complete transaction. Unload and
/// correctness verification are outside every timer.
///
/// # Errors
///
/// Returns an error on import, installation, resident resolution, semantic mismatch or unload.
pub fn sample(
    fixture: &ColdPathFixture,
    session: &mut ColdPathSession,
    dimension: &mut ReferenceDimension,
) -> Result<ColdPathSample, String> {
    ensure_empty(dimension)?;

    let import_start = Instant::now();
    let imported_for_install = session.import(fixture)?;
    let import_ns = import_start.elapsed().as_nanos();

    let install_start = Instant::now();
    let installed = install(fixture, dimension, imported_for_install)?;
    let install_ns = install_start.elapsed().as_nanos();
    validate_and_unload(fixture, dimension, installed)?;

    let combined_start = Instant::now();
    let imported = session.import(fixture)?;
    let installed = install(fixture, dimension, imported)?;
    let combined_ns = combined_start.elapsed().as_nanos();
    validate_and_unload(fixture, dimension, installed)?;

    Ok(ColdPathSample {
        import_ns,
        install_ns,
        combined_ns,
    })
}

fn install(
    fixture: &ColdPathFixture,
    dimension: &mut ReferenceDimension,
    imported: helve_world_import::ImportedStoredChunk<ReferenceSection>,
) -> Result<InstalledStoredChunk, String> {
    install_imported_chunk(dimension, imported, || {
        DirectBlockSection::filled(AIR, &GeneratedStateFacts)
    })
    .map_err(install_error)
    .and_then(|installed| {
        if installed.handle.position != fixture.position {
            return Err("installed resident handle changed chunk identity".to_owned());
        }
        Ok(installed)
    })
}

fn validate_and_unload(
    fixture: &ColdPathFixture,
    dimension: &mut ReferenceDimension,
    installed: InstalledStoredChunk,
) -> Result<(), String> {
    let chunk = dimension
        .resolve_chunk(installed.handle)
        .map_err(access_error)?;
    let block = chunk
        .get_block(BlockPos { x: 0, y: 0, z: 0 })
        .map_err(|error| format!("resident block access failed: {error:?}"))?;
    if block != fixture.expected_state {
        return Err(format!(
            "resident semantic mismatch: expected {}, got {}",
            fixture.expected_state.as_usize(),
            block.as_usize()
        ));
    }
    if !chunk.masks_match_recomputation() {
        return Err("resident section masks disagree with independent recomputation".to_owned());
    }
    let _unloaded = dimension
        .unload_chunk(installed.handle)
        .map_err(access_error)?;
    Ok(())
}

fn ensure_empty(dimension: &ReferenceDimension) -> Result<(), String> {
    if dimension.resident_chunk_count() != 0 {
        return Err("cold-path sample requires an empty qualification dimension".to_owned());
    }
    Ok(())
}

fn import_error(
    error: &StoredChunkImportError<helve_world_import::CompressedPayloadError>,
) -> String {
    format!("stored-chunk import failed: {error:?}")
}

fn install_error(error: ResidentInstallError) -> String {
    format!("resident installation failed: {error:?}")
}

fn access_error(error: ResidentChunkAccessError) -> String {
    format!("resident access failed: {error:?}")
}

fn name(bytes: &mut Vec<u8>, value: &str) {
    let Ok(length) = u16::try_from(value.len()) else {
        return;
    };
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn named_header(bytes: &mut Vec<u8>, tag_type: u8, field: &str) {
    bytes.push(tag_type);
    name(bytes, field);
}

fn int_field(bytes: &mut Vec<u8>, field: &str, value: i32) {
    named_header(bytes, 3, field);
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn uniform_chunk_nbt(position: ChunkPos, state_name: &str) -> Vec<u8> {
    let mut bytes = vec![10, 0, 0];
    int_field(&mut bytes, "DataVersion", 4903);
    int_field(&mut bytes, "xPos", position.x);
    int_field(&mut bytes, "zPos", position.z);
    named_header(&mut bytes, 9, "sections");
    bytes.push(10);
    bytes.extend_from_slice(&1_i32.to_be_bytes());
    named_header(&mut bytes, 1, "Y");
    bytes.push(TARGET_SECTION_Y.to_ne_bytes()[0]);
    named_header(&mut bytes, 10, "block_states");
    named_header(&mut bytes, 9, "palette");
    bytes.push(10);
    bytes.extend_from_slice(&1_i32.to_be_bytes());
    named_header(&mut bytes, 8, "Name");
    name(&mut bytes, state_name);
    bytes.push(0);
    bytes.push(0);
    bytes.push(0);
    bytes.push(0);
    bytes
}

fn region_bytes(payload: &[u8], timestamp: u32) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
    let raw_location = (2_u32 << 8) | 1;
    bytes[..4].copy_from_slice(&raw_location.to_be_bytes());
    bytes[SECTOR_BYTES..SECTOR_BYTES + 4].copy_from_slice(&timestamp.to_be_bytes());
    let record_start = 2 * SECTOR_BYTES;
    let length = u32::try_from(payload.len() + 1)
        .map_err(|_| "qualification payload length exceeds u32".to_owned())?;
    bytes[record_start..record_start + 4].copy_from_slice(&length.to_be_bytes());
    bytes[record_start + 4] = 2;
    let payload_start = record_start + 5;
    bytes[payload_start..payload_start + payload.len()].copy_from_slice(payload);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use helve_generated::GeneratedStateFacts;
    use helve_world_contract::BlockStateFacts;

    use super::{ColdPathFixture, ColdPathSession, reference_dimension, sample};

    #[test]
    fn exact_zlib_import_installs_and_unloads_with_semantic_equality() {
        let fixture = ColdPathFixture::new().expect("valid qualification fixture");
        assert!(
            GeneratedStateFacts
                .facts(fixture.expected_state())
                .non_air()
        );
        let mut session = ColdPathSession::new().expect("bounded decoder scratch");
        let mut dimension = reference_dimension().expect("valid dimension");

        let evidence = sample(&fixture, &mut session, &mut dimension).expect("cold path sample");
        assert!(evidence.import_ns > 0);
        assert!(evidence.install_ns > 0);
        assert!(evidence.combined_ns > 0);
        assert_eq!(dimension.resident_chunk_count(), 0);
        let counters = session.builder_counters();
        assert_eq!(counters.uniform_sections, 2);
        assert_eq!(counters.dense_sections, 0);
        assert_eq!(counters.dense_cell_writes, 0);
    }

    #[test]
    fn fixture_is_pinned_to_origin_and_stone_identity() {
        let fixture = ColdPathFixture::new().expect("valid qualification fixture");
        assert_eq!(fixture.position(), helve_types::ChunkPos { x: 0, z: 0 });
        assert_ne!(fixture.expected_state(), helve_generated::AIR);
    }
}
