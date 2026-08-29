use std::{env, fs, hint::black_box, path::PathBuf, time::Instant};

use helve_benchmark_support::collect_hardware_metadata;
use helve_generated::{AIR, BlockStateId, GeneratedStateFacts};
use helve_types::{BlockPos, DimensionId, DimensionTypeId};
use helve_world_contract::{BlockSection, SectionBlockPos};
use helve_world_import::{
    BlockSectionDecodeScratch, ChunkPayloadLimits, DeflateChunkPayloadDecoder,
    ImportedBlockSectionBuilder, NbtLimits, RegionLimits, RegionView, StoredBlockImporter,
    Target262BlockStateResolver,
};
use helve_world_load::install_imported_chunk;
use helve_world_reference::DirectBlockSection;
use helve_world_runtime::{DimensionInstance, DimensionRuntimeProfile};

const SECTOR_BYTES: usize = 4096;
const REGION_HEADER_BYTES: usize = 2 * SECTOR_BYTES;
const MAX_REGION_BYTES: usize = 16 * SECTOR_BYTES;
const MAX_INLINE_COMPRESSED_BYTES: usize = 4 * SECTOR_BYTES;
const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024;
const ZLIB_NBT: &[u8] = &[
    120, 156, 141, 142, 193, 10, 194, 48, 16, 68, 39, 109, 192, 54, 42, 30, 252, 8, 255, 193, 179,
    103, 241, 36, 120, 42, 107, 88, 33, 216, 38, 146, 221, 131, 248, 245, 77, 64, 232, 181, 183,
    25, 230, 61, 24, 7, 180, 216, 94, 72, 233, 206, 89, 66, 138, 192, 241, 212, 194, 126, 111, 73,
    128, 58, 218, 223, 63, 246, 232, 132, 189, 22, 70, 92, 169, 141, 129, 121, 192, 97, 247, 28,
    147, 127, 15, 162, 164, 44, 61, 54, 31, 26, 89, 149, 43, 98, 58, 216, 43, 77, 140, 195, 20, 34,
    251, 76, 47, 61, 139, 166, 200, 117, 43, 118, 179, 206, 222, 47, 54, 133, 92, 175, 204, 125,
    59, 43, 17,
];

#[derive(Clone, Copy)]
enum Mode {
    Smoke,
    Full,
}

impl Mode {
    const fn rounds(self) -> usize {
        match self {
            Self::Smoke => 16,
            Self::Full => 256,
        }
    }

    const fn warmups(self) -> usize {
        match self {
            Self::Smoke => 4,
            Self::Full => 32,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Full => "full",
        }
    }
}

#[derive(Debug)]
enum FixtureInput {
    EmbeddedUniform,
    Packed4Region(PathBuf),
}

struct Config {
    mode: Mode,
    require_single_cpu: bool,
    fixture: FixtureInput,
}

#[derive(Clone, Copy)]
enum Witness {
    UniformGap,
    PackedBinary,
}

#[derive(Clone, Copy)]
struct CaseSpec {
    label: &'static str,
    region_x: i32,
    region_z: i32,
    local_x: u8,
    local_z: u8,
    min_block_y: i32,
    height: u32,
    witness: Witness,
}

impl CaseSpec {
    const fn embedded_uniform() -> Self {
        Self {
            label: "embedded-uniform-zlib",
            region_x: 0,
            region_z: 0,
            local_x: 0,
            local_z: 0,
            min_block_y: 0,
            height: 48,
            witness: Witness::UniformGap,
        }
    }

    const fn packed4() -> Self {
        Self {
            label: "differential-packed4-gzip",
            region_x: 0,
            region_z: 0,
            local_x: 1,
            local_z: 0,
            min_block_y: -64,
            height: 16,
            witness: Witness::PackedBinary,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct BuilderMetrics {
    elapsed: u128,
    calls: usize,
}

#[derive(Default)]
struct DirectBuilder {
    metrics: BuilderMetrics,
}

impl DirectBuilder {
    fn reset_metrics(&mut self) {
        self.metrics = BuilderMetrics::default();
    }

    const fn metrics(&self) -> BuilderMetrics {
        self.metrics
    }

    fn record_build(&mut self, start: Instant) {
        self.metrics.elapsed = self
            .metrics
            .elapsed
            .saturating_add(start.elapsed().as_nanos());
        self.metrics.calls += 1;
    }
}

impl ImportedBlockSectionBuilder<BlockStateId> for DirectBuilder {
    type Section = DirectBlockSection<BlockStateId>;

    fn build_uniform(&mut self, state: BlockStateId) -> Self::Section {
        let start = Instant::now();
        let section = DirectBlockSection::filled(state, &GeneratedStateFacts);
        self.record_build(start);
        section
    }

    fn build_states(&mut self, states: &[BlockStateId]) -> Self::Section {
        let start = Instant::now();
        let first = states.first().copied().unwrap_or(AIR);
        let mut section = DirectBlockSection::filled(first, &GeneratedStateFacts);
        for y in 0_u8..16 {
            for z in 0_u8..16 {
                for x in 0_u8..16 {
                    let pos = SectionBlockPos::new(x, y, z).expect("bounded section coordinate");
                    let state = states[pos.index()];
                    if state != first {
                        section.replace(pos, state, &GeneratedStateFacts);
                    }
                }
            }
        }
        self.record_build(start);
        section
    }
}

#[derive(Clone, Copy)]
struct Sample {
    import: u128,
    section_build: u128,
    import_residual: u128,
    install: u128,
    whole: u128,
    build_calls: usize,
}

#[derive(Clone, Copy)]
struct Summary {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("r2c import-resident benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let hardware = collect_hardware_metadata()?;
    if config.require_single_cpu && hardware.single_allowed_cpu().is_none() {
        return Err(format!(
            "--require-single-cpu requested but affinity is {}",
            hardware.cpus_allowed_list
        ));
    }

    let (region_bytes, case) = match &config.fixture {
        FixtureInput::EmbeddedUniform => (synthetic_region(), CaseSpec::embedded_uniform()),
        FixtureInput::Packed4Region(path) => (read_region(path)?, CaseSpec::packed4()),
    };
    let region = RegionView::new(
        &region_bytes,
        case.region_x,
        case.region_z,
        RegionLimits::new(MAX_REGION_BYTES, MAX_INLINE_COMPRESSED_BYTES),
    )
    .map_err(|error| format!("benchmark region rejected: {error:?}"))?;
    let nbt_limits = NbtLimits::new(1024, 1024, 8192, 32)
        .map_err(|error| format!("invalid NBT limits: {error:?}"))?;
    let payload_limits =
        ChunkPayloadLimits::new(MAX_INLINE_COMPRESSED_BYTES, MAX_DECOMPRESSED_BYTES);
    let mut decoder = DeflateChunkPayloadDecoder::try_with_output_limit(MAX_DECOMPRESSED_BYTES)
        .map_err(|error| format!("decoder init failed: {error:?}"))?;
    let mut builder = DirectBuilder::default();
    let mut scratch = BlockSectionDecodeScratch::new();

    for _ in 0..config.mode.warmups() {
        black_box(run_sample(
            &region,
            case,
            payload_limits,
            nbt_limits,
            &mut decoder,
            &mut builder,
            &mut scratch,
        )?);
    }

    let mut samples = Vec::with_capacity(config.mode.rounds());
    let mut semantic_checksum = None;
    let mut section_build_calls = None;
    for _ in 0..config.mode.rounds() {
        let (sample, checksum) = run_sample(
            &region,
            case,
            payload_limits,
            nbt_limits,
            &mut decoder,
            &mut builder,
            &mut scratch,
        )?;
        if let Some(expected) = semantic_checksum {
            if expected != checksum {
                return Err("semantic checksum changed across benchmark rounds".to_owned());
            }
        } else {
            semantic_checksum = Some(checksum);
        }
        if let Some(expected) = section_build_calls {
            if expected != sample.build_calls {
                return Err("section builder call count changed across benchmark rounds".to_owned());
            }
        } else {
            section_build_calls = Some(sample.build_calls);
        }
        samples.push(sample);
    }

    let import = summarize(samples.iter().map(|sample| sample.import).collect());
    let section_build = summarize(samples.iter().map(|sample| sample.section_build).collect());
    let import_residual = summarize(
        samples
            .iter()
            .map(|sample| sample.import_residual)
            .collect(),
    );
    let install = summarize(samples.iter().map(|sample| sample.install).collect());
    let whole = summarize(samples.iter().map(|sample| sample.whole).collect());
    println!(
        "{{\"schema\":1,\"kind\":\"r2c-import-resident-whole-path\",\"mode\":\"{}\",\"fixture\":\"{}\",\"performance_admitted\":false,\"section_mechanism\":\"transparent-reference\",\"rounds\":{},\"semantic_checksum\":{},\"section_build_calls\":{},\"import_ns\":{},\"section_build_ns\":{},\"import_residual_ns\":{},\"install_ns\":{},\"whole_ns\":{},\"hardware\":{}}}",
        config.mode.as_str(),
        case.label,
        samples.len(),
        semantic_checksum.unwrap_or_default(),
        section_build_calls.unwrap_or_default(),
        summary_json(import),
        summary_json(section_build),
        summary_json(import_residual),
        summary_json(install),
        summary_json(whole),
        hardware.to_json(),
    );
    Ok(())
}

fn run_sample(
    region: &RegionView<'_>,
    case: CaseSpec,
    payload_limits: ChunkPayloadLimits,
    nbt_limits: NbtLimits,
    decoder: &mut DeflateChunkPayloadDecoder,
    builder: &mut DirectBuilder,
    scratch: &mut BlockSectionDecodeScratch<BlockStateId>,
) -> Result<(Sample, u64), String> {
    builder.reset_metrics();
    let resolver = Target262BlockStateResolver;
    let mut block_importer = StoredBlockImporter::new(
        payload_limits,
        nbt_limits,
        decoder,
        &resolver,
        builder,
        scratch,
    );
    let import_start = Instant::now();
    let stored_chunk = block_importer
        .import_region_chunk(region, case.local_x, case.local_z, None)
        .map_err(|error| format!("import failed: {error:?}"))?;
    let import = import_start.elapsed().as_nanos();
    drop(block_importer);
    let builder_metrics = builder.metrics();
    if builder_metrics.calls == 0 {
        return Err("section builder was not invoked".to_owned());
    }

    let profile =
        DimensionRuntimeProfile::new(DimensionTypeId(1), case.min_block_y, case.height, true)
            .map_err(|error| format!("dimension profile failed: {error:?}"))?;
    let mut dimension = DimensionInstance::new(DimensionId(1), profile);
    let install_start = Instant::now();
    let installed = install_imported_chunk(&mut dimension, stored_chunk, || {
        DirectBlockSection::filled(AIR, &GeneratedStateFacts)
    })
    .map_err(|error| format!("resident install failed: {error:?}"))?;
    let install = install_start.elapsed().as_nanos();

    let checksum = validate_resident(&dimension, installed.handle, case)?;
    let unloaded = dimension
        .unload_chunk(installed.handle)
        .map_err(|error| format!("resident unload failed: {error:?}"))?;
    black_box(unloaded);
    if dimension.resident_chunk_count() != 0 {
        return Err("resident directory did not return to zero".to_owned());
    }

    Ok((
        Sample {
            import,
            section_build: builder_metrics.elapsed,
            import_residual: import.saturating_sub(builder_metrics.elapsed),
            install,
            whole: import + install,
            build_calls: builder_metrics.calls,
        },
        checksum,
    ))
}

fn validate_resident(
    dimension: &DimensionInstance<BlockStateId, DirectBlockSection<BlockStateId>>,
    handle: helve_world_runtime::ResidentChunkHandle,
    case: CaseSpec,
) -> Result<u64, String> {
    let chunk = dimension
        .resolve_chunk(handle)
        .map_err(|error| format!("resident resolve failed: {error:?}"))?;
    match case.witness {
        Witness::UniformGap => {
            if chunk.section_count() != 3 || chunk.masks().non_air_bits() != 0b001 {
                return Err(format!(
                    "unexpected uniform resident shape: sections={} non_air_mask={:#b}",
                    chunk.section_count(),
                    chunk.masks().non_air_bits()
                ));
            }
            let low = read_block(chunk, BlockPos { x: 0, y: 0, z: 0 })?;
            let middle = read_block(chunk, BlockPos { x: 0, y: 16, z: 0 })?;
            let high = read_block(chunk, BlockPos { x: 0, y: 32, z: 0 })?;
            if low == AIR || middle != AIR || high != AIR {
                return Err("uniform resident semantic witness disagrees with fixture".to_owned());
            }
            Ok((low.as_usize() as u64)
                .wrapping_mul(0x9E37_79B1)
                .wrapping_add(chunk.masks().non_air_bits()))
        }
        Witness::PackedBinary => {
            if chunk.section_count() != 1 || chunk.masks().non_air_bits() != 0b1 {
                return Err(format!(
                    "unexpected packed resident shape: sections={} non_air_mask={:#b}",
                    chunk.section_count(),
                    chunk.masks().non_air_bits()
                ));
            }
            let stone = BlockStateId::new(1).ok_or_else(|| "state id 1 missing".to_owned())?;
            let first = read_block(
                chunk,
                BlockPos {
                    x: 16,
                    y: -64,
                    z: 0,
                },
            )?;
            let second = read_block(
                chunk,
                BlockPos {
                    x: 17,
                    y: -64,
                    z: 0,
                },
            )?;
            let row_next = read_block(
                chunk,
                BlockPos {
                    x: 16,
                    y: -64,
                    z: 1,
                },
            )?;
            if first != AIR || second != stone || row_next != stone {
                return Err(format!(
                    "packed resident witness mismatch: first={} second={} row_next={}",
                    first.as_usize(),
                    second.as_usize(),
                    row_next.as_usize()
                ));
            }
            Ok((second.as_usize() as u64)
                .wrapping_mul(0xD6E8_FEB8_6659_FD93)
                .wrapping_add(row_next.as_usize() as u64)
                .wrapping_add(chunk.masks().non_air_bits()))
        }
    }
}

fn read_block(
    chunk: &helve_world_chunk::LiveChunkCore<BlockStateId, DirectBlockSection<BlockStateId>>,
    pos: BlockPos,
) -> Result<BlockStateId, String> {
    chunk
        .get_block(pos)
        .map_err(|error| format!("block read {pos:?} failed: {error:?}"))
}

fn read_region(path: &PathBuf) -> Result<Vec<u8>, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    if bytes.len() > MAX_REGION_BYTES {
        return Err(format!(
            "benchmark region exceeds {MAX_REGION_BYTES} bytes: {}",
            bytes.len()
        ));
    }
    Ok(bytes)
}

fn parse_args() -> Result<Config, String> {
    let mut mode = None;
    let mut require_single_cpu = false;
    let mut fixture = FixtureInput::EmbeddedUniform;
    let mut packed_region_seen = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--require-single-cpu" => require_single_cpu = true,
            "--packed4-region" => {
                if packed_region_seen {
                    return Err("--packed4-region may be specified only once".to_owned());
                }
                let path = args
                    .next()
                    .ok_or_else(|| "--packed4-region requires a path".to_owned())?;
                fixture = FixtureInput::Packed4Region(PathBuf::from(path));
                packed_region_seen = true;
            }
            "--help" | "-h" => {
                return Err(
                    "usage: r2c_import_resident_bench (--smoke|--full) [--require-single-cpu] [--packed4-region PATH]"
                        .to_owned(),
                );
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    Ok(Config {
        mode: mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?,
        require_single_cpu,
        fixture,
    })
}

fn set_mode(slot: &mut Option<Mode>, mode: Mode) -> Result<(), String> {
    if slot.replace(mode).is_some() {
        return Err("specify exactly one benchmark mode".to_owned());
    }
    Ok(())
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

fn synthetic_region() -> Vec<u8> {
    let mut bytes = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
    let location = ((2_u32 << 8) | 1).to_be_bytes();
    bytes[0..4].copy_from_slice(&location);
    bytes[SECTOR_BYTES..SECTOR_BYTES + 4].copy_from_slice(&17_u32.to_be_bytes());
    let start = REGION_HEADER_BYTES;
    let length = u32::try_from(ZLIB_NBT.len() + 1).expect("fixture record length fits u32");
    bytes[start..start + 4].copy_from_slice(&length.to_be_bytes());
    bytes[start + 4] = 2;
    bytes[start + 5..start + 5 + ZLIB_NBT.len()].copy_from_slice(ZLIB_NBT);
    bytes
}

#[cfg(test)]
mod tests {
    use super::{FixtureInput, ZLIB_NBT, parse_args, synthetic_region};

    #[test]
    fn synthetic_region_has_one_zlib_record() {
        let bytes = synthetic_region();
        assert_eq!(&bytes[0..4], &[0, 0, 2, 1]);
        assert_eq!(bytes[8192 + 4], 2);
        assert_eq!(&bytes[8192 + 5..8192 + 5 + ZLIB_NBT.len()], ZLIB_NBT);
    }

    #[test]
    fn packed_fixture_variant_remains_distinct() {
        let input = FixtureInput::Packed4Region("fixture.mca".into());
        assert!(matches!(input, FixtureInput::Packed4Region(_)));
        let _ = parse_args;
    }
}
