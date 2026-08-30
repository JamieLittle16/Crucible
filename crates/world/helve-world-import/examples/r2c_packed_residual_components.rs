use std::{
    env, fs,
    hint::black_box,
    path::{Path, PathBuf},
    time::Instant,
};

use helve_generated::BlockStateId;
use helve_types::ChunkPos;
use helve_world_import::{
    BlockSectionDecodeScratch, ChunkCompression, ChunkPayloadDecoder, DeflateChunkPayloadDecoder,
    ImportedBlockSectionBuilder, NbtLimits, NbtReader, RegionLimits, RegionView,
    TARGET_DATA_VERSION_26_2, TagType, Target262BlockStateResolver, decode_chunk_block_sections,
};

const SECTOR_BYTES: usize = 4096;
const MAX_REGION_BYTES: usize = 16 * SECTOR_BYTES;
const MAX_INLINE_COMPRESSED_BYTES: usize = 4 * SECTOR_BYTES;
const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024;
const PACKED_LOCAL_X: u8 = 1;
const PACKED_LOCAL_Z: u8 = 0;
const EXPECTED_DECOMPRESSED_BYTES: usize = 2204;
const EXPECTED_BLOCK_SECTIONS: usize = 1;
const EXPECTED_PALETTE_ENTRIES: usize = 2;
const EXPECTED_PACKED_WORDS: usize = 256;
const BATCH: usize = 32;

#[derive(Clone, Copy)]
enum Mode {
    Smoke,
    Full,
}

impl Mode {
    const fn rounds(self) -> usize {
        match self {
            Self::Smoke => 256,
            Self::Full => 4096,
        }
    }

    const fn warmups(self) -> usize {
        match self {
            Self::Smoke => 32,
            Self::Full => 256,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Full => "full",
        }
    }
}

struct Config {
    mode: Mode,
    packed_region: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SectionWitness {
    cells: usize,
}

struct NoCopyBuilder;

impl ImportedBlockSectionBuilder<BlockStateId> for NoCopyBuilder {
    type Section = SectionWitness;

    fn build_uniform(&mut self, state: BlockStateId) -> Self::Section {
        black_box(state);
        SectionWitness { cells: 4096 }
    }

    fn build_states(&mut self, states: &[BlockStateId]) -> Self::Section {
        black_box(states);
        SectionWitness {
            cells: states.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ScanWitness {
    data_version: i32,
    x_pos: i32,
    z_pos: i32,
    section_compounds: usize,
    block_sections: usize,
    palette_entries: usize,
    packed_words: usize,
    string_bytes: usize,
    word_mix: u64,
}

#[derive(Clone, Copy)]
enum LongArrayMode {
    ReadWords,
    SkipPayload,
}

#[derive(Clone, Copy)]
struct Summary {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

#[derive(Default)]
struct Samples {
    semantic_no_copy: Vec<u128>,
    nbt_read_words: Vec<u128>,
    nbt_skip_words: Vec<u128>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("r2c packed residual component probe failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let (decompressed, expected_position) = load_packed_nbt(&config.packed_region)?;
    if decompressed.len() != EXPECTED_DECOMPRESSED_BYTES {
        return Err(format!(
            "packed fixture decompressed length changed: expected={EXPECTED_DECOMPRESSED_BYTES} actual={}",
            decompressed.len()
        ));
    }

    let limits = nbt_limits()?;
    let read_witness = scan_nbt(
        &decompressed,
        expected_position,
        limits,
        LongArrayMode::ReadWords,
    )?;
    validate_read_witness(read_witness, expected_position)?;
    let skip_witness = scan_nbt(
        &decompressed,
        expected_position,
        limits,
        LongArrayMode::SkipPayload,
    )?;
    validate_skip_witness(skip_witness, expected_position)?;

    let resolver = Target262BlockStateResolver;
    let mut builder = NoCopyBuilder;
    let mut scratch = BlockSectionDecodeScratch::new();
    validate_semantic_decode(
        &decompressed,
        expected_position,
        limits,
        &resolver,
        &mut builder,
        &mut scratch,
    )?;

    for round in 0..config.mode.warmups() {
        black_box(measure_round(
            round,
            &decompressed,
            expected_position,
            limits,
            &resolver,
            &mut builder,
            &mut scratch,
        )?);
    }

    let mut samples = Samples {
        semantic_no_copy: Vec::with_capacity(config.mode.rounds()),
        nbt_read_words: Vec::with_capacity(config.mode.rounds()),
        nbt_skip_words: Vec::with_capacity(config.mode.rounds()),
    };
    for round in 0..config.mode.rounds() {
        let measured = measure_round(
            round,
            &decompressed,
            expected_position,
            limits,
            &resolver,
            &mut builder,
            &mut scratch,
        )?;
        samples.semantic_no_copy.push(measured[0]);
        samples.nbt_read_words.push(measured[1]);
        samples.nbt_skip_words.push(measured[2]);
    }

    report(config.mode, read_witness, samples);
    Ok(())
}

fn load_packed_nbt(path: &Path) -> Result<(Vec<u8>, ChunkPos), String> {
    let region_bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    if region_bytes.len() > MAX_REGION_BYTES {
        return Err(format!(
            "packed region exceeds {MAX_REGION_BYTES} bytes: {}",
            region_bytes.len()
        ));
    }
    let region = RegionView::new(
        &region_bytes,
        0,
        0,
        RegionLimits::new(MAX_REGION_BYTES, MAX_INLINE_COMPRESSED_BYTES),
    )
    .map_err(|error| format!("packed region rejected: {error:?}"))?;
    let chunk = region
        .chunk(PACKED_LOCAL_X, PACKED_LOCAL_Z)
        .map_err(|error| format!("packed chunk framing failed: {error:?}"))?
        .ok_or_else(|| "packed chunk slot is empty".to_owned())?;
    if chunk.compression != ChunkCompression::Gzip || chunk.external {
        return Err(format!(
            "packed fixture must be inline gzip, observed compression={:?} external={}",
            chunk.compression, chunk.external
        ));
    }
    let payload = chunk
        .inline_payload
        .ok_or_else(|| "inline packed chunk omitted payload".to_owned())?;
    let mut decoder = DeflateChunkPayloadDecoder::try_with_output_limit(MAX_DECOMPRESSED_BYTES)
        .map_err(|error| format!("decoder init failed: {error:?}"))?;
    let output = decoder
        .decode(ChunkCompression::Gzip, payload, MAX_DECOMPRESSED_BYTES)
        .map_err(|error| format!("production gzip decode failed: {error:?}"))?;
    Ok((output.to_vec(), chunk.position))
}

fn nbt_limits() -> Result<NbtLimits, String> {
    NbtLimits::new(1024, 1024, 8192, 32).map_err(|error| format!("invalid NBT limits: {error:?}"))
}

fn validate_semantic_decode(
    decompressed: &[u8],
    expected_position: ChunkPos,
    limits: NbtLimits,
    resolver: &Target262BlockStateResolver,
    builder: &mut NoCopyBuilder,
    scratch: &mut BlockSectionDecodeScratch<BlockStateId>,
) -> Result<(), String> {
    let blocks = decode_chunk_block_sections(
        decompressed,
        expected_position,
        limits,
        resolver,
        builder,
        scratch,
    )
    .map_err(|error| format!("semantic no-copy decode failed: {error:?}"))?;
    if blocks.header.data_version != TARGET_DATA_VERSION_26_2
        || blocks.header.position != expected_position
        || blocks.sections.len() != EXPECTED_BLOCK_SECTIONS
        || blocks
            .sections
            .iter()
            .any(|section| section.section.cells != 4096)
    {
        return Err(format!(
            "semantic no-copy witness changed: header={:?} sections={:?}",
            blocks.header, blocks.sections
        ));
    }
    Ok(())
}

fn measure_round(
    round: usize,
    decompressed: &[u8],
    expected_position: ChunkPos,
    limits: NbtLimits,
    resolver: &Target262BlockStateResolver,
    builder: &mut NoCopyBuilder,
    scratch: &mut BlockSectionDecodeScratch<BlockStateId>,
) -> Result<[u128; 3], String> {
    let phase = round % 3;
    let mut measured = [0_u128; 3];
    for offset in 0..3 {
        match (phase + offset) % 3 {
            0 => {
                measured[0] = measure_batch(|| {
                    let blocks = decode_chunk_block_sections(
                        black_box(decompressed),
                        expected_position,
                        limits,
                        resolver,
                        builder,
                        scratch,
                    )
                    .map_err(|error| format!("semantic no-copy decode failed: {error:?}"))?;
                    black_box(blocks);
                    Ok(())
                })?;
            }
            1 => {
                measured[1] = measure_batch(|| {
                    black_box(scan_nbt(
                        black_box(decompressed),
                        expected_position,
                        limits,
                        LongArrayMode::ReadWords,
                    )?);
                    Ok(())
                })?;
            }
            2 => {
                measured[2] = measure_batch(|| {
                    black_box(scan_nbt(
                        black_box(decompressed),
                        expected_position,
                        limits,
                        LongArrayMode::SkipPayload,
                    )?);
                    Ok(())
                })?;
            }
            _ => unreachable!(),
        }
    }
    Ok(measured)
}

fn scan_nbt(
    data: &[u8],
    expected_position: ChunkPos,
    limits: NbtLimits,
    long_array_mode: LongArrayMode,
) -> Result<ScanWitness, String> {
    let mut reader = NbtReader::new(data, limits);
    black_box(
        reader
            .begin_root_compound()
            .map_err(|error| format!("root NBT failed: {error:?}"))?,
    );

    let mut witness = ScanWitness::default();
    let mut have_data_version = false;
    let mut have_x = false;
    let mut have_z = false;
    let mut have_sections = false;

    while let Some(field) = reader
        .next_compound_field()
        .map_err(|error| format!("root field failed: {error:?}"))?
    {
        match field.name {
            "DataVersion" => {
                require_type("DataVersion", field.tag_type, TagType::Int)?;
                witness.data_version = reader
                    .read_i32()
                    .map_err(|error| format!("DataVersion read failed: {error:?}"))?;
                have_data_version = true;
            }
            "xPos" => {
                require_type("xPos", field.tag_type, TagType::Int)?;
                witness.x_pos = reader
                    .read_i32()
                    .map_err(|error| format!("xPos read failed: {error:?}"))?;
                have_x = true;
            }
            "zPos" => {
                require_type("zPos", field.tag_type, TagType::Int)?;
                witness.z_pos = reader
                    .read_i32()
                    .map_err(|error| format!("zPos read failed: {error:?}"))?;
                have_z = true;
            }
            "sections" => {
                require_type("sections", field.tag_type, TagType::List)?;
                let list = reader
                    .read_list_header()
                    .map_err(|error| format!("sections list failed: {error:?}"))?;
                require_type("sections[]", list.element_type, TagType::Compound)?;
                witness.section_compounds = list.len;
                for section_index in 0..list.len {
                    scan_section(&mut reader, section_index, long_array_mode, &mut witness)?;
                }
                have_sections = true;
            }
            _ => reader
                .skip_payload(field.tag_type)
                .map_err(|error| format!("root skip failed for {}: {error:?}", field.name))?,
        }
    }
    reader
        .finish_root()
        .map_err(|error| format!("root finish failed: {error:?}"))?;

    if !have_data_version || !have_x || !have_z || !have_sections {
        return Err("packed scan omitted a required root field".to_owned());
    }
    if witness.data_version != TARGET_DATA_VERSION_26_2
        || witness.x_pos != expected_position.x
        || witness.z_pos != expected_position.z
    {
        return Err(format!(
            "packed scan identity changed: witness={witness:?} expected_position={expected_position:?}"
        ));
    }
    Ok(witness)
}

fn scan_section(
    reader: &mut NbtReader<'_>,
    section_index: usize,
    long_array_mode: LongArrayMode,
    witness: &mut ScanWitness,
) -> Result<(), String> {
    let mut have_y = false;
    while let Some(field) = reader
        .next_compound_field()
        .map_err(|error| format!("section {section_index} field failed: {error:?}"))?
    {
        match field.name {
            "Y" => {
                require_type("sections[].Y", field.tag_type, TagType::Byte)?;
                black_box(
                    reader
                        .read_i8()
                        .map_err(|error| format!("section Y read failed: {error:?}"))?,
                );
                have_y = true;
            }
            "block_states" => {
                require_type("sections[].block_states", field.tag_type, TagType::Compound)?;
                scan_block_states(reader, section_index, long_array_mode, witness)?;
                witness.block_sections += 1;
            }
            _ => reader.skip_payload(field.tag_type).map_err(|error| {
                format!(
                    "section {section_index} skip failed for {}: {error:?}",
                    field.name
                )
            })?,
        }
    }
    if !have_y {
        return Err(format!("section {section_index} omitted Y"));
    }
    Ok(())
}

fn scan_block_states(
    reader: &mut NbtReader<'_>,
    section_index: usize,
    long_array_mode: LongArrayMode,
    witness: &mut ScanWitness,
) -> Result<(), String> {
    let mut have_palette = false;
    while let Some(field) = reader
        .next_compound_field()
        .map_err(|error| format!("block_states field failed: {error:?}"))?
    {
        match field.name {
            "palette" => {
                require_type("block_states.palette", field.tag_type, TagType::List)?;
                let list = reader
                    .read_list_header()
                    .map_err(|error| format!("palette list failed: {error:?}"))?;
                require_type(
                    "block_states.palette[]",
                    list.element_type,
                    TagType::Compound,
                )?;
                witness.palette_entries = witness.palette_entries.saturating_add(list.len);
                for palette_index in 0..list.len {
                    scan_palette_entry(reader, section_index, palette_index, witness)?;
                }
                have_palette = true;
            }
            "data" => {
                require_type("block_states.data", field.tag_type, TagType::LongArray)?;
                match long_array_mode {
                    LongArrayMode::ReadWords => {
                        let words = reader
                            .read_long_array_len()
                            .map_err(|error| format!("packed long length failed: {error:?}"))?;
                        witness.packed_words = witness.packed_words.saturating_add(words);
                        for _ in 0..words {
                            let word = reader
                                .read_i64()
                                .map_err(|error| format!("packed word read failed: {error:?}"))?;
                            let unsigned = u64::from_be_bytes(word.to_be_bytes());
                            witness.word_mix = witness.word_mix.rotate_left(1) ^ unsigned;
                        }
                    }
                    LongArrayMode::SkipPayload => reader
                        .skip_payload(TagType::LongArray)
                        .map_err(|error| format!("packed long skip failed: {error:?}"))?,
                }
            }
            _ => reader.skip_payload(field.tag_type).map_err(|error| {
                format!("block_states skip failed for {}: {error:?}", field.name)
            })?,
        }
    }
    if !have_palette {
        return Err(format!("section {section_index} omitted palette"));
    }
    Ok(())
}

fn scan_palette_entry(
    reader: &mut NbtReader<'_>,
    section_index: usize,
    palette_index: usize,
    witness: &mut ScanWitness,
) -> Result<(), String> {
    let mut have_name = false;
    while let Some(field) = reader
        .next_compound_field()
        .map_err(|error| format!("palette entry field failed: {error:?}"))?
    {
        match field.name {
            "Name" => {
                require_type("palette.Name", field.tag_type, TagType::String)?;
                let name = reader
                    .read_string()
                    .map_err(|error| format!("palette name read failed: {error:?}"))?;
                witness.string_bytes = witness.string_bytes.saturating_add(name.len());
                black_box(name);
                have_name = true;
            }
            "Properties" => {
                require_type("palette.Properties", field.tag_type, TagType::Compound)?;
                while let Some(property) = reader
                    .next_compound_field()
                    .map_err(|error| format!("property field failed: {error:?}"))?
                {
                    require_type("palette.Properties.*", property.tag_type, TagType::String)?;
                    let value = reader
                        .read_string()
                        .map_err(|error| format!("property value read failed: {error:?}"))?;
                    witness.string_bytes = witness
                        .string_bytes
                        .saturating_add(property.name.len())
                        .saturating_add(value.len());
                    black_box((property.name, value));
                }
            }
            _ => reader.skip_payload(field.tag_type).map_err(|error| {
                format!(
                    "palette entry {section_index}:{palette_index} skip failed for {}: {error:?}",
                    field.name
                )
            })?,
        }
    }
    if !have_name {
        return Err(format!(
            "palette entry {section_index}:{palette_index} omitted Name"
        ));
    }
    Ok(())
}

fn validate_read_witness(witness: ScanWitness, expected_position: ChunkPos) -> Result<(), String> {
    if witness.data_version != TARGET_DATA_VERSION_26_2
        || witness.x_pos != expected_position.x
        || witness.z_pos != expected_position.z
        || witness.block_sections != EXPECTED_BLOCK_SECTIONS
        || witness.palette_entries != EXPECTED_PALETTE_ENTRIES
        || witness.packed_words != EXPECTED_PACKED_WORDS
    {
        return Err(format!("packed read witness changed: {witness:?}"));
    }
    Ok(())
}

fn validate_skip_witness(witness: ScanWitness, expected_position: ChunkPos) -> Result<(), String> {
    if witness.data_version != TARGET_DATA_VERSION_26_2
        || witness.x_pos != expected_position.x
        || witness.z_pos != expected_position.z
        || witness.block_sections != EXPECTED_BLOCK_SECTIONS
        || witness.palette_entries != EXPECTED_PALETTE_ENTRIES
        || witness.packed_words != 0
        || witness.word_mix != 0
    {
        return Err(format!("packed skip witness changed: {witness:?}"));
    }
    Ok(())
}

fn require_type(field: &str, actual: TagType, expected: TagType) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "wrong tag type for {field}: expected={expected:?} actual={actual:?}"
        ))
    }
}

fn measure_batch<F>(mut operation: F) -> Result<u128, String>
where
    F: FnMut() -> Result<(), String>,
{
    let start = Instant::now();
    for _ in 0..BATCH {
        operation()?;
    }
    Ok(start.elapsed().as_nanos() / BATCH as u128)
}

fn report(mode: Mode, witness: ScanWitness, samples: Samples) {
    let semantic_no_copy = summarize(samples.semantic_no_copy);
    let nbt_read_words = summarize(samples.nbt_read_words);
    let nbt_skip_words = summarize(samples.nbt_skip_words);
    let long_read_delta_p50 = nbt_read_words.p50.saturating_sub(nbt_skip_words.p50);
    println!(
        "{{\"schema\":1,\"kind\":\"r2c-packed-residual-components\",\"mode\":\"{}\",\"fixture\":\"differential-packed4-gzip\",\"diagnostic_only\":true,\"performance_admitted\":false,\"rotating_order\":true,\"rounds\":{},\"batch\":{},\"decompressed_bytes\":{},\"block_sections\":{},\"palette_entries\":{},\"packed_words\":{},\"string_bytes\":{},\"word_mix\":{},\"semantic_no_copy_ns\":{},\"nbt_read_words_ns\":{},\"nbt_skip_words_ns\":{},\"long_read_delta_p50_ns\":{}}}",
        mode.as_str(),
        mode.rounds(),
        BATCH,
        EXPECTED_DECOMPRESSED_BYTES,
        witness.block_sections,
        witness.palette_entries,
        witness.packed_words,
        witness.string_bytes,
        witness.word_mix,
        summary_json(semantic_no_copy),
        summary_json(nbt_read_words),
        summary_json(nbt_skip_words),
        long_read_delta_p50,
    );
}

fn summarize(mut values: Vec<u128>) -> Summary {
    values.sort_unstable();
    Summary {
        p50: percentile(&values, 50),
        p95: percentile(&values, 95),
        p99: percentile(&values, 99),
        max: values.last().copied().unwrap_or_default(),
    }
}

fn percentile(values: &[u128], percentile: usize) -> u128 {
    if values.is_empty() {
        return 0;
    }
    let index = (values.len() - 1) * percentile / 100;
    values[index]
}

fn summary_json(summary: Summary) -> String {
    format!(
        "{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}}",
        summary.p50, summary.p95, summary.p99, summary.max
    )
}

fn parse_args() -> Result<Config, String> {
    let mut mode = None;
    let mut packed_region = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--packed4-region" => {
                if packed_region.is_some() {
                    return Err("--packed4-region may be specified only once".to_owned());
                }
                let path = args
                    .next()
                    .ok_or_else(|| "--packed4-region requires a path".to_owned())?;
                packed_region = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                return Err(
                    "usage: r2c_packed_residual_components (--smoke|--full) --packed4-region PATH"
                        .to_owned(),
                );
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    Ok(Config {
        mode: mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?,
        packed_region: packed_region
            .ok_or_else(|| "--packed4-region PATH is required".to_owned())?,
    })
}

fn set_mode(slot: &mut Option<Mode>, mode: Mode) -> Result<(), String> {
    if slot.replace(mode).is_some() {
        return Err("specify exactly one benchmark mode".to_owned());
    }
    Ok(())
}
