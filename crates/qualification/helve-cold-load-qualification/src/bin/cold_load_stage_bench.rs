use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use helve_benchmark_support::{collect_hardware_metadata, push_json_string};
use helve_cold_load_qualification::{ColdLoadHarness, ColdLoadStageSample};

const SCHEMA: u32 = 1;

#[derive(Clone, Debug)]
struct Config {
    output: Option<PathBuf>,
    require_single_cpu: bool,
    warmup_rounds: usize,
    measured_rounds: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            output: None,
            require_single_cpu: false,
            warmup_rounds: 3,
            measured_rounds: 20,
        }
    }
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
        eprintln!("R2C cold-load stage benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let hardware = collect_hardware_metadata()?;
    if config.require_single_cpu && hardware.single_allowed_cpu().is_none() {
        return Err(format!(
            "stage qualification requires exactly one allowed logical CPU; observed {}",
            hardware.cpus_allowed_list
        ));
    }

    let mut harness = ColdLoadHarness::new()?;
    harness.validate_semantics()?;
    for _ in 0..config.warmup_rounds {
        let _ = harness.stage_sample()?;
    }

    let mut samples = Vec::with_capacity(config.measured_rounds);
    for _ in 0..config.measured_rounds {
        samples.push(harness.stage_sample()?);
    }
    let artifact = render_report(&config, &hardware.to_json(), &samples);
    if let Some(path) = config.output {
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
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
    let mut config = Config::default();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--output" => config.output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            "--require-single-cpu" => config.require_single_cpu = true,
            "--warmup-rounds" => {
                config.warmup_rounds = parse_positive(
                    &next_value(&mut args, "--warmup-rounds")?,
                    "warmup rounds",
                )?;
            }
            "--measured-rounds" => {
                config.measured_rounds = parse_positive(
                    &next_value(&mut args, "--measured-rounds")?,
                    "measured rounds",
                )?;
            }
            "--help" | "-h" => {
                return Err("usage: cold_load_stage_bench [--output PATH] [--require-single-cpu] [--warmup-rounds N] [--measured-rounds N]".to_owned());
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(config)
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

fn summarize(values: impl IntoIterator<Item = u128>) -> Summary {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_unstable();
    debug_assert!(!values.is_empty());
    Summary {
        p50: percentile(&values, 50),
        p95: percentile(&values, 95),
        p99: percentile(&values, 99),
        max: values[values.len() - 1],
    }
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    sorted[(sorted.len() - 1) * percentile / 100]
}

fn render_report(config: &Config, hardware_json: &str, samples: &[ColdLoadStageSample]) -> String {
    let framing = summarize(samples.iter().map(|sample| sample.framing_ns));
    let select = summarize(samples.iter().map(|sample| sample.slot_select_ns));
    let decompress = summarize(samples.iter().map(|sample| sample.decompress_ns));
    let decode_build = summarize(samples.iter().map(|sample| sample.decode_build_ns));
    let release = summarize(samples.iter().map(|sample| sample.release_ns));

    let mut output = String::with_capacity(12 * 1024);
    output.push('{');
    write!(&mut output, "\"schema\":{SCHEMA},").expect("String write");
    output.push_str("\"benchmark\":\"r2c-cold-import-stage-breakdown\",");
    output.push_str("\"hosted_ci_is_diagnostic_only\":true,");
    output.push_str("\"performance_admitted\":false,");
    output.push_str("\"timing_threshold_selected\":false,");
    output.push_str("\"semantic_validation_passed\":true,");
    write!(
        &mut output,
        "\"warmup_rounds\":{},\"measured_rounds\":{},",
        config.warmup_rounds, config.measured_rounds
    )
    .expect("String write");
    output.push_str("\"hardware\":");
    output.push_str(hardware_json);
    output.push(',');

    output.push_str("\"samples_ns\":[");
    for (index, sample) in samples.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(
            &mut output,
            "{{\"framing\":{},\"slot_select\":{},\"decompress\":{},\"decode_build\":{},\"release\":{}}}",
            sample.framing_ns,
            sample.slot_select_ns,
            sample.decompress_ns,
            sample.decode_build_ns,
            sample.release_ns,
        )
        .expect("String write");
    }
    output.push_str("],\"summary_ns\":{");
    render_summary(&mut output, "framing", framing);
    output.push(',');
    render_summary(&mut output, "slot_select", select);
    output.push(',');
    render_summary(&mut output, "decompress", decompress);
    output.push(',');
    render_summary(&mut output, "decode_build", decode_build);
    output.push(',');
    render_summary(&mut output, "release", release);
    output.push_str("}}");
    output
}

fn render_summary(output: &mut String, name: &str, summary: Summary) {
    push_json_string(output, name);
    write!(
        output,
        ":{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}}",
        summary.p50, summary.p95, summary.p99, summary.max
    )
    .expect("String write");
}

#[cfg(test)]
mod tests {
    use super::{parse_args, summarize};

    #[test]
    fn arguments_require_positive_rounds() {
        assert!(parse_args(Vec::<String>::new()).is_ok());
        assert!(
            parse_args([
                "--measured-rounds".to_owned(),
                "0".to_owned(),
            ])
            .is_err()
        );
        let config = parse_args([
            "--warmup-rounds".to_owned(),
            "2".to_owned(),
            "--measured-rounds".to_owned(),
            "9".to_owned(),
        ])
        .expect("valid stage config");
        assert_eq!(config.warmup_rounds, 2);
        assert_eq!(config.measured_rounds, 9);
    }

    #[test]
    fn percentile_summary_is_monotone() {
        let summary = summarize([10, 5, 7, 3, 9]);
        assert!(summary.p50 <= summary.p95);
        assert!(summary.p95 <= summary.p99);
        assert!(summary.p99 <= summary.max);
    }
}
