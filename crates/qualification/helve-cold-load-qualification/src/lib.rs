//! Qualification harness for the complete cold stored-chunk -> resident-chunk transaction.
//!
//! This crate is evidence infrastructure only. It intentionally uses a transparent dense final
//! section so the benchmark does not select a production section mechanism by convenience. The
//! measured transaction starts from already-resident region bytes and includes Anvil validation,
//! bounded decompression, exact 26.2 state resolution, section construction, sparse-to-contiguous
//! composition, and `DimensionInstance` admission. Filesystem I/O is deliberately outside scope.

#![forbid(unsafe_code)]

use std::hint::black_box;
use std::time::Instant;

use helve_generated::{AIR, BlockStateId, GeneratedStateFacts};
use helve_types::{BlockPos, DimensionId, DimensionTypeId};
use helve_world_contract::{
    BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionBlockPos, SectionSummary,
};
use helve_world_import::{
    BlockSectionDecodeScratch, BlockSectionScratchCapacities, DeflateChunkPayloadDecoder,
    ImportedBlockSectionBuilder, NbtLimits, RegionLimits, RegionView, StoredBlockImporter,
    Target262BlockStateResolver, resolve_target_26_2_block_state,
};
use helve_world_load::install_imported_chunk;
use helve_world_runtime::{DimensionInstance, DimensionRuntimeProfile};

const SECTOR_BYTES: usize = 4096;
const REGION_HEADER_BYTES: usize = SECTOR_BYTES * 2;
const REGION_BYTES: usize = REGION_HEADER_BYTES + SECTOR_BYTES;
const MAX_DECOMPRESSED_BYTES: usize = 16 * 1024;
const STORED_SECTION_COUNT: usize = 24;
const IMPORTED_SECTION_COUNT: usize = 18;
const UNIFORM_SECTION_COUNT: usize = 12;
const DENSE_SECTION_COUNT: usize = 6;
const OMITTED_SECTION_COUNT: usize = 6;
const DENSE_CELL_COPIES: usize = DENSE_SECTION_COUNT * BLOCK_SECTION_CELLS;

/// SHA-256 of the decompressed deterministic qualification NBT fixture.
pub const FIXTURE_NBT_SHA256: &str =
    "34060101891b2869f1518a7fb52a74ee3d5aa7b49146c470343966f00b25be53";
/// SHA-256 of the exact zlib payload embedded below.
pub const FIXTURE_ZLIB_SHA256: &str =
    "f36e5862f610a84a7007046c46ade1956dec1c449375c59579d6af946bbc11ac";
/// Exact decompressed NBT byte count.
pub const FIXTURE_DECOMPRESSED_BYTES: usize = 13_848;
/// Exact compressed zlib byte count.
pub const FIXTURE_COMPRESSED_BYTES: usize = ZLIB_CHUNK.len();

const ZLIB_CHUNK: [u8; 367] = [
    0x78, 0x9c, 0xed, 0x96, 0x3b, 0x4f, 0xc3, 0x30, 0x14, 0x85, 0x6f, 0x1e, 0xd0, 0x24, 0x4d, 0x1f,
    0xb4, 0x0b, 0x6b, 0x27, 0xfe, 0x03, 0x33, 0x33, 0x62, 0x42, 0x62, 0x42, 0x26, 0x35, 0x52, 0xd4,
    0x36, 0x41, 0xb1, 0x07, 0xc4, 0x6f, 0xe7, 0x95, 0xd8, 0x81, 0x4a, 0x96, 0xe8, 0x80, 0xa5, 0xeb,
    0xe5, 0x48, 0xd5, 0x4d, 0xea, 0xe4, 0x2a, 0xe7, 0x7c, 0xd3, 0x57, 0x10, 0x25, 0x34, 0xbd, 0x11,
    0x5a, 0xdc, 0xcb, 0x4e, 0xd5, 0x6d, 0x43, 0xb4, 0xbe, 0x4a, 0x28, 0x7d, 0xbd, 0x6b, 0x15, 0xd1,
    0xf0, 0x30, 0x7d, 0x1b, 0x6f, 0x73, 0xca, 0x94, 0xac, 0x74, 0xff, 0x8e, 0x2a, 0xfa, 0xbf, 0x97,
    0x11, 0x45, 0x0f, 0xef, 0x34, 0xcc, 0x8f, 0x82, 0xca, 0xa7, 0x7d, 0x5b, 0xed, 0x1e, 0x95, 0x16,
    0x5a, 0xaa, 0x9c, 0x26, 0x2f, 0x62, 0x2f, 0xb5, 0x96, 0xc3, 0x8b, 0x49, 0x46, 0xe9, 0xad, 0x38,
    0x48, 0x9a, 0x1d, 0xea, 0x46, 0x56, 0x9d, 0x78, 0xd6, 0xd7, 0xa2, 0xee, 0xe8, 0xe7, 0x78, 0x71,
    0x3c, 0x56, 0xba, 0x6d, 0xe4, 0xef, 0x83, 0xf9, 0xf1, 0xc1, 0xb6, 0xee, 0x34, 0x95, 0x94, 0x6e,
    0xfb, 0xa0, 0xd4, 0x7f, 0x33, 0x5e, 0x6e, 0xcc, 0x6f, 0xbc, 0x6c, 0xc6, 0x4b, 0xcc, 0x75, 0xce,
    0xfd, 0x3d, 0xf7, 0x3c, 0x54, 0x6f, 0xa7, 0x3e, 0xf8, 0x83, 0x3f, 0xf8, 0x83, 0x3f, 0xf8, 0x83,
    0x3f, 0x6b, 0x0e, 0x32, 0xde, 0xf1, 0x79, 0xd2, 0x3b, 0xa2, 0x3f, 0x05, 0xc3, 0x6e, 0x7f, 0x79,
    0x6d, 0xdb, 0x19, 0xc1, 0x7c, 0x98, 0x73, 0x84, 0xea, 0xed, 0xd4, 0x07, 0x7f, 0xf0, 0x07, 0x7f,
    0xf0, 0x07, 0x7f, 0xf0, 0x67, 0xcd, 0x61, 0xbd, 0x23, 0xf6, 0x72, 0x97, 0xc4, 0x6b, 0x3b, 0x35,
    0xf3, 0x0c, 0xe6, 0xc3, 0x9c, 0x23, 0x54, 0x6f, 0xa7, 0x3e, 0xf8, 0x83, 0x3f, 0xf8, 0x83, 0x3f,
    0xf8, 0x83, 0x3f, 0x6b, 0x0e, 0x6b, 0x1f, 0xe7, 0x5e, 0xee, 0x32, 0xf1, 0xda, 0xce, 0xcc, 0xcc,
    0x61, 0x3e, 0xcc, 0x39, 0x42, 0xf5, 0x76, 0xea, 0x83, 0x3f, 0xf8, 0x83, 0x3f, 0xf8, 0x83, 0x3f,
    0xf8, 0xb3, 0xe6, 0xb0, 0xf6, 0x51, 0x78, 0xb9, 0xcb, 0xd4, 0x6b, 0xbb, 0x34, 0x73, 0x06, 0xf3,
    0x61, 0xce, 0x11, 0xaa, 0xb7, 0x53, 0x1f, 0xfc, 0xc1, 0x1f, 0xfc, 0xc1, 0x1f, 0xfc, 0xc1, 0x9f,
    0x35, 0x87, 0xb5, 0x8f, 0xb9, 0x97, 0xbb, 0x2c, 0xbc, 0xb6, 0x97, 0x66, 0x5e, 0xc0, 0x7c, 0x98,
    0x73, 0x84, 0xea, 0xed, 0xd4, 0x07, 0x7f, 0xf0, 0x07, 0x7f, 0xf0, 0x07, 0x7f, 0xf0, 0x67, 0xcd,
    0x61, 0xed, 0x63, 0xe5, 0xe5, 0x2e, 0xeb, 0x7f, 0x6f, 0xd3, 0x37, 0x41, 0x83, 0xc4, 0x20,
];

/// Structural work performed by the qualification section builder for one imported chunk.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BuildCounters {
    pub uniform_sections: usize,
    pub dense_sections: usize,
    pub dense_semantic_cell_copies: usize,
    pub synthesized_empty_sections: usize,
}

/// Transparent dense final section used only by this qualification harness.
#[derive(Clone, Debug)]
pub struct QualificationBlockSection {
    cells: Box<[BlockStateId; BLOCK_SECTION_CELLS]>,
    summary: SectionSummary,
}

impl QualificationBlockSection {
    fn filled(state: BlockStateId) -> Self {
        let facts = GeneratedStateFacts;
        Self {
            cells: Box::new([state; BLOCK_SECTION_CELLS]),
            summary: summary_for_repeated(state, &facts),
        }
    }

    fn from_states(states: &[BlockStateId]) -> Self {
        debug_assert_eq!(states.len(), BLOCK_SECTION_CELLS);
        let mut cells = Box::new([AIR; BLOCK_SECTION_CELLS]);
        cells.copy_from_slice(states);
        let facts = GeneratedStateFacts;
        let summary = summarize(cells.as_slice(), &facts);
        Self { cells, summary }
    }
}

impl BlockSection<BlockStateId> for QualificationBlockSection {
    #[inline]
    fn get(&self, pos: SectionBlockPos) -> BlockStateId {
        self.cells[pos.index()]
    }

    fn replace<F: BlockStateFacts<BlockStateId>>(
        &mut self,
        pos: SectionBlockPos,
        state: BlockStateId,
        facts: &F,
    ) -> BlockStateId {
        let previous = self.cells[pos.index()];
        if previous != state {
            self.cells[pos.index()] = state;
            self.summary = summarize(self.cells.as_slice(), facts);
        }
        previous
    }

    #[inline]
    fn summary(&self) -> SectionSummary {
        self.summary
    }

    fn maybe_contains<P: FnMut(BlockStateId) -> bool>(&self, predicate: P) -> bool {
        self.cells.iter().copied().any(predicate)
    }
}

#[derive(Debug, Default)]
struct QualificationSectionBuilder {
    counters: BuildCounters,
}

impl QualificationSectionBuilder {
    fn reset(&mut self) {
        self.counters = BuildCounters::default();
    }
}

impl ImportedBlockSectionBuilder<BlockStateId> for QualificationSectionBuilder {
    type Section = QualificationBlockSection;

    fn build_uniform(&mut self, state: BlockStateId) -> Self::Section {
        self.counters.uniform_sections += 1;
        QualificationBlockSection::filled(state)
    }

    fn build_states(&mut self, states: &[BlockStateId]) -> Self::Section {
        self.counters.dense_sections += 1;
        self.counters.dense_semantic_cell_copies += states.len();
        QualificationBlockSection::from_states(states)
    }
}

/// Timings for one complete load-to-resident iteration and its reset unload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColdLoadSample {
    pub import_ns: u128,
    pub install_ns: u128,
    pub total_ns: u128,
    pub unload_drop_ns: u128,
}

/// Stable structural evidence captured after one warmed cold-load iteration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColdLoadStructure {
    pub stored_sections: usize,
    pub imported_block_sections: usize,
    pub uniform_sections: usize,
    pub dense_sections: usize,
    pub synthesized_empty_sections: usize,
    pub resident_sections: usize,
    pub dense_semantic_cell_copies: usize,
    pub decoder_retained_output_bytes: usize,
    pub decoder_retained_output_capacity: usize,
    pub section_scratch: BlockSectionScratchCapacities,
}

/// Reusable benchmark session. Decoder, section scratch, final-section builder, region bytes and
/// resident directory allocation all survive between rounds so measured samples represent the
/// steady cold-loading mechanism rather than one-time harness setup.
#[derive(Debug)]
pub struct ColdLoadHarness {
    region_bytes: Vec<u8>,
    decoder: DeflateChunkPayloadDecoder,
    resolver: Target262BlockStateResolver,
    builder: QualificationSectionBuilder,
    section_scratch: BlockSectionDecodeScratch<BlockStateId>,
    dimension: DimensionInstance<BlockStateId, QualificationBlockSection>,
}

impl ColdLoadHarness {
    /// Builds the deterministic 26.2 qualification fixture and preallocates decompression output.
    ///
    /// # Errors
    ///
    /// Returns an error if the fixed decompression buffer cannot be allocated or the standard
    /// Overworld-style runtime profile cannot be represented.
    pub fn new() -> Result<Self, String> {
        let decoder = DeflateChunkPayloadDecoder::try_with_output_limit(MAX_DECOMPRESSED_BYTES)
            .map_err(|error| format!("decoder allocation: {error:?}"))?;
        let profile = DimensionRuntimeProfile::new(DimensionTypeId(1), -64, 384, true)
            .map_err(|error| format!("qualification profile: {error:?}"))?;
        Ok(Self {
            region_bytes: fixture_region_bytes(),
            decoder,
            resolver: Target262BlockStateResolver,
            builder: QualificationSectionBuilder::default(),
            section_scratch: BlockSectionDecodeScratch::new(),
            dimension: DimensionInstance::new(DimensionId(1), profile),
        })
    }

    /// Performs one complete bytes-in-memory -> resident admission sample, then unloads/drops the
    /// resident generation outside the load timer so the next sample starts from the same lifecycle
    /// state.
    ///
    /// # Errors
    ///
    /// Returns an error for any framing/decode/install/unload failure or structural drift.
    pub fn sample(&mut self) -> Result<(ColdLoadSample, ColdLoadStructure), String> {
        if self.dimension.resident_chunk_count() != 0 {
            return Err("cold-load sample requires an empty resident directory".to_owned());
        }
        self.builder.reset();

        let total_start = Instant::now();
        let import_start = Instant::now();
        let region = RegionView::new(
            &self.region_bytes,
            0,
            0,
            RegionLimits::new(REGION_BYTES, SECTOR_BYTES),
        )
        .map_err(|error| format!("region framing: {error:?}"))?;
        let nbt_limits = NbtLimits::new(256, 4096, 4096, 16)
            .map_err(|error| format!("NBT limits: {error:?}"))?;
        let imported = {
            let mut importer = StoredBlockImporter::new(
                helve_world_import::ChunkPayloadLimits::new(SECTOR_BYTES, MAX_DECOMPRESSED_BYTES),
                nbt_limits,
                &mut self.decoder,
                &self.resolver,
                &mut self.builder,
                &mut self.section_scratch,
            );
            importer
                .import_region_chunk(&region, 0, 0, None)
                .map_err(|error| format!("stored import: {error:?}"))?
        };
        let import_ns = import_start.elapsed().as_nanos();

        let install_start = Instant::now();
        let installed = install_imported_chunk(&mut self.dimension, imported, || {
            self.builder.counters.synthesized_empty_sections += 1;
            QualificationBlockSection::filled(AIR)
        })
        .map_err(|error| format!("resident install: {error:?}"))?;
        let install_ns = install_start.elapsed().as_nanos();
        let total_ns = total_start.elapsed().as_nanos();

        let structure = self.structure()?;
        black_box(installed.handle);

        let unload_start = Instant::now();
        let unloaded = self
            .dimension
            .unload_chunk(installed.handle)
            .map_err(|error| format!("resident unload: {error:?}"))?;
        drop(black_box(unloaded));
        let unload_drop_ns = unload_start.elapsed().as_nanos();
        if self.dimension.resident_chunk_count() != 0 {
            return Err("resident directory did not drain after unload".to_owned());
        }

        Ok((
            ColdLoadSample {
                import_ns,
                install_ns,
                total_ns,
                unload_drop_ns,
            },
            structure,
        ))
    }

    /// Executes one unmeasured full semantic validation against independently expected fixture facts.
    ///
    /// # Errors
    ///
    /// Returns an error if the decoded/resident image or lifecycle does not match the fixture law.
    pub fn validate_semantics(&mut self) -> Result<(), String> {
        self.builder.reset();
        let region = RegionView::new(
            &self.region_bytes,
            0,
            0,
            RegionLimits::new(REGION_BYTES, SECTOR_BYTES),
        )
        .map_err(|error| format!("region framing: {error:?}"))?;
        let nbt_limits = NbtLimits::new(256, 4096, 4096, 16)
            .map_err(|error| format!("NBT limits: {error:?}"))?;
        let imported = {
            let mut importer = StoredBlockImporter::new(
                helve_world_import::ChunkPayloadLimits::new(SECTOR_BYTES, MAX_DECOMPRESSED_BYTES),
                nbt_limits,
                &mut self.decoder,
                &self.resolver,
                &mut self.builder,
                &mut self.section_scratch,
            );
            importer
                .import_region_chunk(&region, 0, 0, None)
                .map_err(|error| format!("stored import: {error:?}"))?
        };
        if imported.blocks.header.stored_section_count != STORED_SECTION_COUNT
            || imported.blocks.sections.len() != IMPORTED_SECTION_COUNT
        {
            return Err("fixture section cardinality drift".to_owned());
        }
        let installed = install_imported_chunk(&mut self.dimension, imported, || {
            self.builder.counters.synthesized_empty_sections += 1;
            QualificationBlockSection::filled(AIR)
        })
        .map_err(|error| format!("resident install: {error:?}"))?;
        let chunk = self
            .dimension
            .resolve_chunk(installed.handle)
            .map_err(|error| format!("resident resolve: {error:?}"))?;
        if chunk.section_count() != STORED_SECTION_COUNT || !chunk.masks_match_recomputation() {
            return Err("resident lattice/mask drift".to_owned());
        }

        let stone = resolve_target_26_2_block_state("minecraft:stone", &[])
            .ok_or_else(|| "qualified state resolver omitted minecraft:stone".to_owned())?;
        let dirt = resolve_target_26_2_block_state("minecraft:dirt", &[])
            .ok_or_else(|| "qualified state resolver omitted minecraft:dirt".to_owned())?;
        let probes = [
            (BlockPos { x: 0, y: -64, z: 0 }, AIR),
            (BlockPos { x: 1, y: -48, z: 0 }, stone),
            (BlockPos { x: 2, y: -48, z: 0 }, dirt),
            (BlockPos { x: 0, y: -32, z: 0 }, stone),
        ];
        for (pos, expected) in probes {
            let actual = chunk
                .get_block(pos)
                .map_err(|error| format!("semantic probe {pos:?}: {error:?}"))?;
            if actual != expected {
                return Err(format!(
                    "semantic probe mismatch at {pos:?}: expected {}, got {}",
                    expected.as_usize(),
                    actual.as_usize()
                ));
            }
        }
        let unloaded = self
            .dimension
            .unload_chunk(installed.handle)
            .map_err(|error| format!("resident unload: {error:?}"))?;
        drop(unloaded);
        if self.dimension.resident_chunk_count() != 0 {
            return Err("semantic validation did not drain residency".to_owned());
        }
        self.assert_builder_shape()
    }

    fn structure(&self) -> Result<ColdLoadStructure, String> {
        self.assert_builder_shape()?;
        Ok(ColdLoadStructure {
            stored_sections: STORED_SECTION_COUNT,
            imported_block_sections: IMPORTED_SECTION_COUNT,
            uniform_sections: self.builder.counters.uniform_sections,
            dense_sections: self.builder.counters.dense_sections,
            synthesized_empty_sections: self.builder.counters.synthesized_empty_sections,
            resident_sections: STORED_SECTION_COUNT,
            dense_semantic_cell_copies: self.builder.counters.dense_semantic_cell_copies,
            decoder_retained_output_bytes: self.decoder.retained_output_bytes(),
            decoder_retained_output_capacity: self.decoder.retained_output_capacity(),
            section_scratch: self.section_scratch.capacities(),
        })
    }

    fn assert_builder_shape(&self) -> Result<(), String> {
        let expected = BuildCounters {
            uniform_sections: UNIFORM_SECTION_COUNT,
            dense_sections: DENSE_SECTION_COUNT,
            dense_semantic_cell_copies: DENSE_CELL_COPIES,
            synthesized_empty_sections: OMITTED_SECTION_COUNT,
        };
        if self.builder.counters != expected {
            return Err(format!(
                "qualification builder shape drift: expected {expected:?}, got {:?}",
                self.builder.counters
            ));
        }
        Ok(())
    }
}

fn fixture_region_bytes() -> Vec<u8> {
    let mut bytes = vec![0_u8; REGION_BYTES];
    let location = (2_u32 << 8) | 1;
    bytes[..4].copy_from_slice(&location.to_be_bytes());
    bytes[SECTOR_BYTES..SECTOR_BYTES + 4].copy_from_slice(&17_u32.to_be_bytes());
    let record = REGION_HEADER_BYTES;
    let length = u32::try_from(ZLIB_CHUNK.len() + 1).expect("fixed fixture length fits u32");
    bytes[record..record + 4].copy_from_slice(&length.to_be_bytes());
    bytes[record + 4] = 2;
    bytes[record + 5..record + 5 + ZLIB_CHUNK.len()].copy_from_slice(&ZLIB_CHUNK);
    bytes
}

fn summary_for_repeated<F: BlockStateFacts<BlockStateId>>(
    state: BlockStateId,
    facts: &F,
) -> SectionSummary {
    let state_facts = facts.facts(state);
    SectionSummary {
        non_air_count: if state_facts.non_air() { 4096 } else { 0 },
        fluid_count: if state_facts.counted_fluid() { 4096 } else { 0 },
        random_block_present: state_facts.random_block(),
        random_fluid_present: state_facts.random_fluid(),
    }
}

fn summarize<F: BlockStateFacts<BlockStateId>>(
    states: &[BlockStateId],
    facts: &F,
) -> SectionSummary {
    let mut non_air_count = 0_u16;
    let mut fluid_count = 0_u16;
    let mut random_block_present = false;
    let mut random_fluid_present = false;
    for &state in states {
        let state_facts = facts.facts(state);
        non_air_count += u16::from(state_facts.non_air());
        fluid_count += u16::from(state_facts.counted_fluid());
        random_block_present |= state_facts.random_block();
        random_fluid_present |= state_facts.random_fluid();
    }
    SectionSummary {
        non_air_count,
        fluid_count,
        random_block_present,
        random_fluid_present,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BLOCK_SECTION_CELLS, ColdLoadHarness, DENSE_CELL_COPIES, DENSE_SECTION_COUNT,
        FIXTURE_COMPRESSED_BYTES, FIXTURE_DECOMPRESSED_BYTES, IMPORTED_SECTION_COUNT,
        OMITTED_SECTION_COUNT, STORED_SECTION_COUNT, UNIFORM_SECTION_COUNT,
    };

    #[test]
    fn deterministic_fixture_shape_is_frozen() {
        assert_eq!(FIXTURE_COMPRESSED_BYTES, 367);
        assert_eq!(FIXTURE_DECOMPRESSED_BYTES, 13_848);
        assert_eq!(STORED_SECTION_COUNT, 24);
        assert_eq!(IMPORTED_SECTION_COUNT, 18);
        assert_eq!(UNIFORM_SECTION_COUNT, 12);
        assert_eq!(DENSE_SECTION_COUNT, 6);
        assert_eq!(OMITTED_SECTION_COUNT, 6);
        assert_eq!(DENSE_CELL_COPIES, 24_576);
    }

    #[test]
    fn full_cold_load_semantics_and_structure_are_exact() {
        let mut harness = ColdLoadHarness::new().expect("qualification harness");
        harness.validate_semantics().expect("fixture semantics");
        let (sample, structure) = harness.sample().expect("cold load sample");
        assert!(sample.import_ns > 0);
        assert!(sample.install_ns > 0);
        assert!(sample.total_ns >= sample.import_ns);
        assert!(sample.unload_drop_ns > 0);
        assert_eq!(structure.stored_sections, 24);
        assert_eq!(structure.imported_block_sections, 18);
        assert_eq!(structure.uniform_sections, 12);
        assert_eq!(structure.dense_sections, 6);
        assert_eq!(structure.synthesized_empty_sections, 6);
        assert_eq!(structure.resident_sections, 24);
        assert_eq!(structure.dense_semantic_cell_copies, 24_576);
        assert!(structure.decoder_retained_output_bytes >= FIXTURE_DECOMPRESSED_BYTES);
        assert!(
            structure.decoder_retained_output_capacity >= structure.decoder_retained_output_bytes
        );
        assert!(structure.section_scratch.palette >= 3);
        assert!(structure.section_scratch.packed_words >= 256);
        assert!(structure.section_scratch.states >= BLOCK_SECTION_CELLS);
    }
}
