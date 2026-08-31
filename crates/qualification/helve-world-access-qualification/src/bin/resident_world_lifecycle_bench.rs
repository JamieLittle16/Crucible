use std::env;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::mem::size_of;
use std::path::PathBuf;
use std::time::Instant;

use helve_benchmark_support::{HardwareMetadata, collect_hardware_metadata, push_json_string};
use helve_generated::{BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts};
use helve_types::{BlockPos, ChunkPos, DimensionId, DimensionTypeId};
use helve_world_reference::DirectBlockSection;
use helve_world_runtime::{
    DimensionInstance, DimensionRuntimeProfile, ResidentChunkAccessError, ResidentChunkHandle,
};

const SCHEMA: u32 = 1;
const STANDARD_MIN_BLOCK_Y: i32 = -64;
const STANDARD_HEIGHT: u32 = 384;
const STANDARD_SECTION_COUNT: usize = 24;
const BLOCKS_PER_CHUNK_AXIS: i32 = 16;

type BenchSection = DirectBlockSection<BlockStateId>;
type BenchDimension = DimensionInstance<BlockStateId, BenchSection>;

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

#[derive(Clone, Copy, Debug)]
struct CaseSpec {
    name: &'static str,
    origin: ChunkPos,
    side: usize,
}

impl CaseSpec {
    fn chunk_count(self) -> Result<usize, String> {
        self.side
            .checked_mul(self.side)
            .ok_or_else(|| format!("{} chunk extent overflow", self.name))
    }
}

#[derive(Clone, Debug)]
struct Config {
    mode: Mode,
    output: Option<PathBuf>,
    require_single_cpu: bool,
    warmup_rounds: usize,
    measured_rounds: usize,
    hot_reads: usize,
}

impl Config {
    fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                require_single_cpu: false,
                warmup_rounds: 2,
                measured_rounds: 12,
                hot_reads: 16_384,
            },
            Mode::Full => Self {
                mode,
                output: None,
                require_single_cpu: false,
                warmup_rounds: 8,
                measured_rounds: 64,
                hot_reads: 262_144,
            },
        }
    }
}

#[derive(Debug)]
struct PreparedChunk {
    position: ChunkPos,
    sections: Vec<BenchSection>,
}

#[derive(Clone, Copy, Debug)]
struct LifecycleSample {
    round: usize,
    elapsed_ns: u128,
}

#[derive(Clone, Copy, Debug)]
struct HotPairSample {
    round: usize,
    repeated_first: bool,
    repeated_resolve_ns: u128,
    resolve_once_ns: u128,
}

#[derive(Clone, Copy, Debug)]
struct Summary {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

#[derive(Debug)]
struct CaseReport {
    spec: CaseSpec,
    chunk_count: usize,
    lifecycle_checksum: u64,
    hot_checksum: u64,
    lifecycle_samples: Vec<LifecycleSample>,
    hot_samples: Vec<HotPairSample>,
    lifecycle_summary: Summary,
    repeated_summary: Summary,
    resolve_once_summary: Summary,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("resident-world lifecycle benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let hardware = collect_hardware_metadata()?;
    if config.require_single_cpu && hardware.single_allowed_cpu().is_none() {
        return Err(format!(
            "production qualification requires exactly one allowed logical CPU; observed {}. Run under taskset or an equivalent affinity mechanism",
            hardware.cpus_allowed_list
        ));
    }

    let profile = standard_profile()?;
    if profile.section_count() != STANDARD_SECTION_COUNT {
        return Err(format!(
            "standard profile section count drifted: expected {STANDARD_SECTION_COUNT}, got {}",
            profile.section_count()
        ));
    }

    let specs = match config.mode {
        Mode::Smoke => smoke_cases(),
        Mode::Full => full_cases(),
    };
    let mut reports = Vec::with_capacity(specs.len());
    for spec in specs {
        eprintln!(
            "benchmarking {} ({} resident chunks)",
            spec.name,
            spec.chunk_count()?
        );
        reports.push(bench_case(spec, profile, &config)?);
    }

    let artifact = render_report(&config, &hardware, profile, &reports);
    if let Some(path) = config.output {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        fs::write(&path, artifact)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        eprintln!("wrote {}", path.display());
    } else {
        println!("{artifact}");
    }
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut args = args.into_iter().peekable();
    let mut mode = None;
    let mut output = None;
    let mut require_single_cpu = false;
    let mut warmup_rounds = None;
    let mut measured_rounds = None;
    let mut hot_reads = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--require-single-cpu" => require_single_cpu = true,
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            "--warmup-rounds" => {
                warmup_rounds = Some(parse_positive(
                    &next_value(&mut args, "--warmup-rounds")?,
                    "warmup rounds",
                )?);
            }
            "--measured-rounds" => {
                measured_rounds = Some(parse_positive(
                    &next_value(&mut args, "--measured-rounds")?,
                    "measured rounds",
                )?);
            }
            "--hot-reads" => {
                hot_reads = Some(parse_positive(
                    &next_value(&mut args, "--hot-reads")?,
                    "hot reads",
                )?);
            }
            "--help" | "-h" => {
                return Err("usage: resident_world_lifecycle_bench (--smoke|--full) [--output PATH] [--require-single-cpu] [--warmup-rounds N] [--measured-rounds N] [--hot-reads N]".to_owned());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    config.require_single_cpu = require_single_cpu;
    if let Some(value) = warmup_rounds {
        config.warmup_rounds = value;
    }
    if let Some(value) = measured_rounds {
        config.measured_rounds = value;
    }
    if let Some(value) = hot_reads {
        config.hot_reads = value;
    }
    Ok(config)
}

fn set_mode(slot: &mut Option<Mode>, value: Mode) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err("specify exactly one benchmark mode".to_owned());
    }
    Ok(())
}

fn next_value(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    flag: &str,
) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_positive(value: &str, label: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid {label}: {value}"))?;
    if parsed == 0 {
        return Err(format!("{label} must be positive"));
    }
    Ok(parsed)
}

fn smoke_cases() -> Vec<CaseSpec> {
    vec![
        CaseSpec {
            name: "resident-1x1-positive",
            origin: ChunkPos { x: 4, z: 4 },
            side: 1,
        },
        CaseSpec {
            name: "resident-3x3-signed",
            origin: ChunkPos { x: -1, z: -1 },
            side: 3,
        },
    ]
}

fn full_cases() -> Vec<CaseSpec> {
    vec![
        CaseSpec {
            name: "resident-1x1-positive",
            origin: ChunkPos { x: 4, z: 4 },
            side: 1,
        },
        CaseSpec {
            name: "resident-3x3-signed",
            origin: ChunkPos { x: -1, z: -1 },
            side: 3,
        },
        CaseSpec {
            name: "resident-5x5-mixed",
            origin: ChunkPos { x: -17, z: 9 },
            side: 5,
        },
        CaseSpec {
            name: "resident-9x9-negative",
            origin: ChunkPos { x: -29, z: -29 },
            side: 9,
        },
    ]
}

fn standard_profile() -> Result<DimensionRuntimeProfile, String> {
    DimensionRuntimeProfile::new(
        DimensionTypeId(1),
        STANDARD_MIN_BLOCK_Y,
        STANDARD_HEIGHT,
        true,
    )
    .map_err(|error| format!("could not build standard dimension profile: {error:?}"))
}

fn bench_case(
    spec: CaseSpec,
    profile: DimensionRuntimeProfile,
    config: &Config,
) -> Result<CaseReport, String> {
    let chunk_count = spec.chunk_count()?;
    let lifecycle_checksum = lifecycle_transaction(spec, profile)?.1;
    let (mut hot_dimension, hot_handles) = build_resident_dimension(spec, profile)?;
    let hot_handle = *hot_handles
        .first()
        .ok_or_else(|| format!("{} produced no resident handles", spec.name))?;
    let hot_trace = build_hot_trace(hot_handle.position, config.hot_reads)?;
    let hot_checksum = validate_hot_equivalence(&hot_dimension, hot_handle, &hot_trace)?;

    for _ in 0..config.warmup_rounds {
        black_box(lifecycle_transaction(spec, profile)?);
    }
    let mut lifecycle_samples = Vec::with_capacity(config.measured_rounds);
    for round in 0..config.measured_rounds {
        let (elapsed_ns, checksum) = lifecycle_transaction(spec, profile)?;
        if checksum != lifecycle_checksum {
            return Err(format!(
                "{} lifecycle checksum drifted between rounds: expected {lifecycle_checksum}, got {checksum}",
                spec.name
            ));
        }
        lifecycle_samples.push(LifecycleSample { round, elapsed_ns });
    }

    for round in 0..config.warmup_rounds {
        let repeated_first = round % 2 == 0;
        let (repeated_ns, resolve_once_ns, checksum) =
            run_hot_pair(&hot_dimension, hot_handle, &hot_trace, repeated_first)?;
        black_box((repeated_ns, resolve_once_ns, checksum));
    }

    let mut hot_samples = Vec::with_capacity(config.measured_rounds);
    for round in 0..config.measured_rounds {
        let repeated_first = round % 2 == 0;
        let (repeated_resolve_ns, resolve_once_ns, checksum) =
            run_hot_pair(&hot_dimension, hot_handle, &hot_trace, repeated_first)?;
        if checksum != hot_checksum {
            return Err(format!(
                "{} HOT checksum drifted between rounds: expected {hot_checksum}, got {checksum}",
                spec.name
            ));
        }
        hot_samples.push(HotPairSample {
            round,
            repeated_first,
            repeated_resolve_ns,
            resolve_once_ns,
        });
    }

    for handle in hot_handles {
        black_box(
            hot_dimension
                .unload_chunk(handle)
                .map_err(|error| format!("HOT cleanup unload failed: {error:?}"))?,
        );
    }
    if hot_dimension.resident_chunk_count() != 0 {
        return Err(format!("{} HOT cleanup left resident chunks", spec.name));
    }

    let lifecycle_summary = summarize(
        lifecycle_samples
            .iter()
            .map(|sample| sample.elapsed_ns)
            .collect(),
    );
    let repeated_summary = summarize(
        hot_samples
            .iter()
            .map(|sample| sample.repeated_resolve_ns)
            .collect(),
    );
    let resolve_once_summary = summarize(
        hot_samples
            .iter()
            .map(|sample| sample.resolve_once_ns)
            .collect(),
    );

    Ok(CaseReport {
        spec,
        chunk_count,
        lifecycle_checksum,
        hot_checksum,
        lifecycle_samples,
        hot_samples,
        lifecycle_summary,
        repeated_summary,
        resolve_once_summary,
    })
}

fn lifecycle_transaction(
    spec: CaseSpec,
    profile: DimensionRuntimeProfile,
) -> Result<(u128, u64), String> {
    let first_epoch = prepare_epoch(spec, profile)?;
    let second_epoch = prepare_epoch(spec, profile)?;
    let chunk_count = spec.chunk_count()?;
    let mut dimension = BenchDimension::with_chunk_capacity(DimensionId(7), profile, chunk_count);

    let start = Instant::now();
    let first_handles = load_epoch(&mut dimension, first_epoch)?;
    let mut checksum = 0_u64;
    for handle in &first_handles {
        let discovered = dimension
            .discover_chunk(handle.position)
            .ok_or_else(|| format!("{} resident discovery missed loaded chunk", spec.name))?;
        if discovered != *handle {
            return Err(format!(
                "{} resident discovery returned wrong handle",
                spec.name
            ));
        }
        checksum = mix_handle(checksum, discovered);
        checksum = mix_state(checksum, read_probe(&dimension, *handle)?);
    }
    for handle in first_handles.iter().copied() {
        black_box(
            dimension
                .unload_chunk(handle)
                .map_err(|error| format!("first-epoch unload failed: {error:?}"))?,
        );
    }
    if dimension.resident_chunk_count() != 0 {
        return Err(format!(
            "{} first unload did not drain residency",
            spec.name
        ));
    }

    let second_handles = load_epoch(&mut dimension, second_epoch)?;
    if second_handles.len() != first_handles.len() {
        return Err(format!("{} reload handle count changed", spec.name));
    }
    for (old, current) in first_handles
        .iter()
        .copied()
        .zip(second_handles.iter().copied())
    {
        if old.position != current.position || old.generation == current.generation {
            return Err(format!("{} reload identity did not advance", spec.name));
        }
        if !matches!(
            dimension.resolve_chunk(old),
            Err(ResidentChunkAccessError::StaleGeneration { .. })
        ) {
            return Err(format!("{} stale handle was not rejected", spec.name));
        }
        checksum = mix_handle(checksum, current);
        checksum = mix_state(checksum, read_probe(&dimension, current)?);
    }
    for handle in second_handles {
        black_box(
            dimension
                .unload_chunk(handle)
                .map_err(|error| format!("second-epoch unload failed: {error:?}"))?,
        );
    }
    if dimension.resident_chunk_count() != 0 {
        return Err(format!(
            "{} second unload did not drain residency",
            spec.name
        ));
    }

    let elapsed_ns = start.elapsed().as_nanos();
    black_box(checksum);
    Ok((elapsed_ns, checksum))
}

fn build_resident_dimension(
    spec: CaseSpec,
    profile: DimensionRuntimeProfile,
) -> Result<(BenchDimension, Vec<ResidentChunkHandle>), String> {
    let mut dimension =
        BenchDimension::with_chunk_capacity(DimensionId(9), profile, spec.chunk_count()?);
    let handles = load_epoch(&mut dimension, prepare_epoch(spec, profile)?)?;
    Ok((dimension, handles))
}

fn load_epoch(
    dimension: &mut BenchDimension,
    epoch: Vec<PreparedChunk>,
) -> Result<Vec<ResidentChunkHandle>, String> {
    let mut handles = Vec::with_capacity(epoch.len());
    for chunk in epoch {
        handles.push(
            dimension
                .load_chunk(chunk.position, chunk.sections)
                .map_err(|error| format!("resident load failed: {error:?}"))?,
        );
    }
    Ok(handles)
}

fn prepare_epoch(
    spec: CaseSpec,
    profile: DimensionRuntimeProfile,
) -> Result<Vec<PreparedChunk>, String> {
    let mut chunks = Vec::with_capacity(spec.chunk_count()?);
    for z_offset in 0..spec.side {
        for x_offset in 0..spec.side {
            let x_offset =
                i32::try_from(x_offset).map_err(|_| "chunk X offset overflow".to_owned())?;
            let z_offset =
                i32::try_from(z_offset).map_err(|_| "chunk Z offset overflow".to_owned())?;
            let position = ChunkPos {
                x: spec
                    .origin
                    .x
                    .checked_add(x_offset)
                    .ok_or_else(|| "chunk X coordinate overflow".to_owned())?,
                z: spec
                    .origin
                    .z
                    .checked_add(z_offset)
                    .ok_or_else(|| "chunk Z coordinate overflow".to_owned())?,
            };
            let mut sections = Vec::with_capacity(profile.section_count());
            for section_index in 0..profile.section_count() {
                sections.push(DirectBlockSection::filled(
                    state_for(position, section_index)?,
                    &GeneratedStateFacts,
                ));
            }
            chunks.push(PreparedChunk { position, sections });
        }
    }
    Ok(chunks)
}

fn state_for(position: ChunkPos, section_index: usize) -> Result<BlockStateId, String> {
    if BLOCK_STATE_COUNT <= 1 {
        return Err("generated block-state universe is unexpectedly empty".to_owned());
    }
    let section_index =
        u64::try_from(section_index).map_err(|_| "section index does not fit u64".to_owned())?;
    let mixed = i64::from(position.x)
        .unsigned_abs()
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(
            i64::from(position.z)
                .unsigned_abs()
                .wrapping_mul(0x85EB_CA77),
        )
        .wrapping_add(section_index.wrapping_mul(0xC2B2_AE3D));
    let non_air = u64::try_from(BLOCK_STATE_COUNT - 1)
        .map_err(|_| "block-state count overflow".to_owned())?;
    let raw = u32::try_from(mixed % non_air + 1)
        .map_err(|_| "block-state identity does not fit u32".to_owned())?;
    BlockStateId::new(raw).ok_or_else(|| format!("invalid generated block-state identity: {raw}"))
}

fn read_probe(
    dimension: &BenchDimension,
    handle: ResidentChunkHandle,
) -> Result<BlockStateId, String> {
    let chunk = dimension
        .resolve_chunk(handle)
        .map_err(|error| format!("resident resolve failed: {error:?}"))?;
    let pos = probe_position(handle.position)?;
    chunk
        .get_block(pos)
        .map_err(|error| format!("probe block read failed: {error:?}"))
}

fn probe_position(position: ChunkPos) -> Result<BlockPos, String> {
    Ok(BlockPos {
        x: position
            .x
            .checked_mul(BLOCKS_PER_CHUNK_AXIS)
            .and_then(|value| value.checked_add(7))
            .ok_or_else(|| "probe X coordinate overflow".to_owned())?,
        y: 64,
        z: position
            .z
            .checked_mul(BLOCKS_PER_CHUNK_AXIS)
            .and_then(|value| value.checked_add(11))
            .ok_or_else(|| "probe Z coordinate overflow".to_owned())?,
    })
}

fn build_hot_trace(position: ChunkPos, count: usize) -> Result<Vec<BlockPos>, String> {
    let base_x = position
        .x
        .checked_mul(BLOCKS_PER_CHUNK_AXIS)
        .ok_or_else(|| "HOT trace X base overflow".to_owned())?;
    let base_z = position
        .z
        .checked_mul(BLOCKS_PER_CHUNK_AXIS)
        .ok_or_else(|| "HOT trace Z base overflow".to_owned())?;
    let mut trace = Vec::with_capacity(count);
    for index in 0..count {
        let index_i32 = i32::try_from(index % 384)
            .map_err(|_| "HOT trace vertical index overflow".to_owned())?;
        let local_x = i32::try_from(index.wrapping_mul(13) & 15)
            .map_err(|_| "HOT local X overflow".to_owned())?;
        let local_z = i32::try_from(index.wrapping_mul(7) & 15)
            .map_err(|_| "HOT local Z overflow".to_owned())?;
        trace.push(BlockPos {
            x: base_x
                .checked_add(local_x)
                .ok_or_else(|| "HOT X coordinate overflow".to_owned())?,
            y: STANDARD_MIN_BLOCK_Y + index_i32,
            z: base_z
                .checked_add(local_z)
                .ok_or_else(|| "HOT Z coordinate overflow".to_owned())?,
        });
    }
    Ok(trace)
}

fn validate_hot_equivalence(
    dimension: &BenchDimension,
    handle: ResidentChunkHandle,
    trace: &[BlockPos],
) -> Result<u64, String> {
    let repeated = repeated_resolve_checksum(dimension, handle, trace)?;
    let resolved = resolve_once_checksum(dimension, handle, trace)?;
    if repeated != resolved {
        return Err(format!(
            "HOT semantic mismatch: repeated={repeated}, resolve-once={resolved}"
        ));
    }
    Ok(repeated)
}

fn run_hot_pair(
    dimension: &BenchDimension,
    handle: ResidentChunkHandle,
    trace: &[BlockPos],
    repeated_first: bool,
) -> Result<(u128, u128, u64), String> {
    if repeated_first {
        let (repeated_ns, repeated_checksum) = time_repeated_resolve(dimension, handle, trace)?;
        let (resolve_once_ns, resolved_checksum) = time_resolve_once(dimension, handle, trace)?;
        require_same_hot_checksum(repeated_checksum, resolved_checksum)?;
        Ok((repeated_ns, resolve_once_ns, repeated_checksum))
    } else {
        let (resolve_once_ns, resolved_checksum) = time_resolve_once(dimension, handle, trace)?;
        let (repeated_ns, repeated_checksum) = time_repeated_resolve(dimension, handle, trace)?;
        require_same_hot_checksum(repeated_checksum, resolved_checksum)?;
        Ok((repeated_ns, resolve_once_ns, repeated_checksum))
    }
}

fn require_same_hot_checksum(repeated: u64, resolved: u64) -> Result<(), String> {
    if repeated == resolved {
        Ok(())
    } else {
        Err(format!(
            "HOT paired semantic mismatch: repeated={repeated}, resolve-once={resolved}"
        ))
    }
}

fn time_repeated_resolve(
    dimension: &BenchDimension,
    handle: ResidentChunkHandle,
    trace: &[BlockPos],
) -> Result<(u128, u64), String> {
    let start = Instant::now();
    let checksum = repeated_resolve_checksum(dimension, handle, trace)?;
    black_box(checksum);
    Ok((start.elapsed().as_nanos(), checksum))
}

fn time_resolve_once(
    dimension: &BenchDimension,
    handle: ResidentChunkHandle,
    trace: &[BlockPos],
) -> Result<(u128, u64), String> {
    let start = Instant::now();
    let checksum = resolve_once_checksum(dimension, handle, trace)?;
    black_box(checksum);
    Ok((start.elapsed().as_nanos(), checksum))
}

fn repeated_resolve_checksum(
    dimension: &BenchDimension,
    handle: ResidentChunkHandle,
    trace: &[BlockPos],
) -> Result<u64, String> {
    let mut checksum = 0_u64;
    for &pos in trace {
        let chunk = dimension
            .resolve_chunk(black_box(handle))
            .map_err(|error| format!("repeated HOT resolve failed: {error:?}"))?;
        let state = chunk
            .get_block(black_box(pos))
            .map_err(|error| format!("repeated HOT block read failed: {error:?}"))?;
        checksum = mix_state(checksum, state);
    }
    Ok(checksum)
}

fn resolve_once_checksum(
    dimension: &BenchDimension,
    handle: ResidentChunkHandle,
    trace: &[BlockPos],
) -> Result<u64, String> {
    let chunk = dimension
        .resolve_chunk(handle)
        .map_err(|error| format!("resolve-once HOT boundary failed: {error:?}"))?;
    let mut checksum = 0_u64;
    for &pos in trace {
        let state = chunk
            .get_block(black_box(pos))
            .map_err(|error| format!("resolve-once HOT block read failed: {error:?}"))?;
        checksum = mix_state(checksum, state);
    }
    Ok(checksum)
}

fn mix_handle(mut checksum: u64, handle: ResidentChunkHandle) -> u64 {
    checksum = checksum.rotate_left(7) ^ handle.generation.0;
    checksum = checksum.rotate_left(11) ^ i64::from(handle.position.x).cast_unsigned();
    checksum.rotate_left(13) ^ i64::from(handle.position.z).cast_unsigned()
}

fn mix_state(checksum: u64, state: BlockStateId) -> u64 {
    let value = u64::try_from(state.as_usize()).unwrap_or(u64::MAX);
    checksum.rotate_left(9) ^ value.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn summarize(mut samples: Vec<u128>) -> Summary {
    samples.sort_unstable();
    Summary {
        p50: percentile(&samples, 50),
        p95: percentile(&samples, 95),
        p99: percentile(&samples, 99),
        max: samples.last().copied().unwrap_or(0),
    }
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index]
}

fn render_report(
    config: &Config,
    hardware: &HardwareMetadata,
    profile: DimensionRuntimeProfile,
    reports: &[CaseReport],
) -> String {
    let mut output = String::new();
    write!(output, "{{\"schema\":{SCHEMA},\"benchmark\":").expect("String writes cannot fail");
    push_json_string(&mut output, "r2c-resident-world-lifecycle");
    write!(output, ",\"mode\":").expect("String writes cannot fail");
    push_json_string(&mut output, config.mode.as_str());
    write!(
        output,
        ",\"hosted_ci_is_diagnostic_only\":true,\"timing_threshold_selected\":false,\"production_path_unchanged\":true,\"warmup_rounds\":{},\"measured_rounds\":{},\"hot_reads\":{},\"structural\":{{\"resident_handle_bytes\":{},\"dimension_profile_bytes\":{},\"profile_section_count\":{},\"repeated_resolutions_per_hot_sample\":{},\"resolve_once_resolutions_per_hot_sample\":1}},\"hardware\":{},\"cases\":[",
        config.warmup_rounds,
        config.measured_rounds,
        config.hot_reads,
        size_of::<ResidentChunkHandle>(),
        size_of::<DimensionRuntimeProfile>(),
        profile.section_count(),
        config.hot_reads,
        hardware.to_json(),
    )
    .expect("String writes cannot fail");

    for (index, report) in reports.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        render_case(&mut output, report);
    }
    output.push_str("]}\n");
    output
}

fn render_case(output: &mut String, report: &CaseReport) {
    output.push('{');
    push_json_string(output, "name");
    output.push(':');
    push_json_string(output, report.spec.name);
    write!(
        output,
        ",\"side\":{},\"chunk_count\":{},\"semantic_equivalent\":true,\"stale_rejections_per_lifecycle\":{},\"lifecycle_checksum\":{},\"hot_checksum\":{},\"lifecycle_summary_ns\":",
        report.spec.side,
        report.chunk_count,
        report.chunk_count,
        report.lifecycle_checksum,
        report.hot_checksum,
    )
    .expect("String writes cannot fail");
    render_summary(output, report.lifecycle_summary);
    output.push_str(",\"repeated_resolve_summary_ns\":");
    render_summary(output, report.repeated_summary);
    output.push_str(",\"resolve_once_summary_ns\":");
    render_summary(output, report.resolve_once_summary);

    output.push_str(",\"lifecycle_samples_ns\":[");
    for (index, sample) in report.lifecycle_samples.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(
            output,
            "{{\"round\":{},\"elapsed_ns\":{}}}",
            sample.round, sample.elapsed_ns
        )
        .expect("String writes cannot fail");
    }
    output.push_str("],\"hot_pairs\":[");
    for (index, sample) in report.hot_samples.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(
            output,
            "{{\"round\":{},\"repeated_first\":{},\"repeated_resolve_ns\":{},\"resolve_once_ns\":{}}}",
            sample.round,
            sample.repeated_first,
            sample.repeated_resolve_ns,
            sample.resolve_once_ns,
        )
        .expect("String writes cannot fail");
    }
    output.push_str("]}");
}

fn render_summary(output: &mut String, summary: Summary) {
    write!(
        output,
        "{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}}",
        summary.p50, summary.p95, summary.p99, summary.max
    )
    .expect("String writes cannot fail");
}
