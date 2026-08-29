use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use helve_benchmark_support::{collect_hardware_metadata, push_json_string};
use helve_cold_load_qualification::{
    ColdLoadHarness, ColdLoadSample, ColdLoadStructure, FIXTURE_COMPRESSED_BYTES,
    FIXTURE_DECOMPRESSED_BYTES, FIXTURE_NBT_SHA256, FIXTURE_ZLIB_SHA256,
};

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
    warmup_rounds: usize,
    measured_rounds: usize,
}

impl Config {
    const fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                require_single_cpu: false,
                warmup_rounds: 3,
                measured_rounds: 20,
            },
            Mode::Full => Self {
                mode,
                output: None,
                require_single_cpu: false,
                warmup_rounds: 10,
                measured_rounds: 100,
            },
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
        eprintln!("R2C cold-load benchmark failed: {error}");
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

    let mut harness = ColdLoadHarness::new()?;
    harness.validate_semantics()?;
    for _ in 0..config.warmup_rounds {
        let _ = harness.sample()?;
    }

    let mut samples = Vec::with_capacity(config.measured_rounds);
    let mut structure = None;
    for _ in 0..config.measured_rounds {
        let (sample, observed) = harness.sample()?;
        if let Some(expected) = structure {
            if expected != observed {
                return Err(format!(
                    "structural witness changed between samples: {expected:?} != {observed:?}"
                ));
            }
        } else {
            structure = Some(observed);
        }
        samples.push(sample);
    }
    let structure = structure.ok_or_else(|| "no measured samples".to_owned())?;
    let artifact = render_report(&config, &hardware.to_json(), &samples, structure);
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
                return Err("usage: cold_load_bench (--smoke|--full) [--output PATH] [--require-single-cpu] [--warmup-rounds N] [--measured-rounds N]".to_owned());
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }

    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    config.require_single_cpu = require_single_cpu;
    if let Some(rounds) = warmup_rounds {
        config.warmup_rounds = rounds;
    }
    if let Some(rounds) = measured_rounds {
        config.measured_rounds = rounds;
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
    let index = (sorted.len() - 1) * percentile / 100;
    sorted[index]
}

fn render_report(
    config: &Config,
    hardware_json: &str,
    samples: &[ColdLoadSample],
    structure: ColdLoadStructure,
) -> String {
    let import = summarize(samples.iter().map(|sample| sample.import_ns));
    let install = summarize(samples.iter().map(|sample| sample.install_ns));
    let total = summarize(samples.iter().map(|sample| sample.total_ns));
    let unload = summarize(samples.iter().map(|sample| sample.unload_drop_ns));
    let mut output = String::with_capacity(16 * 1024);
    output.push('{');
    write!(&mut output, "\"schema\":{SCHEMA},").expect("String write");
    output.push_str("\"benchmark\":\"r2c-cold-load-to-residency\",");
    output.push_str("\"mode\":");
    push_json_string(&mut output, config.mode.as_str());
    output.push(',');
    output.push_str("\"hosted_ci_is_diagnostic_only\":true,");
    output.push_str("\"timing_threshold_selected\":false,");
    output.push_str("\"performance_admitted\":false,");
    output.push_str("\"production_section_policy_selected\":false,");
    output.push_str("\"filesystem_io_included\":false,");
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

    output.push_str("\"fixture\":{");
    write!(
        &mut output,
        "\"compression\":\"zlib\",\"compressed_bytes\":{FIXTURE_COMPRESSED_BYTES},\"decompressed_bytes\":{FIXTURE_DECOMPRESSED_BYTES},"
    )
    .expect("String write");
    output.push_str("\"nbt_sha256\":");
    push_json_string(&mut output, FIXTURE_NBT_SHA256);
    output.push_str(",\"zlib_sha256\":");
    push_json_string(&mut output, FIXTURE_ZLIB_SHA256);
    output.push_str("},");

    output.push_str("\"structural\":{");
    write!(
        &mut output,
        "\"stored_sections\":{},\"imported_block_sections\":{},\"uniform_sections\":{},\"dense_sections\":{},\"synthesized_empty_sections\":{},\"resident_sections\":{},\"dense_semantic_cell_copies_per_load\":{},\"resident_column_allocations_per_load\":1,\"decoder_retained_output_bytes\":{},\"decoder_retained_output_capacity\":{},\"scratch_palette_capacity\":{},\"scratch_packed_word_capacity\":{},\"scratch_state_capacity\":{}",
        structure.stored_sections,
        structure.imported_block_sections,
        structure.uniform_sections,
        structure.dense_sections,
        structure.synthesized_empty_sections,
        structure.resident_sections,
        structure.dense_semantic_cell_copies,
        structure.decoder_retained_output_bytes,
        structure.decoder_retained_output_capacity,
        structure.section_scratch.palette,
        structure.section_scratch.packed_words,
        structure.section_scratch.states,
    )
    .expect("String write");
    output.push_str("},");

    output.push_str("\"samples_ns\":[");
    for (index, sample) in samples.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(
            &mut output,
            "{{\"import\":{},\"install\":{},\"total\":{},\"unload_drop\":{}}}",
            sample.import_ns, sample.install_ns, sample.total_ns, sample.unload_drop_ns
        )
        .expect("String write");
    }
    output.push_str("],");

    output.push_str("\"summary_ns\":{");
    render_summary(&mut output, "import", import);
    output.push(',');
    render_summary(&mut output, "install", install);
    output.push(',');
    render_summary(&mut output, "total", total);
    output.push(',');
    render_summary(&mut output, "unload_drop", unload);
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
    use super::{Mode, parse_args, summarize};

    #[test]
    fn argument_parser_requires_one_mode_and_positive_rounds() {
        assert!(parse_args(["--smoke".to_owned()]).is_ok());
        assert!(parse_args(["--full".to_owned()]).is_ok());
        assert!(parse_args(Vec::<String>::new()).is_err());
        assert!(parse_args(["--smoke".to_owned(), "--full".to_owned()]).is_err());
        assert!(
            parse_args([
                "--smoke".to_owned(),
                "--measured-rounds".to_owned(),
                "0".to_owned()
            ])
            .is_err()
        );
        let parsed = parse_args([
            "--smoke".to_owned(),
            "--warmup-rounds".to_owned(),
            "2".to_owned(),
            "--measured-rounds".to_owned(),
            "7".to_owned(),
        ])
        .expect("valid config");
        assert_eq!(parsed.mode, Mode::Smoke);
        assert_eq!(parsed.warmup_rounds, 2);
        assert_eq!(parsed.measured_rounds, 7);
    }

    #[test]
    fn percentile_summary_is_monotone() {
        let summary = summarize([9, 1, 7, 3, 5]);
        assert!(summary.p50 <= summary.p95);
        assert!(summary.p95 <= summary.p99);
        assert!(summary.p99 <= summary.max);
    }
}
