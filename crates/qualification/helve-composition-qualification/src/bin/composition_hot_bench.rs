use std::env;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use helve_benchmark_support::{collect_hardware_metadata, push_json_string};
use helve_composition::{COMPOSITION_SHA256, MINECRAFT_VERSION, PROFILE};
use helve_composition_qualification::{
    GeneratedSection, exact_type_identity, generated_get, hand_wired_get,
};
use helve_generated::{
    AIR, BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256,
    STATE_DATA_INPUT_SHA256,
};
use helve_world_contract::{BLOCK_SECTION_CELLS, BlockSection, SectionBlockPos};
use helve_world_reference::DirectBlockSection;

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
    operations: usize,
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
                operations: 65_536,
                warmup_rounds: 2,
                measured_rounds: 12,
            },
            Mode::Full => Self {
                mode,
                output: None,
                require_single_cpu: false,
                operations: 2_097_152,
                warmup_rounds: 8,
                measured_rounds: 64,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PairSample {
    round: usize,
    generated_first: bool,
    hand_wired_ns: u128,
    generated_ns: u128,
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
        eprintln!("composition HOT benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    if !exact_type_identity() {
        return Err("generated composition provider is not the exact hand-wired type".to_owned());
    }

    let hardware = collect_hardware_metadata()?;
    if config.require_single_cpu && hardware.single_allowed_cpu().is_none() {
        return Err(format!(
            "target qualification requires exactly one allowed logical CPU; observed {}",
            hardware.cpus_allowed_list
        ));
    }

    let (section, positions) = build_fixture(config.operations)?;
    let hand_checksum = hand_wired_checksum(&section, &positions);
    let generated_checksum = generated_checksum(&section, &positions);
    if hand_checksum != generated_checksum {
        return Err(format!(
            "semantic checksum mismatch: hand-wired={hand_checksum}, generated={generated_checksum}"
        ));
    }

    for round in 0..config.warmup_rounds {
        let generated_first = round % 2 != 0;
        black_box(measure_pair(&section, &positions, generated_first));
    }

    let mut samples = Vec::with_capacity(config.measured_rounds);
    for round in 0..config.measured_rounds {
        let generated_first = round % 2 != 0;
        let (hand_wired_ns, generated_ns) = measure_pair(&section, &positions, generated_first);
        samples.push(PairSample {
            round,
            generated_first,
            hand_wired_ns,
            generated_ns,
        });
    }

    let hand_summary = summarize(samples.iter().map(|sample| sample.hand_wired_ns).collect());
    let generated_summary = summarize(samples.iter().map(|sample| sample.generated_ns).collect());
    let artifact = render_report(
        &config,
        &hardware.to_json(),
        hand_checksum,
        &samples,
        hand_summary,
        generated_summary,
    );

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
    let mut operations = None;
    let mut warmup_rounds = None;
    let mut measured_rounds = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--require-single-cpu" => require_single_cpu = true,
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            "--operations" => {
                operations = Some(parse_positive(
                    &next_value(&mut args, "--operations")?,
                    "operations",
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
                return Err("usage: composition_hot_bench (--smoke|--full) [--output PATH] [--require-single-cpu] [--operations N] [--warmup-rounds N] [--measured-rounds N]".to_owned());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    config.require_single_cpu = require_single_cpu;
    if let Some(value) = operations {
        config.operations = value;
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

fn build_fixture(operations: usize) -> Result<(GeneratedSection, Vec<SectionBlockPos>), String> {
    let mut section = DirectBlockSection::filled(AIR, &GeneratedStateFacts);
    let state_universe = BLOCK_STATE_COUNT
        .checked_sub(1)
        .ok_or_else(|| "target state universe must contain non-air states".to_owned())?;

    for cell in 0..BLOCK_SECTION_CELLS {
        let pos = position_from_cell(cell)?;
        let raw = (cell.wrapping_mul(37).wrapping_add(11)) % state_universe + 1;
        let raw = u32::try_from(raw).map_err(|_| "state identity does not fit u32".to_owned())?;
        let state = BlockStateId::new(raw)
            .ok_or_else(|| format!("generated invalid target state identity {raw}"))?;
        black_box(section.replace(pos, state, &GeneratedStateFacts));
    }

    let mut positions = Vec::with_capacity(operations);
    let mut rng = 0xD1B5_4A32_D192_ED03_u64;
    for _ in 0..operations {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let cell = usize::try_from(rng % 4_096)
            .map_err(|_| "bounded position identity does not fit usize".to_owned())?;
        positions.push(position_from_cell(cell)?);
    }
    Ok((section, positions))
}

fn position_from_cell(cell: usize) -> Result<SectionBlockPos, String> {
    if cell >= BLOCK_SECTION_CELLS {
        return Err(format!("section cell index out of range: {cell}"));
    }
    let x = u8::try_from(cell & 0x0f).map_err(|_| "x coordinate overflow".to_owned())?;
    let z = u8::try_from((cell >> 4) & 0x0f).map_err(|_| "z coordinate overflow".to_owned())?;
    let y = u8::try_from((cell >> 8) & 0x0f).map_err(|_| "y coordinate overflow".to_owned())?;
    SectionBlockPos::new(x, y, z).ok_or_else(|| "bounded section coordinate rejected".to_owned())
}

fn measure_pair(
    section: &GeneratedSection,
    positions: &[SectionBlockPos],
    generated_first: bool,
) -> (u128, u128) {
    if generated_first {
        let start = Instant::now();
        black_box(generated_checksum(section, positions));
        let generated_ns = start.elapsed().as_nanos();

        let start = Instant::now();
        black_box(hand_wired_checksum(section, positions));
        let hand_wired_ns = start.elapsed().as_nanos();
        (hand_wired_ns, generated_ns)
    } else {
        let start = Instant::now();
        black_box(hand_wired_checksum(section, positions));
        let hand_wired_ns = start.elapsed().as_nanos();

        let start = Instant::now();
        black_box(generated_checksum(section, positions));
        let generated_ns = start.elapsed().as_nanos();
        (hand_wired_ns, generated_ns)
    }
}

fn hand_wired_checksum(section: &GeneratedSection, positions: &[SectionBlockPos]) -> usize {
    let mut checksum = 0x9E37_79B9_usize;
    for &pos in positions {
        let state = hand_wired_get(black_box(section), black_box(pos));
        checksum = checksum.rotate_left(7) ^ black_box(state).as_usize().wrapping_mul(131);
    }
    checksum
}

fn generated_checksum(section: &GeneratedSection, positions: &[SectionBlockPos]) -> usize {
    let mut checksum = 0x9E37_79B9_usize;
    for &pos in positions {
        let state = generated_get(black_box(section), black_box(pos));
        checksum = checksum.rotate_left(7) ^ black_box(state).as_usize().wrapping_mul(131);
    }
    checksum
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
    hardware_json: &str,
    checksum: usize,
    samples: &[PairSample],
    hand: Summary,
    generated: Summary,
) -> String {
    let mut output = String::new();
    write!(output, "{{\n  \"schema\":{SCHEMA}").expect("writing to String cannot fail");
    output.push_str(",\n  \"benchmark\":\"composition-hot-tax\",\n  \"mode\":");
    push_json_string(&mut output, config.mode.as_str());
    output.push_str(",\n  \"hosted_ci_is_diagnostic_only\":true,\n  \"minecraft_version\":");
    push_json_string(&mut output, MINECRAFT_VERSION);
    output.push_str(",\n  \"profile\":");
    push_json_string(&mut output, PROFILE);
    output.push_str(",\n  \"composition_sha256\":");
    push_json_string(&mut output, COMPOSITION_SHA256);
    output.push_str(",\n  \"target_state_input_sha256\":");
    push_json_string(&mut output, STATE_DATA_INPUT_SHA256);
    output.push_str(",\n  \"target_state_generation_sha256\":");
    push_json_string(&mut output, STATE_DATA_GENERATION_SHA256);
    write!(
        output,
        ",\n  \"structural\":{{\"exact_type_identity\":true,\"size_bytes\":{},\"align_bytes\":{}}}",
        core::mem::size_of::<GeneratedSection>(),
        core::mem::align_of::<GeneratedSection>()
    )
    .expect("writing to String cannot fail");
    write!(
        output,
        ",\n  \"settings\":{{\"operations\":{},\"warmup_rounds\":{},\"measured_rounds\":{},\"require_single_cpu\":{}}}",
        config.operations, config.warmup_rounds, config.measured_rounds, config.require_single_cpu
    )
    .expect("writing to String cannot fail");
    output.push_str(",\n  \"hardware\":");
    output.push_str(hardware_json);
    write!(output, ",\n  \"semantic_checksum\":{checksum}").expect("writing to String cannot fail");
    output.push_str(",\n  \"hand_wired_summary\":");
    render_summary(&mut output, hand, config.operations);
    output.push_str(",\n  \"generated_summary\":");
    render_summary(&mut output, generated, config.operations);
    output.push_str(",\n  \"paired_rounds\":[");
    for (index, sample) in samples.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(
            output,
            "{{\"round\":{},\"generated_first\":{},\"hand_wired_ns\":{},\"generated_ns\":{}}}",
            sample.round, sample.generated_first, sample.hand_wired_ns, sample.generated_ns
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("]\n}\n");
    output
}

fn render_summary(output: &mut String, summary: Summary, operations: usize) {
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

#[cfg(test)]
mod tests {
    use super::{Mode, build_fixture, parse_args, percentile};

    #[test]
    fn fixture_builds_irregular_equivalent_work() {
        let (section, positions) = build_fixture(8_192).expect("valid fixture");
        assert_eq!(positions.len(), 8_192);
        assert_ne!(
            super::hand_wired_checksum(&section, &positions),
            0,
            "fixture checksum must retain work"
        );
        assert_eq!(
            super::hand_wired_checksum(&section, &positions),
            super::generated_checksum(&section, &positions)
        );
    }

    #[test]
    fn percentile_uses_nearest_rank_ceiling() {
        let values = [10, 20, 30, 40, 50];
        assert_eq!(percentile(&values, 50), 30);
        assert_eq!(percentile(&values, 95), 50);
        assert_eq!(percentile(&values, 99), 50);
    }

    #[test]
    fn parser_requires_explicit_mode_and_accepts_affinity_gate() {
        let config = parse_args([
            "--full".to_owned(),
            "--require-single-cpu".to_owned(),
            "--operations".to_owned(),
            "4096".to_owned(),
        ])
        .expect("valid benchmark arguments");
        assert_eq!(config.mode, Mode::Full);
        assert!(config.require_single_cpu);
        assert_eq!(config.operations, 4_096);
        assert!(parse_args(Vec::<String>::new()).is_err());
    }
}
