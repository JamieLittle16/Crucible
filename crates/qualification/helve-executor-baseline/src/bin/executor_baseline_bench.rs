use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use helve_benchmark_support::{collect_hardware_metadata, push_json_string};
use helve_executor_baseline::{
    LogicalMemory, PreparedWorkload, RunEvidence, TARGET_STATE_GENERATION_SHA256,
    TARGET_STATE_INPUT_SHA256, read_process_memory, semantic_digest_checksum,
};

const SCHEMA: u32 = 1;
const WORKER_COUNTS: [usize; 3] = [1, 2, 4];
const ORDER_MATRIX: [[usize; 3]; 6] = [
    [1, 2, 4],
    [1, 4, 2],
    [2, 1, 4],
    [2, 4, 1],
    [4, 1, 2],
    [4, 2, 1],
];

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
    domains: usize,
    stages: usize,
    operations_per_domain: usize,
    warmup_rounds: usize,
    measured_rounds: usize,
}

impl Config {
    fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                domains: 8,
                stages: 4,
                operations_per_domain: 2_048,
                warmup_rounds: 1,
                measured_rounds: 3,
            },
            Mode::Full => Self {
                mode,
                output: None,
                domains: 64,
                stages: 16,
                operations_per_domain: 8_192,
                warmup_rounds: 3,
                measured_rounds: 18,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct RoundSample {
    round: usize,
    order_position: usize,
    workers: usize,
    elapsed_ns: u128,
    rss_before_kib: Option<u64>,
    rss_after_kib: Option<u64>,
    hwm_after_kib: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
struct Summary {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("executor baseline benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let hardware = collect_hardware_metadata()?;
    let workload =
        PreparedWorkload::new(config.domains, config.stages, config.operations_per_domain)?;

    if config.domains < 4 {
        return Err("1/2/4-worker qualification requires at least four domains".to_owned());
    }

    let reference = workload.clone().execute(1)?;
    let reference_final_digest = semantic_digest_checksum(
        reference
            .stage_digests
            .last()
            .ok_or_else(|| "reference run produced no semantic stages".to_owned())?,
    );

    for round in 0..config.warmup_rounds {
        for &workers in &ORDER_MATRIX[round % ORDER_MATRIX.len()] {
            let candidate = workload.clone().execute(workers)?;
            require_equivalent(&reference, &candidate)?;
        }
    }

    let mut samples =
        Vec::with_capacity(config.measured_rounds.saturating_mul(WORKER_COUNTS.len()));
    for round in 0..config.measured_rounds {
        let order = ORDER_MATRIX[(round + config.warmup_rounds) % ORDER_MATRIX.len()];
        for (order_position, workers) in order.into_iter().enumerate() {
            let prepared = workload.clone();
            let before = read_process_memory();
            let start = Instant::now();
            let candidate = prepared.execute(workers)?;
            let elapsed_ns = start.elapsed().as_nanos();
            let after = read_process_memory();
            require_equivalent(&reference, &candidate)?;
            samples.push(RoundSample {
                round,
                order_position,
                workers,
                elapsed_ns,
                rss_before_kib: before.rss_kib,
                rss_after_kib: after.rss_kib,
                hwm_after_kib: after.hwm_kib,
            });
        }
    }

    let one_summary = summarize_for(&samples, 1)?;
    let report = render_report(
        &config,
        &hardware.to_json(),
        &workload,
        &reference,
        reference_final_digest,
        &samples,
        one_summary,
    )?;

    if let Some(path) = config.output {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        fs::write(&path, report)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        eprintln!("wrote {}", path.display());
    } else {
        println!("{report}");
    }
    Ok(())
}

fn require_equivalent(reference: &RunEvidence, candidate: &RunEvidence) -> Result<(), String> {
    if reference.stage_digests != candidate.stage_digests {
        return Err(format!(
            "semantic stage digests diverged for {} workers",
            candidate.workers
        ));
    }
    if reference.work_checksum != candidate.work_checksum {
        return Err(format!(
            "useful-work checksum diverged for {} workers",
            candidate.workers
        ));
    }
    if reference.useful_operations != candidate.useful_operations {
        return Err(format!(
            "useful-operation count diverged for {} workers",
            candidate.workers
        ));
    }
    Ok(())
}

fn summarize_for(samples: &[RoundSample], workers: usize) -> Result<Summary, String> {
    let values = samples
        .iter()
        .filter(|sample| sample.workers == workers)
        .map(|sample| sample.elapsed_ns)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(format!("no measured samples for {workers} workers"));
    }
    Ok(summarize(values))
}

fn summarize(mut values: Vec<u128>) -> Summary {
    values.sort_unstable();
    Summary {
        p50: percentile(&values, 50),
        p95: percentile(&values, 95),
        p99: percentile(&values, 99),
        max: values.last().copied().unwrap_or(0),
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
    hardware_json: &str,
    workload: &PreparedWorkload,
    reference: &RunEvidence,
    reference_final_digest: u64,
    samples: &[RoundSample],
    one_summary: Summary,
) -> Result<String, String> {
    let mut output = String::new();
    write!(output, "{{\n  \"schema\":{SCHEMA}").expect("writing to String cannot fail");
    output.push_str(",\n  \"benchmark\":\"executor-worker-memory-baseline\",\n  \"mode\":");
    push_json_string(&mut output, config.mode.as_str());
    output.push_str(",\n  \"hosted_ci_is_diagnostic_only\":true");
    output.push_str(",\n  \"target_state_input_sha256\":");
    push_json_string(&mut output, TARGET_STATE_INPUT_SHA256);
    output.push_str(",\n  \"target_state_generation_sha256\":");
    push_json_string(&mut output, TARGET_STATE_GENERATION_SHA256);
    output.push_str(",\n  \"hardware\":");
    output.push_str(hardware_json);
    write!(
        output,
        ",\n  \"settings\":{{\"domains\":{},\"stages\":{},\"operations_per_domain\":{},\"warmup_rounds\":{},\"measured_rounds\":{}}}",
        config.domains,
        config.stages,
        config.operations_per_domain,
        config.warmup_rounds,
        config.measured_rounds
    )
    .expect("writing to String cannot fail");
    write!(
        output,
        ",\n  \"semantic_reference\":{{\"useful_operations\":{},\"work_checksum\":{},\"final_digest_checksum\":{},\"stage_count\":{}}}",
        reference.useful_operations,
        reference.work_checksum,
        reference_final_digest,
        reference.stage_digests.len()
    )
    .expect("writing to String cannot fail");

    output.push_str(",\n  \"candidates\":[");
    for (index, workers) in WORKER_COUNTS.into_iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        let summary = summarize_for(samples, workers)?;
        let speedup_millionths = ratio_millionths(one_summary.p50, summary.p50);
        let parallel_efficiency_millionths = speedup_millionths
            .checked_div(u128::try_from(workers).map_err(|_| "worker count overflow".to_owned())?)
            .unwrap_or(0);
        let core_ns_per_operation = summary
            .p50
            .checked_mul(u128::try_from(workers).map_err(|_| "worker count overflow".to_owned())?)
            .and_then(|value| value.checked_div(u128::from(reference.useful_operations)))
            .unwrap_or(0);
        let logical = workload.logical_memory(workers);
        write!(
            output,
            "{{\"workers\":{workers},\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{},\"speedup_millionths\":{speedup_millionths},\"parallel_efficiency_millionths\":{parallel_efficiency_millionths},\"p50_core_ns_per_operation\":{core_ns_per_operation},\"logical_memory\":",
            summary.p50, summary.p95, summary.p99, summary.max
        )
        .expect("writing to String cannot fail");
        render_logical_memory(&mut output, logical);
        output.push('}');
    }
    output.push(']');

    output.push_str(",\n  \"rounds\":[");
    for (index, sample) in samples.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(
            output,
            "{{\"round\":{},\"order_position\":{},\"workers\":{},\"elapsed_ns\":{},\"rss_before_kib\":",
            sample.round, sample.order_position, sample.workers, sample.elapsed_ns
        )
        .expect("writing to String cannot fail");
        render_optional_u64(&mut output, sample.rss_before_kib);
        output.push_str(",\"rss_after_kib\":");
        render_optional_u64(&mut output, sample.rss_after_kib);
        output.push_str(",\"hwm_after_kib\":");
        render_optional_u64(&mut output, sample.hwm_after_kib);
        output.push('}');
    }
    output.push_str("]\n}\n");
    Ok(output)
}

fn ratio_millionths(numerator: u128, denominator: u128) -> u128 {
    if denominator == 0 {
        return 0;
    }
    numerator
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_div(denominator))
        .unwrap_or(0)
}

fn render_logical_memory(output: &mut String, logical: LogicalMemory) {
    write!(
        output,
        "{{\"section_cell_bytes\":{},\"domain_shallow_bytes\":{},\"trace_bytes\":{},\"stage_outcome_bytes\":{},\"worker_partition_shallow_bytes\":{},\"total_accounted_bytes\":{}}}",
        logical.section_cell_bytes,
        logical.domain_shallow_bytes,
        logical.trace_bytes,
        logical.stage_outcome_bytes,
        logical.worker_partition_shallow_bytes,
        logical.total_accounted_bytes()
    )
    .expect("writing to String cannot fail");
}

fn render_optional_u64(output: &mut String, value: Option<u64>) {
    match value {
        Some(value) => write!(output, "{value}").expect("writing to String cannot fail"),
        None => output.push_str("null"),
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut args = args.into_iter().peekable();
    let mut mode = None;
    let mut output = None;
    let mut domains = None;
    let mut stages = None;
    let mut operations_per_domain = None;
    let mut warmup_rounds = None;
    let mut measured_rounds = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            "--domains" => {
                domains = Some(parse_positive(
                    &next_value(&mut args, "--domains")?,
                    "domains",
                )?);
            }
            "--stages" => {
                stages = Some(parse_positive(
                    &next_value(&mut args, "--stages")?,
                    "stages",
                )?);
            }
            "--operations-per-domain" => {
                operations_per_domain = Some(parse_positive(
                    &next_value(&mut args, "--operations-per-domain")?,
                    "operations per domain",
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
                return Err("usage: executor_baseline_bench (--smoke|--full) [--output PATH] [--domains N] [--stages N] [--operations-per-domain N] [--warmup-rounds N] [--measured-rounds N]".to_owned());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    if let Some(value) = domains {
        config.domains = value;
    }
    if let Some(value) = stages {
        config.stages = value;
    }
    if let Some(value) = operations_per_domain {
        config.operations_per_domain = value;
    }
    if let Some(value) = warmup_rounds {
        config.warmup_rounds = value;
    }
    if let Some(value) = measured_rounds {
        config.measured_rounds = value;
    }
    Ok(config)
}

fn set_mode(slot: &mut Option<Mode>, mode: Mode) -> Result<(), String> {
    if slot.replace(mode).is_some() {
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

#[cfg(test)]
mod tests {
    use super::{Mode, ORDER_MATRIX, parse_args, percentile, ratio_millionths};

    #[test]
    fn order_matrix_contains_every_worker_once_per_round() {
        for order in ORDER_MATRIX {
            let mut sorted = order;
            sorted.sort_unstable();
            assert_eq!(sorted, [1, 2, 4]);
        }
    }

    #[test]
    fn percentile_and_ratio_are_deterministic() {
        assert_eq!(percentile(&[10, 20, 30, 40, 50], 50), 30);
        assert_eq!(percentile(&[10, 20, 30, 40, 50], 95), 50);
        assert_eq!(ratio_millionths(100, 50), 2_000_000);
    }

    #[test]
    fn parser_requires_mode_and_accepts_overrides() {
        let config = parse_args([
            "--smoke".to_owned(),
            "--domains".to_owned(),
            "12".to_owned(),
            "--stages".to_owned(),
            "3".to_owned(),
        ])
        .expect("valid arguments");
        assert_eq!(config.mode, Mode::Smoke);
        assert_eq!(config.domains, 12);
        assert_eq!(config.stages, 3);
        assert!(parse_args(Vec::<String>::new()).is_err());
    }
}
