use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use helve_benchmark_support::{collect_hardware_metadata, push_json_string};
use helve_r2c_cold_path_qualification::{
    BuilderCounters, ColdPathFixture, ColdPathSample, ColdPathSession, reference_dimension, sample,
};

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

    const fn defaults(self) -> (usize, usize) {
        match self {
            Self::Smoke => (4, 24),
            Self::Full => (32, 512),
        }
    }
}

#[derive(Debug)]
struct Config {
    mode: Mode,
    warmup_rounds: usize,
    measured_rounds: usize,
    require_single_cpu: bool,
    output: Option<PathBuf>,
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
        eprintln!("R2C cold-path benchmark failed: {error}");
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

    let fixture = ColdPathFixture::new()?;
    let mut session = ColdPathSession::new()?;
    let mut dimension = reference_dimension()?;

    for _ in 0..config.warmup_rounds {
        let _ = sample(&fixture, &mut session, &mut dimension)?;
    }
    let counters_before = session.builder_counters();

    let mut samples = Vec::with_capacity(config.measured_rounds);
    for _ in 0..config.measured_rounds {
        samples.push(sample(&fixture, &mut session, &mut dimension)?);
    }
    let counters_after = session.builder_counters();
    let measured_counters = subtract_counters(counters_after, counters_before)?;

    let import = summarize(samples.iter().map(|sample| sample.import_ns).collect())?;
    let install = summarize(samples.iter().map(|sample| sample.install_ns).collect())?;
    let combined = summarize(samples.iter().map(|sample| sample.combined_ns).collect())?;

    let report = render_report(
        &config,
        &hardware.to_json(),
        fixture.expected_state().as_usize(),
        &samples,
        import,
        install,
        combined,
        measured_counters,
    );
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

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut args = args.into_iter();
    let mut mode = None;
    let mut output = None;
    let mut require_single_cpu = false;
    let mut warmup_rounds = None;
    let mut measured_rounds = None;

    while let Some(argument) = args.next() {
        match argument.as_str() {
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
            "--help" | "-h" => {
                return Err("usage: r2c_cold_path_bench (--smoke|--full) [--output PATH] [--require-single-cpu] [--warmup-rounds N] [--measured-rounds N]".to_owned());
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }

    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let (default_warmup, default_measured) = mode.defaults();
    Ok(Config {
        mode,
        warmup_rounds: warmup_rounds.unwrap_or(default_warmup),
        measured_rounds: measured_rounds.unwrap_or(default_measured),
        require_single_cpu,
        output,
    })
}

fn set_mode(slot: &mut Option<Mode>, value: Mode) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err("specify exactly one benchmark mode".to_owned());
    }
    Ok(())
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
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

fn subtract_counters(
    after: BuilderCounters,
    before: BuilderCounters,
) -> Result<BuilderCounters, String> {
    Ok(BuilderCounters {
        uniform_sections: after
            .uniform_sections
            .checked_sub(before.uniform_sections)
            .ok_or_else(|| "uniform section counter regressed".to_owned())?,
        dense_sections: after
            .dense_sections
            .checked_sub(before.dense_sections)
            .ok_or_else(|| "dense section counter regressed".to_owned())?,
        dense_cell_writes: after
            .dense_cell_writes
            .checked_sub(before.dense_cell_writes)
            .ok_or_else(|| "dense cell counter regressed".to_owned())?,
    })
}

fn summarize(mut values: Vec<u128>) -> Result<Summary, String> {
    if values.is_empty() {
        return Err("cannot summarize zero timing samples".to_owned());
    }
    values.sort_unstable();
    Ok(Summary {
        p50: percentile(&values, 500),
        p95: percentile(&values, 950),
        p99: percentile(&values, 990),
        p999: percentile(&values, 999),
        max: *values.last().unwrap_or(&0),
    })
}

fn percentile(sorted: &[u128], permille: usize) -> u128 {
    let last = sorted.len() - 1;
    let index = last.saturating_mul(permille).div_ceil(1000);
    sorted[index.min(last)]
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[expect(
    clippy::too_many_arguments,
    reason = "qualification rendering receives independent immutable evidence groups"
)]
fn render_report(
    config: &Config,
    hardware_json: &str,
    expected_state: usize,
    samples: &[ColdPathSample],
    import: Summary,
    install: Summary,
    combined: Summary,
    counters: BuilderCounters,
) -> String {
    let mut output = String::new();
    output.push('{');
    push_field_string(
        &mut output,
        "kind",
        "r2c-cold-import-residency-reference-baseline",
        true,
    );
    push_field_u64(&mut output, "schema", 1, false);
    push_field_string(&mut output, "mode", config.mode.as_str(), false);
    push_field_bool(&mut output, "reference_section_builder", true, false);
    push_field_bool(
        &mut output,
        "production_section_policy_selected",
        false,
        false,
    );
    push_field_bool(&mut output, "performance_admitted", false, false);
    push_field_u64(
        &mut output,
        "warmup_rounds",
        usize_to_u64(config.warmup_rounds),
        false,
    );
    push_field_u64(
        &mut output,
        "measured_rounds",
        usize_to_u64(config.measured_rounds),
        false,
    );
    push_field_u64(
        &mut output,
        "expected_state_id",
        usize_to_u64(expected_state),
        false,
    );
    output.push_str(",\"hardware\":");
    output.push_str(hardware_json);
    output.push_str(",\"summary_ns\":{");
    push_summary(&mut output, "import", import, true);
    push_summary(&mut output, "install", install, false);
    push_summary(&mut output, "combined", combined, false);
    output.push('}');
    output.push_str(",\"builder_counters\":{");
    push_field_u64(
        &mut output,
        "uniform_sections",
        counters.uniform_sections,
        true,
    );
    push_field_u64(
        &mut output,
        "dense_sections",
        counters.dense_sections,
        false,
    );
    push_field_u64(
        &mut output,
        "dense_cell_writes",
        counters.dense_cell_writes,
        false,
    );
    output.push('}');
    output.push_str(",\"samples\":[");
    for (index, sample) in samples.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(
            output,
            "{{\"import_ns\":{},\"install_ns\":{},\"combined_ns\":{}}}",
            sample.import_ns, sample.install_ns, sample.combined_ns
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("]}\n");
    output
}

fn push_summary(output: &mut String, name: &str, summary: Summary, first: bool) {
    if !first {
        output.push(',');
    }
    push_json_string(output, name);
    write!(
        output,
        ":{{\"p50\":{},\"p95\":{},\"p99\":{},\"p999\":{},\"max\":{}}}",
        summary.p50, summary.p95, summary.p99, summary.p999, summary.max
    )
    .expect("writing to String cannot fail");
}

fn push_field_string(output: &mut String, name: &str, value: &str, first: bool) {
    if !first {
        output.push(',');
    }
    push_json_string(output, name);
    output.push(':');
    push_json_string(output, value);
}

fn push_field_u64(output: &mut String, name: &str, value: u64, first: bool) {
    if !first {
        output.push(',');
    }
    push_json_string(output, name);
    write!(output, ":{value}").expect("writing to String cannot fail");
}

fn push_field_bool(output: &mut String, name: &str, value: bool, first: bool) {
    if !first {
        output.push(',');
    }
    push_json_string(output, name);
    write!(output, ":{value}").expect("writing to String cannot fail");
}

#[cfg(test)]
mod tests {
    use super::{Mode, parse_args, percentile, summarize};

    #[test]
    fn percentile_summary_is_monotone_and_keeps_max() {
        let summary = summarize(vec![9, 1, 5, 3, 7]).expect("nonempty summary");
        assert!(summary.p50 <= summary.p95);
        assert!(summary.p95 <= summary.p99);
        assert!(summary.p99 <= summary.p999);
        assert!(summary.p999 <= summary.max);
        assert_eq!(summary.max, 9);
        assert_eq!(percentile(&[1, 2, 3, 4], 500), 3);
    }

    #[test]
    fn cli_requires_one_mode_and_positive_rounds() {
        assert!(parse_args(["--smoke".to_owned()]).is_ok());
        assert!(parse_args(["--full".to_owned()]).is_ok());
        assert!(parse_args(Vec::<String>::new()).is_err());
        assert!(parse_args(["--smoke".to_owned(), "--full".to_owned()]).is_err());
        assert!(
            parse_args([
                "--smoke".to_owned(),
                "--measured-rounds".to_owned(),
                "0".to_owned(),
            ])
            .is_err()
        );
        assert_eq!(Mode::Smoke.as_str(), "smoke");
    }
}
