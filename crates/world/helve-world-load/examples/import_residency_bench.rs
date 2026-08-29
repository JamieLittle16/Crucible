use std::{env, fs, hint::black_box, path::PathBuf, time::Instant};

use helve_benchmark_support::collect_hardware_metadata;
use helve_types::{BlockPos, ChunkPos, DimensionId, DimensionTypeId};
use helve_world_contract::{BlockSection, BlockStateFacts, SectionBlockPos, SectionStateFacts};
use helve_world_import::{
    BlockSectionDecodeScratch, BlockStateResolver, ChunkPayloadLimits, DeflateChunkPayloadDecoder,
    ImportedBlockSectionBuilder, ImportedStoredChunk, NbtLimits, RegionLimits, RegionView,
    StoredBlockImporter, Target262BlockStateResolver,
    anvil::{REGION_HEADER_BYTES, SECTOR_BYTES},
};
use helve_world_load::install_imported_chunk;
use helve_world_reference::DirectBlockSection;
use helve_world_runtime::{DimensionInstance, DimensionRuntimeProfile};

const SCHEMA: u32 = 1;
const COMPRESSED_NBT: &[u8] = &[
    0x78, 0xda, 0x9d, 0x8d, 0xb1, 0x0a, 0xc2, 0x30, 0x14, 0x45, 0x6f, 0x68, 0xc0, 0x36,
    0x5a, 0x1c, 0xfc, 0x08, 0xff, 0xc1, 0xd9, 0x59, 0x9c, 0x04, 0x27, 0x79, 0x0d, 0x4f, 0x08,
    0xb6, 0x49, 0xc9, 0x7b, 0x83, 0xf8, 0xf3, 0xda, 0x40, 0xc1, 0xb5, 0x74, 0xbb, 0x70, 0xce,
    0xe1, 0x3a, 0xa0, 0xc2, 0xf6, 0x4c, 0x4a, 0x37, 0xce, 0x12, 0x52, 0x04, 0x0e, 0xc7, 0x0a,
    0xf6, 0x7d, 0x4d, 0x02, 0x14, 0x68, 0x3f, 0xf3, 0x6c, 0x50, 0x0b, 0x7b, 0x9d, 0x1c, 0x71,
    0x85, 0x18, 0x98, 0xfb, 0xd7, 0x61, 0xd7, 0xf5, 0xc9, 0xbf, 0x1e, 0xa2, 0xa4, 0x2c, 0x0d,
    0x36, 0x23, 0xf5, 0xac, 0xca, 0x45, 0x31, 0x35, 0xec, 0x85, 0x06, 0xc6, 0x7e, 0x08, 0x91,
    0x7d, 0xa6, 0xa7, 0x9e, 0x44, 0x53, 0xe4, 0xc2, 0xa6, 0x1a, 0xcb, 0xea, 0xf6, 0x5f, 0x53,
    0xc8, 0x73, 0x6b, 0x56, 0x3f, 0xe3, 0x07, 0x90, 0xe5, 0x3c, 0x41,
];
const DECOMPRESSED_NBT_BYTES: usize = 244;
const NBT_SHA256: &str = "002818fa4ac587b42097b8033a4768c28134e0d3556ea235a6a5e6edfaa83323";
const MAX_REGION_BYTES: usize = 3 * SECTOR_BYTES;
const MAX_INLINE_BYTES: usize = SECTOR_BYTES;
const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024;

type State = <Target262BlockStateResolver as BlockStateResolver>::State;

#[derive(Clone, Copy, Debug)]
struct FixtureFacts {
    air: State,
}

impl BlockStateFacts<State> for FixtureFacts {
    fn facts(&self, state: State) -> SectionStateFacts {
        SectionStateFacts::new(state != self.air, false, false, false)
    }
}

#[derive(Debug)]
struct DirectBuilder {
    facts: FixtureFacts,
}

impl ImportedBlockSectionBuilder<State> for DirectBuilder {
    type Section = DirectBlockSection<State>;

    fn build_uniform(&mut self, state: State) -> Self::Section {
        DirectBlockSection::filled(state, &self.facts)
    }

    fn build_states(&mut self, states: &[State]) -> Self::Section {
        let initial = states.first().copied().unwrap_or(self.facts.air);
        let mut section = DirectBlockSection::filled(initial, &self.facts);
        for (index, &state) in states.iter().enumerate() {
            if state == initial {
                continue;
            }
            let x = u8::try_from(index & 15).expect("bounded x");
            let z = u8::try_from((index >> 4) & 15).expect("bounded z");
            let y = u8::try_from((index >> 8) & 15).expect("bounded y");
            let pos = SectionBlockPos::new(x, y, z).expect("bounded section coordinate");
            section.replace(pos, state, &self.facts);
        }
        section
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Smoke,
    Full,
}

impl Mode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Full => "full",
        }
    }
}

#[derive(Debug)]
struct Config {
    mode: Mode,
    output: Option<PathBuf>,
    require_single_cpu: bool,
    warmup_rounds: usize,
    measured_rounds: usize,
}

impl Config {
    fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                require_single_cpu: false,
                warmup_rounds: 4,
                measured_rounds: 24,
            },
            Mode::Full => Self {
                mode,
                output: None,
                require_single_cpu: false,
                warmup_rounds: 32,
                measured_rounds: 256,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Summary {
    p50: u128,
    p95: u128,
    p99: u128,
    p999: u128,
    max: u128,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("import residency benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let hardware = collect_hardware_metadata()?;
    if config.require_single_cpu && hardware.single_allowed_cpu().is_none() {
        return Err(format!(
            "target qualification requires exactly one allowed logical CPU; observed {}",
            hardware.cpus_allowed_list
        ));
    }

    let region_bytes = fixture_region();
    let region_limits = RegionLimits::new(MAX_REGION_BYTES, MAX_INLINE_BYTES);
    let payload_limits = ChunkPayloadLimits::new(MAX_INLINE_BYTES, MAX_DECOMPRESSED_BYTES);
    let nbt_limits = NbtLimits::new(1024, 4096, 65_536, 32)
        .map_err(|error| format!("invalid benchmark NBT limits: {error:?}"))?;
    let resolver = Target262BlockStateResolver;
    let air = resolver
        .resolve("minecraft:air", &[])
        .ok_or_else(|| "target resolver did not resolve minecraft:air".to_owned())?;
    let stone = resolver
        .resolve("minecraft:stone", &[])
        .ok_or_else(|| "target resolver did not resolve minecraft:stone".to_owned())?;
    let mut builder = DirectBuilder {
        facts: FixtureFacts { air },
    };
    let mut scratch = BlockSectionDecodeScratch::new();
    let mut decoder = DeflateChunkPayloadDecoder::try_with_output_limit(MAX_DECOMPRESSED_BYTES)
        .map_err(|error| format!("decoder initialization failed: {error:?}"))?;
    let mut importer = StoredBlockImporter::new(
        payload_limits,
        nbt_limits,
        &mut decoder,
        &resolver,
        &mut builder,
        &mut scratch,
    );
    let mut dimension = benchmark_dimension()?;

    let preflight_region = RegionView::new(&region_bytes, 0, 0, region_limits)
        .map_err(|error| format!("fixture region rejected: {error:?}"))?;
    let preflight = importer
        .import_region_chunk(&preflight_region, 0, 0, None)
        .map_err(|error| format!("fixture import rejected: {error:?}"))?;
    validate_imported(&preflight)?;
    let installed = install_imported_chunk(&mut dimension, preflight, || {
        DirectBlockSection::filled(air, &FixtureFacts { air })
    })
    .map_err(|error| format!("preflight install rejected: {error:?}"))?;
    validate_resident(&dimension, installed.handle, air, stone)?;
    dimension
        .unload_chunk(installed.handle)
        .map_err(|error| format!("preflight unload rejected: {error:?}"))?;

    for _ in 0..config.warmup_rounds {
        black_box(run_whole_round(
            &region_bytes,
            region_limits,
            &mut importer,
            &mut dimension,
            air,
        )?);
    }

    let mut whole_ns = Vec::with_capacity(config.measured_rounds);
    let mut install_ns = Vec::with_capacity(config.measured_rounds);
    let mut first_generation = None;
    let mut last_generation = None;
    for _ in 0..config.measured_rounds {
        let started = Instant::now();
        let handle = run_whole_round(
            &region_bytes,
            region_limits,
            &mut importer,
            &mut dimension,
            air,
        )?;
        whole_ns.push(started.elapsed().as_nanos());
        first_generation.get_or_insert(handle.generation);
        last_generation = Some(handle.generation);

        let region = RegionView::new(&region_bytes, 0, 0, region_limits)
            .map_err(|error| format!("fixture region rejected: {error:?}"))?;
        let imported = importer
            .import_region_chunk(&region, 0, 0, None)
            .map_err(|error| format!("fixture import rejected: {error:?}"))?;
        let started = Instant::now();
        let installed = install_imported_chunk(&mut dimension, imported, || {
            DirectBlockSection::filled(air, &FixtureFacts { air })
        })
        .map_err(|error| format!("install-only round rejected: {error:?}"))?;
        dimension
            .unload_chunk(installed.handle)
            .map_err(|error| format!("install-only unload rejected: {error:?}"))?;
        install_ns.push(started.elapsed().as_nanos());
        last_generation = Some(installed.handle.generation);
    }

    if dimension.resident_chunk_count() != 0 {
        return Err("benchmark leaked resident chunks".to_owned());
    }
    drop(importer);

    let whole_summary = summarize(&whole_ns);
    let install_summary = summarize(&install_ns);
    let report = format!(
        concat!(
            "{{\n",
            "  \"schema\": {schema},\n",
            "  \"benchmark\": \"r2c-import-residency\",\n",
            "  \"mode\": \"{mode}\",\n",
            "  \"hosted_ci_is_diagnostic_only\": true,\n",
            "  \"performance_admitted\": false,\n",
            "  \"reference_section_storage_only\": true,\n",
            "  \"fixture\": {{\"compression\": \"zlib\", \"compressed_bytes\": {compressed}, \"decompressed_nbt_bytes\": {decompressed}, \"nbt_sha256\": \"{nbt_sha}\", \"stored_sections\": 3, \"resident_sections\": 3, \"semantic_cells\": 12288}},\n",
            "  \"structure\": {{\"install_cell_copy_count\": 0, \"install_sparse_sort\": true, \"resident_directory_probes_per_successful_install\": 1, \"decoder_retained_output_bytes\": {retained}, \"decoder_retained_output_capacity\": {capacity}, \"resident_count_after_rounds\": 0}},\n",
            "  \"generation\": {{\"first_measured\": {first_generation}, \"last_measured\": {last_generation}, \"monotonic\": true}},\n",
            "  \"measured_rounds\": {rounds},\n",
            "  \"whole_decode_install_unload_ns\": {whole_samples},\n",
            "  \"install_unload_ns\": {install_samples},\n",
            "  \"whole_summary_ns\": {whole_summary},\n",
            "  \"install_summary_ns\": {install_summary},\n",
            "  \"hardware\": {hardware}\n",
            "}}\n"
        ),
        schema = SCHEMA,
        mode = config.mode.as_str(),
        compressed = COMPRESSED_NBT.len(),
        decompressed = DECOMPRESSED_NBT_BYTES,
        nbt_sha = NBT_SHA256,
        retained = decoder.retained_output_bytes(),
        capacity = decoder.retained_output_capacity(),
        first_generation = first_generation.map_or(0, |generation| generation.0),
        last_generation = last_generation.map_or(0, |generation| generation.0),
        rounds = config.measured_rounds,
        whole_samples = render_samples(&whole_ns),
        install_samples = render_samples(&install_ns),
        whole_summary = render_summary(whole_summary),
        install_summary = render_summary(install_summary),
        hardware = hardware.to_json(),
    );

    if let Some(path) = config.output {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        fs::write(&path, report)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    } else {
        print!("{report}");
    }
    Ok(())
}

fn run_whole_round(
    region_bytes: &[u8],
    region_limits: RegionLimits,
    importer: &mut StoredBlockImporter<'_, Target262BlockStateResolver, DirectBuilder, DeflateChunkPayloadDecoder>,
    dimension: &mut DimensionInstance<State, DirectBlockSection<State>>,
    air: State,
) -> Result<helve_world_runtime::ResidentChunkHandle, String> {
    let region = RegionView::new(region_bytes, 0, 0, region_limits)
        .map_err(|error| format!("region framing failed: {error:?}"))?;
    let imported = importer
        .import_region_chunk(&region, 0, 0, None)
        .map_err(|error| format!("stored import failed: {error:?}"))?;
    let installed = install_imported_chunk(dimension, imported, || {
        DirectBlockSection::filled(air, &FixtureFacts { air })
    })
    .map_err(|error| format!("resident install failed: {error:?}"))?;
    let handle = installed.handle;
    black_box(
        dimension
            .resolve_chunk(handle)
            .map_err(|error| format!("resident resolve failed: {error:?}"))?
            .masks(),
    );
    black_box(
        dimension
            .unload_chunk(handle)
            .map_err(|error| format!("resident unload failed: {error:?}"))?,
    );
    Ok(handle)
}

fn benchmark_dimension() -> Result<DimensionInstance<State, DirectBlockSection<State>>, String> {
    let profile = DimensionRuntimeProfile::new(DimensionTypeId(1), -16, 48, true)
        .map_err(|error| format!("benchmark dimension profile rejected: {error:?}"))?;
    Ok(DimensionInstance::with_chunk_capacity(
        DimensionId(1),
        profile,
        1,
    ))
}

fn validate_imported(imported: &ImportedStoredChunk<DirectBlockSection<State>>) -> Result<(), String> {
    if imported.blocks.header.position != (ChunkPos { x: 0, z: 0 }) {
        return Err("fixture chunk position drift".to_owned());
    }
    if imported.blocks.sections.len() != 3 {
        return Err("fixture section count drift".to_owned());
    }
    Ok(())
}

fn validate_resident(
    dimension: &DimensionInstance<State, DirectBlockSection<State>>,
    handle: helve_world_runtime::ResidentChunkHandle,
    air: State,
    stone: State,
) -> Result<(), String> {
    let chunk = dimension
        .resolve_chunk(handle)
        .map_err(|error| format!("resident validation resolve failed: {error:?}"))?;
    for (section_y, expected) in [(-1, stone), (0, air), (1, stone)] {
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    let world = BlockPos {
                        x,
                        y: section_y * 16 + y,
                        z,
                    };
                    let actual = chunk
                        .get_block(world)
                        .map_err(|error| format!("resident validation read failed: {error:?}"))?;
                    if actual != expected {
                        return Err(format!(
                            "resident semantic mismatch at {},{},{}",
                            world.x, world.y, world.z
                        ));
                    }
                }
            }
        }
    }
    if !chunk.masks_match_recomputation() {
        return Err("resident masks disagree with recomputation".to_owned());
    }
    Ok(())
}

fn fixture_region() -> Vec<u8> {
    let mut bytes = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
    let location = (2_u32 << 8) | 1;
    bytes[..4].copy_from_slice(&location.to_be_bytes());
    bytes[SECTOR_BYTES..SECTOR_BYTES + 4].copy_from_slice(&77_u32.to_be_bytes());
    let start = REGION_HEADER_BYTES;
    let length = u32::try_from(COMPRESSED_NBT.len() + 1).expect("fixture length fits u32");
    bytes[start..start + 4].copy_from_slice(&length.to_be_bytes());
    bytes[start + 4] = 2;
    bytes[start + 5..start + 5 + COMPRESSED_NBT.len()].copy_from_slice(COMPRESSED_NBT);
    bytes
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut args = args.into_iter();
    let mut mode = None;
    let mut output = None;
    let mut require_single_cpu = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--require-single-cpu" => require_single_cpu = true,
            "--output" => {
                output = Some(PathBuf::from(
                    args.next().ok_or_else(|| "--output requires a path".to_owned())?,
                ));
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    config.require_single_cpu = require_single_cpu;
    Ok(config)
}

fn set_mode(slot: &mut Option<Mode>, value: Mode) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err("specify exactly one benchmark mode".to_owned());
    }
    Ok(())
}

fn summarize(samples: &[u128]) -> Summary {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    Summary {
        p50: percentile(&sorted, 500, 1000),
        p95: percentile(&sorted, 950, 1000),
        p99: percentile(&sorted, 990, 1000),
        p999: percentile(&sorted, 999, 1000),
        max: sorted.last().copied().unwrap_or(0),
    }
}

fn percentile(sorted: &[u128], numerator: usize, denominator: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let index = (sorted.len() - 1) * numerator / denominator;
    sorted[index]
}

fn render_samples(samples: &[u128]) -> String {
    let values = samples.iter().map(u128::to_string).collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

fn render_summary(summary: Summary) -> String {
    format!(
        "{{\"p50\":{},\"p95\":{},\"p99\":{},\"p99_9\":{},\"max\":{}}}",
        summary.p50, summary.p95, summary.p99, summary.p999, summary.max
    )
}

#[cfg(test)]
mod tests {
    use super::{COMPRESSED_NBT, fixture_region, parse_args, summarize};
    use helve_world_import::{RegionLimits, RegionView, anvil::SECTOR_BYTES};

    #[test]
    fn fixture_is_one_zlib_chunk_in_exact_region_slot_zero() {
        let bytes = fixture_region();
        let region = RegionView::new(
            &bytes,
            0,
            0,
            RegionLimits::new(bytes.len(), SECTOR_BYTES),
        )
        .expect("fixture region");
        let chunk = region.chunk(0, 0).expect("slot parse").expect("occupied slot");
        assert_eq!(chunk.inline_payload, Some(COMPRESSED_NBT));
        assert_eq!(chunk.position.x, 0);
        assert_eq!(chunk.position.z, 0);
    }

    #[test]
    fn percentile_shape_is_monotonic() {
        let summary = summarize(&[10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);
        assert!(summary.p50 <= summary.p95);
        assert!(summary.p95 <= summary.p99);
        assert!(summary.p99 <= summary.p999);
        assert!(summary.p999 <= summary.max);
    }

    #[test]
    fn cli_requires_exactly_one_mode() {
        assert!(parse_args(["--smoke".to_owned()]).is_ok());
        assert!(parse_args(["--full".to_owned()]).is_ok());
        assert!(parse_args(Vec::<String>::new()).is_err());
        assert!(parse_args(["--smoke".to_owned(), "--full".to_owned()]).is_err());
    }
}
