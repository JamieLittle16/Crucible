use std::hint::black_box;
use std::time::Instant;

use helve_world_import::{
    ChunkPayloadDecoder, NbtLimits, RegionLimits, RegionView, decode_chunk_block_sections,
};

use super::{
    BuildCounters, ColdLoadHarness, DENSE_CELL_COPIES, DENSE_SECTION_COUNT, IMPORTED_SECTION_COUNT,
    MAX_DECOMPRESSED_BYTES, REGION_BYTES, SECTOR_BYTES, STORED_SECTION_COUNT,
    UNIFORM_SECTION_COUNT,
};

/// Diagnostic timings for the major stages inside one warmed stored-block import.
///
/// These timings deliberately exclude residency and filesystem I/O. They exist to identify which
/// import mechanism deserves a tournament; hosted values never constitute performance admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColdLoadStageSample {
    pub framing_ns: u128,
    pub slot_select_ns: u128,
    pub decompress_ns: u128,
    pub decode_build_ns: u128,
    pub release_ns: u128,
}

impl ColdLoadHarness {
    /// Measures the major stages inside one import without resident admission.
    ///
    /// Decoder output and semantic decode scratch are reused exactly as in the whole-load harness.
    /// Final sections are built through the same transparent builder and dropped after the timed
    /// decode/build stage so release work can be observed separately.
    ///
    /// # Errors
    ///
    /// Returns an error for framing, slot, decompression, semantic decode or structural drift.
    pub fn stage_sample(&mut self) -> Result<ColdLoadStageSample, String> {
        if self.dimension.resident_chunk_count() != 0 {
            return Err("stage sample requires an empty resident directory".to_owned());
        }
        self.builder.reset();

        let framing_start = Instant::now();
        let region = RegionView::new(
            &self.region_bytes,
            0,
            0,
            RegionLimits::new(REGION_BYTES, SECTOR_BYTES),
        )
        .map_err(|error| format!("stage region framing: {error:?}"))?;
        let framing_ns = framing_start.elapsed().as_nanos();

        let select_start = Instant::now();
        let chunk = region
            .chunk(0, 0)
            .map_err(|error| format!("stage region slot: {error:?}"))?
            .ok_or_else(|| "stage fixture region slot is empty".to_owned())?;
        if chunk.external {
            return Err("stage fixture unexpectedly uses external payload".to_owned());
        }
        let payload = chunk
            .inline_payload
            .ok_or_else(|| "stage fixture inline payload is absent".to_owned())?;
        let slot_select_ns = select_start.elapsed().as_nanos();

        let decompress_start = Instant::now();
        let decompressed = self
            .decoder
            .decode(chunk.compression, payload, MAX_DECOMPRESSED_BYTES)
            .map_err(|error| format!("stage decompression: {error:?}"))?;
        let decompress_ns = decompress_start.elapsed().as_nanos();

        let nbt_limits = NbtLimits::new(256, 4096, 4096, 16)
            .map_err(|error| format!("stage NBT limits: {error:?}"))?;
        let decode_start = Instant::now();
        let decoded = decode_chunk_block_sections(
            decompressed,
            chunk.position,
            nbt_limits,
            &self.resolver,
            &mut self.builder,
            &mut self.section_scratch,
        )
        .map_err(|error| format!("stage semantic decode: {error:?}"))?;
        let decode_build_ns = decode_start.elapsed().as_nanos();

        if decoded.header.stored_section_count != STORED_SECTION_COUNT
            || decoded.sections.len() != IMPORTED_SECTION_COUNT
        {
            return Err("stage decoded section cardinality drift".to_owned());
        }
        let expected = BuildCounters {
            uniform_sections: UNIFORM_SECTION_COUNT,
            dense_sections: DENSE_SECTION_COUNT,
            dense_semantic_cell_copies: DENSE_CELL_COPIES,
            synthesized_empty_sections: 0,
        };
        if self.builder.counters != expected {
            return Err(format!(
                "stage builder shape drift: expected {expected:?}, got {:?}",
                self.builder.counters
            ));
        }

        let release_start = Instant::now();
        drop(black_box(decoded));
        let release_ns = release_start.elapsed().as_nanos();

        Ok(ColdLoadStageSample {
            framing_ns,
            slot_select_ns,
            decompress_ns,
            decode_build_ns,
            release_ns,
        })
    }
}
