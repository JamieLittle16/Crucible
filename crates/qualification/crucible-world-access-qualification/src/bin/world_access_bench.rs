use std::env;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use crucible_benchmark_support::{HardwareMetadata, collect_hardware_metadata, push_json_string};
use crucible_generated::BlockStateId;
use crucible_generated::{STATE_DATA_GENERATION_SHA256, STATE_DATA_INPUT_SHA256};
use crucible_types::BlockPos;
use crucible_world_access_qualification::{
    CaseSpec, PreparedCase, ReferenceRouter, WorldAccessError, full_cases, smoke_cases,
};
use crucible_world_chunk::ResolvedChunkWindow;
use crucible_world_reference::DirectBlockSection;

const SCHEMA: u32 = 1;

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

#[derive(Clone, Debug)]
struct Config {
    mode: Mode,
    output: Option<PathBuf>,
    require_single_cpu: bool,
    query_count: usize,
    warmup_rounds: usize,
    measured_rounds: usize,
    setup_samples: usize,
    amortization_samples: usize,
    amortization_counts: Vec<usize>,
}

impl Config {
    fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                require_single_cpu: false,
                query_count: 16_384,
                warmup_rounds: 2,
                measured_rounds: 12,
                setup_samples: 12,
                amortization_samples: 6,
                amortization_counts: vec![1, 32, 512],
            },
            Mode::Full => Self {
                mode,
                output: None,
                require_single_cpu: false,
                query_count: 262_144,
                warmup_rounds: 8,
                measured_rounds: 64,
                setup_samples: 64,
                amortization_samples: 24,
                amortization_counts: vec![1, 8, 32, 128, 512, 2_048, 8_192],
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PairSample {
    round: usize,
    reference_first: bool,
    reference_ns: u128,
    resolved_ns: u128,
}

#[derive(Clone, Copy, Debug)]
struct WholeCostSample {
    round: usize,
    query_count: usize,
    reference_first: bool,
    reference_ns: u128,
    resolved_total_ns: u128,
}

#[derive(Clone, Debug)]
struct Summary {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

#[derive(Clone, Debug)]
struct CaseReport {
    spec: CaseSpec,
    semantic_checksum: u64,
    pair_samples: Vec<PairSample>,
    setup_samples_ns: Vec<u128>,
    whole_cost_samples: Vec<WholeCostSample>,
    reference_summary: Summary,
    resolved_summary: Summary,
    setup_summary: Summary,
    derived_break_even_queries: Option<u128>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("world-access benchmark failed: {error}");
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

    let specs = match config.mode {
        Mode::Smoke => smoke_cases(),
        Mode::Full => full_cases(),
    };
    let mut reports = Vec::with_capacity(specs.len());
    for spec in specs {
        eprintln!("benchmarking {}", spec.name);
        reports.push(bench_case(spec, &config)?);
    }

    let artifact = render_report(&config, &hardware, &reports);
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
    let mut query_count = None;
    let mut warmup_rounds = None;
    let mut measured_rounds = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--require-single-cpu" => require_single_cpu = true,
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            "--queries" => {
                query_count = Some(parse_positive(
                    &next_value(&mut args, "--queries")?,
                    "queries",
                )?);
            }
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
            "--help" | "-h" => {
                return Err("usage: world_access_bench (--smoke|--full) [--output PATH] [--require-single-cpu] [--queries N] [--warmup-rounds N] [--measured-rounds N]".to_owned());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    config.require_single_cpu = require_single_cpu;
    if let Some(value) = query_count {
        config.query_count = value;
    }
    if let Some(value) = warmup_rounds {
        config.warmup_rounds = value;
    }
    if let Some(value) = measured_rounds {
        config.measured_rounds = value;
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

fn bench_case(spec: CaseSpec, config: &Config) -> Result<CaseReport, String> {
    let case = PreparedCase::new(spec, config.query_count).map_err(world_error)?;
    let semantic_checksum = case.validate_equivalence().map_err(world_error)?;
    let reference = case.reference_router();
    let resolved = case.resolved_window().map_err(world_error)?;

    for round in 0..config.warmup_rounds {
        let reference_first = round % 2 == 0;
        run_pair(
            &reference,
            &resolved,
            case.trace(),
            0,
            case.trace().len(),
            reference_first,
        )?;
    }

    let mut pair_samples = Vec::with_capacity(config.measured_rounds);
    for round in 0..config.measured_rounds {
        let reference_first = round % 2 == 0;
        let (reference_ns, resolved_ns) = run_pair(
            &reference,
            &resolved,
            case.trace(),
            0,
            case.trace().len(),
            reference_first,
        )?;
        pair_samples.push(PairSample {
            round,
            reference_first,
            reference_ns,
            resolved_ns,
        });
    }

    for _ in 0..config.warmup_rounds {
        black_box(build_and_drop_window(&case)?);
    }
    let mut setup_samples_ns = Vec::with_capacity(config.setup_samples);
    for _ in 0..config.setup_samples {
        let start = Instant::now();
        black_box(build_and_drop_window(&case)?);
        setup_samples_ns.push(start.elapsed().as_nanos());
    }

    let mut whole_cost_samples = Vec::new();
    for &query_count in &config.amortization_counts {
        for round in 0..config.amortization_samples {
            let start_index = round
                .wrapping_mul(7_919)
                .wrapping_add(query_count.wrapping_mul(131))
                % case.trace().len();
            let reference_first = round % 2 == 0;
            let (reference_ns, resolved_total_ns) =
                run_whole_cost_pair(&case, &reference, start_index, query_count, reference_first)?;
            whole_cost_samples.push(WholeCostSample {
                round,
                query_count,
                reference_first,
                reference_ns,
                resolved_total_ns,
            });
        }
    }

    let reference_summary = summarize(
        pair_samples
            .iter()
            .map(|sample| sample.reference_ns)
            .collect(),
    );
    let resolved_summary = summarize(
        pair_samples
            .iter()
            .map(|sample| sample.resolved_ns)
            .collect(),
    );
    let setup_summary = summarize(setup_samples_ns.clone());
    let derived_break_even_queries = derive_break_even(
        setup_summary.p50,
        reference_summary.p50,
        resolved_summary.p50,
        case.trace().len(),
    );

    Ok(CaseReport {
        spec,
        semantic_checksum,
        pair_samples,
        setup_samples_ns,
        whole_cost_samples,
        reference_summary,
        resolved_summary,
        setup_summary,
        derived_break_even_queries,
    })
}

fn run_pair(
    reference: &ReferenceRouter<'_>,
    resolved: &ResolvedChunkWindow<'_, BlockStateId, DirectBlockSection<BlockStateId>>,
    trace: &[BlockPos],
    start_index: usize,
    count: usize,
    reference_first: bool,
) -> Result<(u128, u128), String> {
    if reference_first {
        let reference = time_reads(trace, start_index, count, |pos| {
            reference.get_block(pos).map_err(world_error)
        })?;
        let resolved = time_reads(trace, start_index, count, |pos| {
            resolved
                .get_block(pos)
                .map_err(|error| world_error(error.into()))
        })?;
        Ok((reference, resolved))
    } else {
        let resolved_ns = time_reads(trace, start_index, count, |pos| {
            resolved
                .get_block(pos)
                .map_err(|error| world_error(error.into()))
        })?;
        let reference_ns = time_reads(trace, start_index, count, |pos| {
            reference.get_block(pos).map_err(world_error)
        })?;
        Ok((reference_ns, resolved_ns))
    }
}

fn run_whole_cost_pair(
    case: &PreparedCase,
    reference: &ReferenceRouter<'_>,
    start_index: usize,
    count: usize,
    reference_first: bool,
) -> Result<(u128, u128), String> {
    if reference_first {
        let reference_ns = time_reads(case.trace(), start_index, count, |pos| {
            reference.get_block(pos).map_err(world_error)
        })?;
        let resolved_ns = time_resolved_whole_cost(case, start_index, count)?;
        Ok((reference_ns, resolved_ns))
    } else {
        let resolved_ns = time_resolved_whole_cost(case, start_index, count)?;
        let reference_ns = time_reads(case.trace(), start_index, count, |pos| {
            reference.get_block(pos).map_err(world_error)
        })?;
        Ok((reference_ns, resolved_ns))
    }
}

fn time_resolved_whole_cost(
    case: &PreparedCase,
    start_index: usize,
    count: usize,
) -> Result<u128, String> {
    let start = Instant::now();
    let window = case.resolved_window().map_err(world_error)?;
    let checksum = read_checksum(case.trace(), start_index, count, |pos| {
        window
            .get_block(pos)
            .map_err(|error| world_error(error.into()))
    })?;
    black_box(checksum);
    drop(window);
    Ok(start.elapsed().as_nanos())
}

fn build_and_drop_window(case: &PreparedCase) -> Result<usize, String> {
    let window = case.resolved_window().map_err(world_error)?;
    let count = black_box(window.chunk_count());
    drop(window);
    Ok(count)
}

fn time_reads<F>(
    trace: &[BlockPos],
    start_index: usize,
    count: usize,
    read: F,
) -> Result<u128, String>
where
    F: FnMut(BlockPos) -> Result<BlockStateId, String>,
{
    let start = Instant::now();
    let checksum = read_checksum(trace, start_index, count, read)?;
    black_box(checksum);
    Ok(start.elapsed().as_nanos())
}

fn read_checksum<F>(
    trace: &[BlockPos],
    start_index: usize,
    count: usize,
    mut read: F,
) -> Result<u64, String>
where
    F: FnMut(BlockPos) -> Result<BlockStateId, String>,
{
    let mut checksum = 0_u64;
    for offset in 0..count {
        let pos = trace[(start_index + offset) % trace.len()];
        let state = read(black_box(pos))?;
        checksum = checksum.wrapping_add(
            u64::try_from(black_box(state).as_usize())
                .map_err(|_| "state identity does not fit u64".to_owned())?,
        );
    }
    Ok(checksum)
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

fn derive_break_even(
    setup_ns: u128,
    reference_trace_ns: u128,
    resolved_trace_ns: u128,
    trace_queries: usize,
) -> Option<u128> {
    let savings = reference_trace_ns.checked_sub(resolved_trace_ns)?;
    if savings == 0 || trace_queries == 0 {
        return None;
    }
    let queries = u128::try_from(trace_queries).ok()?;
    Some(setup_ns.saturating_mul(queries).div_ceil(savings))
}

fn world_error(error: WorldAccessError) -> String {
    format!("world-access qualification error: {error:?}")
}

fn render_report(config: &Config, hardware: &HardwareMetadata, reports: &[CaseReport]) -> String {
    let mut output = String::new();
    output.push_str("{\n  \"schema\":");
    output.push_str(&SCHEMA.to_string());
    output.push_str(",\n  \"benchmark\":\"resolved-chunk-window\",\n  \"mode\":");
    push_json_string(&mut output, config.mode.as_str());
    output
        .push_str(",\n  \"hosted_ci_is_diagnostic_only\":true,\n  \"target_state_input_sha256\":");
    push_json_string(&mut output, STATE_DATA_INPUT_SHA256);
    output.push_str(",\n  \"target_state_generation_sha256\":");
    push_json_string(&mut output, STATE_DATA_GENERATION_SHA256);
    output.push_str(",\n  \"settings\":{");
    write!(
        output,
        "\"query_count\":{},\"warmup_rounds\":{},\"measured_rounds\":{},\"setup_samples\":{},\"amortization_samples\":{},\"require_single_cpu\":{}",
        config.query_count,
        config.warmup_rounds,
        config.measured_rounds,
        config.setup_samples,
        config.amortization_samples,
        config.require_single_cpu
    )
    .expect("writing to String cannot fail");
    output.push_str(",\"amortization_counts\":[");
    push_usize_array(&mut output, &config.amortization_counts);
    output.push_str("]},\n  \"hardware\":");
    output.push_str(&hardware.to_json());
    output.push_str(",\n  \"cases\":[");
    for (index, report) in reports.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('\n');
        render_case(&mut output, report, config.query_count);
    }
    output.push_str("\n  ]\n}\n");
    output
}

fn render_case(output: &mut String, report: &CaseReport, query_count: usize) {
    output.push_str("    {\"name\":");
    push_json_string(output, report.spec.name);
    write!(
        output,
        ",\"origin_x\":{},\"origin_z\":{},\"width\":{},\"depth\":{},\"semantic_checksum\":{},\"query_count\":{}",
        report.spec.origin.x,
        report.spec.origin.z,
        report.spec.width,
        report.spec.depth,
        report.semantic_checksum,
        query_count
    )
    .expect("writing to String cannot fail");
    output.push_str(",\"reference_summary\":");
    render_summary(output, &report.reference_summary, query_count);
    output.push_str(",\"resolved_summary\":");
    render_summary(output, &report.resolved_summary, query_count);
    output.push_str(",\"setup_summary\":");
    render_summary(output, &report.setup_summary, 1);
    output.push_str(",\"derived_break_even_queries\":");
    match report.derived_break_even_queries {
        Some(value) => output.push_str(&value.to_string()),
        None => output.push_str("null"),
    }
    output.push_str(",\"setup_samples_ns\":[");
    push_u128_array(output, &report.setup_samples_ns);
    output.push_str("],\"paired_rounds\":[");
    for (index, sample) in report.pair_samples.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(
            output,
            "{{\"round\":{},\"reference_first\":{},\"reference_ns\":{},\"resolved_ns\":{}}}",
            sample.round, sample.reference_first, sample.reference_ns, sample.resolved_ns
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("],\"whole_cost_samples\":[");
    for (index, sample) in report.whole_cost_samples.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(
            output,
            "{{\"round\":{},\"query_count\":{},\"reference_first\":{},\"reference_ns\":{},\"resolved_total_ns\":{}}}",
            sample.round,
            sample.query_count,
            sample.reference_first,
            sample.reference_ns,
            sample.resolved_total_ns
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("]}");
}

fn render_summary(output: &mut String, summary: &Summary, operations: usize) {
    let operations = u128::try_from(operations).unwrap_or(1).max(1);
    write!(
        output,
        "{{\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{},\"p50_ns_per_operation\":{}}}",
        summary.p50,
        summary.p95,
        summary.p99,
        summary.max,
        summary.p50 / operations
    )
    .expect("writing to String cannot fail");
}

fn push_u128_array(output: &mut String, values: &[u128]) {
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(&value.to_string());
    }
}

fn push_usize_array(output: &mut String, values: &[usize]) {
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(&value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::{Mode, derive_break_even, parse_args, percentile};

    #[test]
    fn percentile_uses_nearest_rank_ceiling() {
        let values = [10, 20, 30, 40, 50];
        assert_eq!(percentile(&values, 50), 30);
        assert_eq!(percentile(&values, 95), 50);
        assert_eq!(percentile(&values, 99), 50);
    }

    #[test]
    fn break_even_is_none_when_resolved_does_not_win_query_path() {
        assert_eq!(derive_break_even(100, 1_000, 1_100, 100), None);
        assert_eq!(derive_break_even(100, 1_000, 1_000, 100), None);
        assert_eq!(derive_break_even(100, 1_000, 900, 100), Some(100));
    }

    #[test]
    fn parser_requires_explicit_mode_and_accepts_target_affinity_gate() {
        let config = parse_args([
            "--full".to_owned(),
            "--require-single-cpu".to_owned(),
            "--queries".to_owned(),
            "4096".to_owned(),
        ])
        .expect("valid benchmark arguments");
        assert_eq!(config.mode, Mode::Full);
        assert!(config.require_single_cpu);
        assert_eq!(config.query_count, 4_096);
        assert!(parse_args(Vec::<String>::new()).is_err());
    }
}
