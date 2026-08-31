use std::env;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::hint::black_box;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

use helve_benchmark_support::{collect_hardware_metadata, push_json_string};
use helve_generated::{AIR, BlockStateId, STATE_DATA_GENERATION_SHA256, STATE_DATA_INPUT_SHA256};
use helve_types::{ChunkPos, DimensionId, DimensionTypeId};
use helve_world_import::{
    BlockSectionDecodeScratch, BlockSectionScratchCapacities, ChunkPayloadLimits,
    DeflateChunkPayloadDecoder, ExternalChunkPayload, NbtLimits, RegionLimits, RegionView,
    StoredBlockImporter, Target262BlockStateResolver,
};
use helve_world_load::install_imported_chunk;
use helve_world_load_qualification::{
    QualificationDirectSection, QualificationSectionBuilder, SectionBuildStats,
};
use helve_world_runtime::{DimensionInstance, DimensionRuntimeProfile};

const SCHEMA: u32 = 1;
const MAX_REGION_BYTES: usize = 256 * 1024 * 1024;
const MAX_INLINE_COMPRESSED_BYTES: usize = 8 * 1024 * 1024;
const MAX_EXTERNAL_COMPRESSED_BYTES: usize = 16 * 1024 * 1024;
const MAX_DECOMPRESSED_BYTES: usize = 16 * 1024 * 1024;
const MAX_NBT_STRING_BYTES: usize = 1024 * 1024;
const MAX_NBT_LIST_ELEMENTS: usize = 1024 * 1024;
const MAX_NBT_ARRAY_ELEMENTS: usize = 16 * 1024 * 1024;
const MAX_NBT_DEPTH: usize = 64;

type ResidentDimension = DimensionInstance<BlockStateId, QualificationDirectSection>;
type QualificationImporter<'a> = StoredBlockImporter<
    'a,
    Target262BlockStateResolver,
    QualificationSectionBuilder,
    DeflateChunkPayloadDecoder,
>;

#[derive(Debug)]
struct Config {
    world: PathBuf,
    output: Option<PathBuf>,
    warmup_rounds: usize,
    measured_rounds: usize,
    require_single_cpu: bool,
}

#[derive(Debug)]
struct PreparedChunk {
    local_x: u8,
    local_z: u8,
    position: ChunkPos,
    external_payload: Option<Vec<u8>>,
    compressed_payload_bytes: usize,
}

#[derive(Debug)]
struct PreparedRegion {
    path: PathBuf,
    region_x: i32,
    region_z: i32,
    bytes: Vec<u8>,
    chunks: Vec<PreparedChunk>,
}

#[derive(Clone, Debug)]
struct Summary {
    count: usize,
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

#[derive(Debug, Default)]
struct Measurement {
    dimension_setup_ns: Vec<u128>,
    region_open_ns: Vec<u128>,
    import_ns: Vec<u128>,
    install_ns: Vec<u128>,
    whole_chunk_ns: Vec<u128>,
    round_ns: Vec<u128>,
    empty_sections: u64,
    semantic_checksum: u64,
}

impl Measurement {
    fn reserve(&mut self, regions: usize, chunks: usize) {
        self.dimension_setup_ns.reserve(1);
        self.region_open_ns.reserve(regions);
        self.import_ns.reserve(chunks);
        self.install_ns.reserve(chunks);
        self.whole_chunk_ns.reserve(chunks);
        self.round_ns.reserve(1);
    }

    fn merge(&mut self, round: Self) {
        self.dimension_setup_ns.extend(round.dimension_setup_ns);
        self.region_open_ns.extend(round.region_open_ns);
        self.import_ns.extend(round.import_ns);
        self.install_ns.extend(round.install_ns);
        self.whole_chunk_ns.extend(round.whole_chunk_ns);
        self.round_ns.extend(round.round_ns);
        self.empty_sections += round.empty_sections;
        self.semantic_checksum = self
            .semantic_checksum
            .wrapping_mul(0x9e37_79b9_7f4a_7c15)
            .wrapping_add(round.semantic_checksum);
    }
}

#[derive(Debug)]
struct ChunkTiming {
    import_ns: u128,
    install_ns: u128,
    whole_ns: u128,
    empty_sections: u64,
    semantic_checksum: u64,
}

#[derive(Debug)]
struct Mechanisms {
    payload_limits: ChunkPayloadLimits,
    nbt_limits: NbtLimits,
    decoder: DeflateChunkPayloadDecoder,
    resolver: Target262BlockStateResolver,
    builder: QualificationSectionBuilder,
    scratch: BlockSectionDecodeScratch<BlockStateId>,
}

impl Mechanisms {
    fn new() -> Result<Self, String> {
        let nbt_limits = NbtLimits::new(
            MAX_NBT_STRING_BYTES,
            MAX_NBT_LIST_ELEMENTS,
            MAX_NBT_ARRAY_ELEMENTS,
            MAX_NBT_DEPTH,
        )
        .map_err(|error| format!("invalid qualification NBT limits: {error:?}"))?;
        let decoder = DeflateChunkPayloadDecoder::try_with_output_limit(MAX_DECOMPRESSED_BYTES)
            .map_err(|error| format!("could not initialize retained decoder output: {error:?}"))?;
        Ok(Self {
            payload_limits: ChunkPayloadLimits::new(
                MAX_EXTERNAL_COMPRESSED_BYTES,
                MAX_DECOMPRESSED_BYTES,
            ),
            nbt_limits,
            decoder,
            resolver: Target262BlockStateResolver,
            builder: QualificationSectionBuilder::default(),
            scratch: BlockSectionDecodeScratch::new(),
        })
    }

    fn run_round(
        &mut self,
        regions: &[PreparedRegion],
        chunk_count: usize,
        record_samples: bool,
    ) -> Result<Measurement, String> {
        let setup_start = Instant::now();
        let profile = DimensionRuntimeProfile::new(DimensionTypeId(1), -64, 384, true)
            .map_err(|error| format!("invalid Overworld qualification profile: {error:?}"))?;
        let mut dimension =
            DimensionInstance::with_chunk_capacity(DimensionId(1), profile, chunk_count);
        let setup_elapsed = setup_start.elapsed().as_nanos();

        let mut measurement = Measurement::default();
        if record_samples {
            measurement.reserve(regions.len(), chunk_count);
            measurement.dimension_setup_ns.push(setup_elapsed);
        }
        let round_start = Instant::now();
        {
            let mut stored_importer = StoredBlockImporter::new(
                self.payload_limits,
                self.nbt_limits,
                &mut self.decoder,
                &self.resolver,
                &mut self.builder,
                &mut self.scratch,
            );
            for prepared in regions {
                process_region(
                    &mut stored_importer,
                    &mut dimension,
                    prepared,
                    record_samples,
                    &mut measurement,
                )?;
            }
        }

        if dimension.resident_chunk_count() != chunk_count {
            return Err(format!(
                "resident chunk count mismatch: expected {chunk_count}, got {}",
                dimension.resident_chunk_count()
            ));
        }
        if record_samples {
            measurement.round_ns.push(round_start.elapsed().as_nanos());
        }
        black_box(dimension.resident_chunk_count());
        Ok(measurement)
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("R2C import-resident benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let hardware = collect_hardware_metadata()?;
    if config.require_single_cpu && hardware.single_allowed_cpu().is_none() {
        return Err(format!(
            "qualification requires exactly one allowed logical CPU; observed {}",
            hardware.cpus_allowed_list
        ));
    }

    let regions = prepare_world(&config.world)?;
    let chunk_count = regions.iter().map(|region| region.chunks.len()).sum::<usize>();
    if chunk_count == 0 {
        return Err("selected world contains no occupied overworld region chunks".to_owned());
    }
    let region_bytes = regions.iter().map(|region| region.bytes.len()).sum::<usize>();
    let compressed_payload_bytes = regions
        .iter()
        .flat_map(|region| &region.chunks)
        .map(|chunk| chunk.compressed_payload_bytes)
        .sum::<usize>();

    let mut mechanisms = Mechanisms::new()?;
    for _ in 0..config.warmup_rounds {
        black_box(mechanisms.run_round(&regions, chunk_count, false)?);
    }

    let builder_before = mechanisms.builder.stats();
    let scratch_before = mechanisms.scratch.capacities();
    let decoder_capacity_before = mechanisms.decoder.retained_output_capacity();
    let mut measured = Measurement::default();
    for _ in 0..config.measured_rounds {
        measured.merge(mechanisms.run_round(&regions, chunk_count, true)?);
    }
    let evidence = ReportEvidence {
        region_count: regions.len(),
        chunk_count,
        region_bytes,
        compressed_payload_bytes,
        builder: stats_delta(builder_before, mechanisms.builder.stats()),
        scratch_before,
        scratch_after: mechanisms.scratch.capacities(),
        decoder_capacity_before,
        decoder_capacity_after: mechanisms.decoder.retained_output_capacity(),
        measurement: &measured,
    };
    let report = render_report(&config, &hardware.to_json(), &evidence);
    write_report(&config, &report)
}

fn process_region(
    stored_importer: &mut QualificationImporter<'_>,
    dimension: &mut ResidentDimension,
    prepared: &PreparedRegion,
    record_samples: bool,
    measurement: &mut Measurement,
) -> Result<(), String> {
    let region_start = Instant::now();
    let region = RegionView::new(
        &prepared.bytes,
        prepared.region_x,
        prepared.region_z,
        RegionLimits::new(MAX_REGION_BYTES, MAX_INLINE_COMPRESSED_BYTES),
    )
    .map_err(|error| format!("region {} failed validation: {error:?}", prepared.path.display()))?;
    let region_elapsed = region_start.elapsed().as_nanos();
    if record_samples {
        measurement.region_open_ns.push(region_elapsed);
    }

    for prepared_chunk in &prepared.chunks {
        let sample = process_chunk(stored_importer, dimension, &region, prepared_chunk)?;
        measurement.empty_sections += sample.empty_sections;
        measurement.semantic_checksum = measurement
            .semantic_checksum
            .rotate_left(5)
            .wrapping_add(sample.semantic_checksum);
        if record_samples {
            measurement.import_ns.push(sample.import_ns);
            measurement.install_ns.push(sample.install_ns);
            measurement.whole_chunk_ns.push(sample.whole_ns);
        }
    }
    Ok(())
}

fn process_chunk(
    stored_importer: &mut QualificationImporter<'_>,
    dimension: &mut ResidentDimension,
    region: &RegionView<'_>,
    prepared: &PreparedChunk,
) -> Result<ChunkTiming, String> {
    let whole_start = Instant::now();
    let import_start = Instant::now();
    let external = prepared
        .external_payload
        .as_deref()
        .map(|bytes| ExternalChunkPayload { bytes });
    let imported_chunk = stored_importer
        .import_region_chunk(region, prepared.local_x, prepared.local_z, external)
        .map_err(|error| {
            format!(
                "import failed for chunk {},{}: {error:?}",
                prepared.position.x, prepared.position.z
            )
        })?;
    let import_ns = import_start.elapsed().as_nanos();

    let install_start = Instant::now();
    let mut empty_sections = 0_u64;
    let installed = install_imported_chunk(dimension, imported_chunk, || {
        empty_sections += 1;
        QualificationDirectSection::filled(AIR)
    })
    .map_err(|error| {
        format!(
            "resident install failed for chunk {},{}: {error:?}",
            prepared.position.x, prepared.position.z
        )
    })?;
    let install_ns = install_start.elapsed().as_nanos();
    let whole_ns = whole_start.elapsed().as_nanos();

    let resident = dimension
        .resolve_chunk(installed.handle)
        .map_err(|error| format!("new resident handle failed to resolve: {error:?}"))?;
    let masks = resident.masks();
    let checksum = masks.non_air_bits()
        ^ masks.fluid_bits().rotate_left(17)
        ^ masks.random_tick_bits().rotate_left(31)
        ^ coordinate_bits(prepared.position.x).rotate_left(7)
        ^ coordinate_bits(prepared.position.z).rotate_left(41);
    black_box(resident.section_count());
    Ok(ChunkTiming {
        import_ns,
        install_ns,
        whole_ns,
        empty_sections,
        semantic_checksum: checksum,
    })
}

fn coordinate_bits(value: i32) -> u64 {
    u64::from(u32::from_ne_bytes(value.to_ne_bytes()))
}

fn prepare_world(world: &Path) -> Result<Vec<PreparedRegion>, String> {
    let region_dir = world.join("region");
    let mut paths = fs::read_dir(&region_dir)
        .map_err(|error| format!("could not read {}: {error}", region_dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_region_file(path))
        .collect::<Vec<_>>();
    paths.sort();
    if paths.is_empty() {
        return Err(format!("no Anvil region files found under {}", region_dir.display()));
    }

    let mut prepared = Vec::with_capacity(paths.len());
    for path in paths {
        prepared.push(prepare_region(&region_dir, path)?);
    }
    Ok(prepared)
}

fn is_region_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "mca")
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("r."))
}

fn prepare_region(region_dir: &Path, path: PathBuf) -> Result<PreparedRegion, String> {
    let (region_x, region_z) = parse_region_coordinates(&path)?;
    let bytes = read_bounded(&path, MAX_REGION_BYTES)?;
    let region = RegionView::new(
        &bytes,
        region_x,
        region_z,
        RegionLimits::new(MAX_REGION_BYTES, MAX_INLINE_COMPRESSED_BYTES),
    )
    .map_err(|error| format!("invalid region {}: {error:?}", path.display()))?;
    let mut chunks = Vec::new();
    for local_x in 0_u8..32 {
        for local_z in 0_u8..32 {
            let Some(chunk) = region
                .chunk(local_x, local_z)
                .map_err(|error| format!("region slot {local_x},{local_z}: {error:?}"))?
            else {
                continue;
            };
            let external_payload = if chunk.external {
                let external_path =
                    region_dir.join(format!("c.{}.{}.mcc", chunk.position.x, chunk.position.z));
                Some(read_bounded(&external_path, MAX_EXTERNAL_COMPRESSED_BYTES)?)
            } else {
                None
            };
            let compressed_payload_bytes = external_payload.as_ref().map_or_else(
                || chunk.inline_payload.map_or(0, <[u8]>::len),
                Vec::len,
            );
            chunks.push(PreparedChunk {
                local_x,
                local_z,
                position: chunk.position,
                external_payload,
                compressed_payload_bytes,
            });
        }
    }
    Ok(PreparedRegion {
        path,
        region_x,
        region_z,
        bytes,
        chunks,
    })
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut args = args.into_iter();
    let mut world = None;
    let mut output = None;
    let mut warmup_rounds = 1_usize;
    let mut measured_rounds = 5_usize;
    let mut require_single_cpu = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--world" => world = Some(PathBuf::from(next_value(&mut args, "--world")?)),
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            "--warmup-rounds" => {
                warmup_rounds =
                    parse_positive(&next_value(&mut args, "--warmup-rounds")?, "warmup rounds")?;
            }
            "--measured-rounds" => {
                measured_rounds = parse_positive(
                    &next_value(&mut args, "--measured-rounds")?,
                    "measured rounds",
                )?;
            }
            "--require-single-cpu" => require_single_cpu = true,
            "--help" | "-h" => return Err(usage()),
            _ => return Err(format!("unknown argument: {arg}; {}", usage())),
        }
    }
    Ok(Config {
        world: world.ok_or_else(usage)?,
        output,
        warmup_rounds,
        measured_rounds,
        require_single_cpu,
    })
}

fn usage() -> String {
    "usage: r2c_import_resident_bench --world PATH [--output PATH] [--warmup-rounds N] [--measured-rounds N] [--require-single-cpu]".to_owned()
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_positive(value: &str, label: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid {label}: {value}"))?;
    if parsed == 0 {
        Err(format!("{label} must be positive"))
    } else {
        Ok(parsed)
    }
}

fn parse_region_coordinates(path: &Path) -> Result<(i32, i32), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("region path has no UTF-8 filename: {}", path.display()))?;
    let middle = name
        .strip_prefix("r.")
        .and_then(|name| name.strip_suffix(".mca"))
        .ok_or_else(|| format!("invalid Anvil region filename: {name}"))?;
    let mut parts = middle.split('.');
    let x = parts
        .next()
        .ok_or_else(|| format!("invalid Anvil region filename: {name}"))?
        .parse::<i32>()
        .map_err(|_| format!("invalid region X in filename: {name}"))?;
    let z = parts
        .next()
        .ok_or_else(|| format!("invalid Anvil region filename: {name}"))?
        .parse::<i32>()
        .map_err(|_| format!("invalid region Z in filename: {name}"))?;
    if parts.next().is_some() {
        return Err(format!("invalid Anvil region filename: {name}"));
    }
    Ok((x, z))
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let read_limit = u64::try_from(limit)
        .map_err(|_| "qualification file bound does not fit u64".to_owned())?
        .checked_add(1)
        .ok_or_else(|| "qualification file bound overflow".to_owned())?;
    let mut bytes = Vec::new();
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!(
            "{} exceeds qualification byte bound: actual>{limit}",
            path.display()
        ));
    }
    Ok(bytes)
}

fn stats_delta(before: SectionBuildStats, after: SectionBuildStats) -> SectionBuildStats {
    SectionBuildStats {
        uniform_sections: after.uniform_sections - before.uniform_sections,
        dense_sections: after.dense_sections - before.dense_sections,
        dense_cells_copied: after.dense_cells_copied - before.dense_cells_copied,
        retained_cells_written: after.retained_cells_written - before.retained_cells_written,
    }
}

fn summarize(mut values: Vec<u128>) -> Summary {
    values.sort_unstable();
    Summary {
        count: values.len(),
        p50: percentile(&values, 50),
        p95: percentile(&values, 95),
        p99: percentile(&values, 99),
        max: values.last().copied().unwrap_or(0),
    }
}

fn percentile(values: &[u128], percentile: usize) -> u128 {
    if values.is_empty() {
        return 0;
    }
    let numerator = (values.len() - 1) * percentile;
    values[numerator.div_ceil(100)]
}

fn summary_json(summary: &Summary) -> String {
    format!(
        "{{\"count\":{},\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}}",
        summary.count, summary.p50, summary.p95, summary.p99, summary.max
    )
}

fn scratch_json(capacity: BlockSectionScratchCapacities) -> String {
    format!(
        "{{\"palette\":{},\"packed_words\":{},\"states\":{}}}",
        capacity.palette, capacity.packed_words, capacity.states
    )
}

struct ReportEvidence<'a> {
    region_count: usize,
    chunk_count: usize,
    region_bytes: usize,
    compressed_payload_bytes: usize,
    builder: SectionBuildStats,
    scratch_before: BlockSectionScratchCapacities,
    scratch_after: BlockSectionScratchCapacities,
    decoder_capacity_before: usize,
    decoder_capacity_after: usize,
    measurement: &'a Measurement,
}

fn render_report(config: &Config, hardware_json: &str, evidence: &ReportEvidence<'_>) -> String {
    let mut output = format!(
        "{{\"schema\":{SCHEMA},\"kind\":\"r2c-import-resident-cpu-qualification\",\"mode\":\"hosted-diagnostic\",\"hardware\":{hardware_json},"
    );
    write!(
        output,
        "\"world\":{{\"region_files\":{},\"chunks\":{},\"region_file_bytes\":{},\"compressed_payload_bytes\":{}}},",
        evidence.region_count,
        evidence.chunk_count,
        evidence.region_bytes,
        evidence.compressed_payload_bytes
    )
    .expect("writing to String cannot fail");
    output.push_str("\"profile\":{\"dimension\":\"minecraft:overworld\",\"min_block_y\":-64,\"height\":384,\"section_count\":24},");
    write!(
        output,
        "\"config\":{{\"warmup_rounds\":{},\"measured_rounds\":{},\"filesystem_io_timed\":false,\"dimension_setup_timed_separately\":true,\"round_excludes_dimension_setup\":true}},",
        config.warmup_rounds, config.measured_rounds
    )
    .expect("writing to String cannot fail");
    output.push_str("\"mechanism\":{\"decoder\":\"DeflateChunkPayloadDecoder-preallocated\",\"section\":\"qualification-direct-4096-cell\",\"resident_directory\":\"pre-sized-DimensionInstance\"},");
    output.push_str("\"state_data\":{\"input_sha256\":");
    push_json_string(&mut output, STATE_DATA_INPUT_SHA256);
    output.push_str(",\"generation_sha256\":");
    push_json_string(&mut output, STATE_DATA_GENERATION_SHA256);
    output.push_str("},\"builder\":{");
    write!(
        output,
        "\"uniform_sections\":{},\"dense_sections\":{},\"dense_cells_copied\":{},\"retained_cells_written\":{}",
        evidence.builder.uniform_sections,
        evidence.builder.dense_sections,
        evidence.builder.dense_cells_copied,
        evidence.builder.retained_cells_written
    )
    .expect("writing to String cannot fail");
    output.push_str("},\"scratch\":{\"before\":");
    output.push_str(&scratch_json(evidence.scratch_before));
    output.push_str(",\"after\":");
    output.push_str(&scratch_json(evidence.scratch_after));
    write!(
        output,
        ",\"grew_during_measurement\":{},\"decoder_capacity_before\":{},\"decoder_capacity_after\":{},\"decoder_grew_during_measurement\":{}",
        evidence.scratch_before != evidence.scratch_after,
        evidence.decoder_capacity_before,
        evidence.decoder_capacity_after,
        evidence.decoder_capacity_before != evidence.decoder_capacity_after
    )
    .expect("writing to String cannot fail");
    output.push_str("},\"samples_ns\":{");
    push_summary_field(
        &mut output,
        "dimension_setup",
        &evidence.measurement.dimension_setup_ns,
        false,
    );
    push_summary_field(
        &mut output,
        "region_open",
        &evidence.measurement.region_open_ns,
        true,
    );
    push_summary_field(&mut output, "import", &evidence.measurement.import_ns, true);
    push_summary_field(&mut output, "install", &evidence.measurement.install_ns, true);
    push_summary_field(
        &mut output,
        "whole_chunk",
        &evidence.measurement.whole_chunk_ns,
        true,
    );
    push_summary_field(&mut output, "round", &evidence.measurement.round_ns, true);
    write!(
        output,
        "}},\"empty_sections_synthesized\":{},\"semantic_checksum\":{},\"production_decision_eligible\":false}}",
        evidence.measurement.empty_sections, evidence.measurement.semantic_checksum
    )
    .expect("writing to String cannot fail");
    output
}

fn push_summary_field(output: &mut String, name: &str, values: &[u128], comma: bool) {
    if comma {
        output.push(',');
    }
    push_json_string(output, name);
    output.push(':');
    output.push_str(&summary_json(&summarize(values.to_vec())));
}

fn write_report(config: &Config, report: &str) -> Result<(), String> {
    if let Some(path) = &config.output {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        fs::write(path, report)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        eprintln!("wrote {}", path.display());
    } else {
        println!("{report}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{parse_positive, parse_region_coordinates, percentile};

    #[test]
    fn strict_region_filename_parser_handles_signed_coordinates() {
        assert_eq!(
            parse_region_coordinates(Path::new("r.-2.7.mca")),
            Ok((-2, 7))
        );
        assert!(parse_region_coordinates(Path::new("r.2.7.mca.tmp")).is_err());
        assert!(parse_region_coordinates(Path::new("r.2.7.1.mca")).is_err());
    }

    #[test]
    fn positive_round_counts_are_required() {
        assert_eq!(parse_positive("3", "rounds"), Ok(3));
        assert!(parse_positive("0", "rounds").is_err());
        assert!(parse_positive("x", "rounds").is_err());
    }

    #[test]
    fn percentile_uses_integer_rank_without_floating_point() {
        let values = [1_u128, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&values, 50), 6);
        assert_eq!(percentile(&values, 95), 10);
        assert_eq!(percentile(&[], 99), 0);
    }
}
