use std::env;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use crucible_target_26_2::r2b::{
    CommandPermissionProfile, CommandProjectionArtifact, CommandProjectionKey,
    PlayBootstrapImage26_2, ProjectionArtifactError, ProjectionRevision, RecipeProjectionArtifact,
    RecipeProjectionKey, ServerDataProjectionArtifact, ServerDataProjectionKey,
};

const SCHEMA: u32 = 1;
const COMMAND_PACKET_ID: i32 = 16;
const SERVER_DATA_PACKET_ID: i32 = 86;
const UPDATE_RECIPES_PACKET_ID: i32 = 133;
const CHECKSUM_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const CHECKSUM_PRIME: u64 = 0x0000_0100_0000_01b3;

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
    warmup_rounds: usize,
    measured_rounds: usize,
    joins_per_round: usize,
}

impl Config {
    const fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                warmup_rounds: 2,
                measured_rounds: 7,
                joins_per_round: 500_000,
            },
            Mode::Full => Self {
                mode,
                output: None,
                warmup_rounds: 8,
                measured_rounds: 31,
                joins_per_round: 5_000_000,
            },
        }
    }
}

#[derive(Debug)]
struct Fixture {
    image: PlayBootstrapImage26_2,
    command_key: CommandProjectionKey,
    recipe_key: RecipeProjectionKey,
    status: ServerDataProjectionArtifact,
    status_key: ServerDataProjectionKey,
}

impl Fixture {
    fn new() -> Result<Self, String> {
        let command_key = command_key();
        let recipe_key = recipe_key();
        let status_key = status_key();
        let image = PlayBootstrapImage26_2::new(
            CommandProjectionArtifact::new(command_key, vec![16, 0xaa, 0xbb].into_boxed_slice())
                .map_err(artifact_error)?,
            RecipeProjectionArtifact::new(
                recipe_key,
                vec![0x85, 0x01, 0xcc, 0xdd].into_boxed_slice(),
            )
            .map_err(artifact_error)?,
        );
        let status =
            ServerDataProjectionArtifact::new(status_key, vec![86, 0xee].into_boxed_slice())
                .map_err(artifact_error)?;
        Ok(Self {
            image,
            command_key,
            recipe_key,
            status,
            status_key,
        })
    }
}

#[derive(Debug)]
struct Samples {
    reference_ns: Vec<u128>,
    certified_ns: Vec<u128>,
    semantic_checksum: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("R2B shared-artifact benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let fixture = Fixture::new()?;

    let reference_gate = run_reference(&fixture, 1)?;
    let certified_gate = run_certified(&fixture, 1)?;
    if reference_gate != certified_gate {
        return Err(format!(
            "semantic gate mismatch: reference={reference_gate} certified={certified_gate}"
        ));
    }

    for round in 0..config.warmup_rounds {
        run_pair(round, &fixture, config.joins_per_round, false, None)?;
    }

    let mut samples = Samples {
        reference_ns: Vec::with_capacity(config.measured_rounds),
        certified_ns: Vec::with_capacity(config.measured_rounds),
        semantic_checksum: reference_gate,
    };
    for round in 0..config.measured_rounds {
        run_pair(
            round,
            &fixture,
            config.joins_per_round,
            true,
            Some(&mut samples),
        )?;
    }

    let reference_p50 = median(&samples.reference_ns)?;
    let certified_p50 = median(&samples.certified_ns)?;
    let ratio_ppm = certified_p50
        .saturating_mul(1_000_000)
        .checked_div(reference_p50)
        .ok_or_else(|| "reference median must be positive".to_owned())?;
    let artifact = render_json(&config, &samples, reference_p50, certified_p50, ratio_ppm);

    if let Some(path) = config.output {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create output directory: {error}"))?;
        }
        fs::write(&path, artifact.as_bytes())
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    } else {
        println!("{artifact}");
    }

    println!(
        "R2B shared artifact p50: reference={reference_p50}ns certified={certified_p50}ns ratio_ppm={ratio_ppm} faster={}",
        certified_p50 < reference_p50
    );
    Ok(())
}

fn run_pair(
    round: usize,
    fixture: &Fixture,
    joins: usize,
    measured: bool,
    mut samples: Option<&mut Samples>,
) -> Result<(), String> {
    let reference_first = round.is_multiple_of(2);
    let first = if reference_first {
        Candidate::Reference
    } else {
        Candidate::Certified
    };
    let second = if reference_first {
        Candidate::Certified
    } else {
        Candidate::Reference
    };

    let mut expected = None;
    for candidate in [first, second] {
        let (elapsed, checksum) = timed(|| match candidate {
            Candidate::Reference => run_reference(fixture, joins),
            Candidate::Certified => run_certified(fixture, joins),
        })?;
        if let Some(expected_checksum) = expected {
            if checksum != expected_checksum {
                return Err(format!(
                    "paired semantic mismatch: expected={expected_checksum} actual={checksum}"
                ));
            }
        } else {
            expected = Some(checksum);
        }

        if measured {
            let output = samples
                .as_deref_mut()
                .ok_or_else(|| "measured round requires sample storage".to_owned())?;
            match candidate {
                Candidate::Reference => output.reference_ns.push(elapsed),
                Candidate::Certified => output.certified_ns.push(elapsed),
            }
            output.semantic_checksum = checksum;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Candidate {
    Reference,
    Certified,
}

fn run_reference(fixture: &Fixture, joins: usize) -> Result<u64, String> {
    let fixture = black_box(fixture);
    let mut checksum = CHECKSUM_OFFSET;
    for _ in 0..joins {
        let commands = fixture
            .image
            .commands(black_box(&fixture.command_key))
            .map_err(artifact_error)?;
        validate_packet_id(black_box(commands), COMMAND_PACKET_ID)?;
        checksum = observe(checksum, black_box(commands));

        let recipes = fixture
            .image
            .update_recipes(black_box(&fixture.recipe_key))
            .map_err(artifact_error)?;
        validate_packet_id(black_box(recipes), UPDATE_RECIPES_PACKET_ID)?;
        checksum = observe(checksum, black_box(recipes));

        let status = fixture
            .status
            .body_for(black_box(&fixture.status_key))
            .map_err(artifact_error)?;
        validate_packet_id(black_box(status), SERVER_DATA_PACKET_ID)?;
        checksum = observe(checksum, black_box(status));
    }
    Ok(black_box(checksum))
}

fn run_certified(fixture: &Fixture, joins: usize) -> Result<u64, String> {
    let fixture = black_box(fixture);
    let mut checksum = CHECKSUM_OFFSET;
    for _ in 0..joins {
        let commands = fixture
            .image
            .commands(black_box(&fixture.command_key))
            .map_err(artifact_error)?;
        checksum = observe(checksum, black_box(commands));

        let recipes = fixture
            .image
            .update_recipes(black_box(&fixture.recipe_key))
            .map_err(artifact_error)?;
        checksum = observe(checksum, black_box(recipes));

        let status = fixture
            .status
            .body_for(black_box(&fixture.status_key))
            .map_err(artifact_error)?;
        checksum = observe(checksum, black_box(status));
    }
    Ok(black_box(checksum))
}

fn observe(mut checksum: u64, body: &[u8]) -> u64 {
    checksum ^= u64::try_from(body.len()).unwrap_or(u64::MAX);
    checksum = checksum.wrapping_mul(CHECKSUM_PRIME);
    for byte in body {
        checksum ^= u64::from(*byte);
        checksum = checksum.wrapping_mul(CHECKSUM_PRIME);
    }
    checksum
}

fn validate_packet_id(body: &[u8], expected: i32) -> Result<(), String> {
    let actual = decode_nonnegative_var_int(body)
        .ok_or_else(|| "reference body has invalid packet identity".to_owned())?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "reference packet identity mismatch: expected={expected} actual={actual}"
        ))
    }
}

fn decode_nonnegative_var_int(body: &[u8]) -> Option<i32> {
    let mut value = 0_u32;
    for (index, byte) in body.iter().copied().take(5).enumerate() {
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            if value > i32::MAX.cast_unsigned() {
                return None;
            }
            let value = i32::try_from(value).ok()?;
            if var_int_len(value) != index + 1 {
                return None;
            }
            return Some(value);
        }
    }
    None
}

const fn var_int_len(value: i32) -> usize {
    let mut remaining = value.cast_unsigned();
    let mut length = 1_usize;
    while remaining & !0x7f != 0 {
        remaining >>= 7;
        length += 1;
    }
    length
}

fn timed<T>(operation: impl FnOnce() -> Result<T, String>) -> Result<(u128, T), String> {
    let start = Instant::now();
    let value = black_box(operation()?);
    Ok((start.elapsed().as_nanos(), value))
}

fn median(values: &[u128]) -> Result<u128, String> {
    if values.is_empty() {
        return Err("cannot compute median of empty sample set".to_owned());
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Ok(sorted[sorted.len() / 2])
}

fn render_json(
    config: &Config,
    samples: &Samples,
    reference_p50: u128,
    certified_p50: u128,
    ratio_ppm: u128,
) -> String {
    let parallelism = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let mut out = String::from("{");
    out.push_str("\"schema\":");
    out.push_str(&SCHEMA.to_string());
    out.push_str(",\"benchmark\":\"r2b-shared-artifact-validation\"");
    out.push_str(",\"mode\":\"");
    out.push_str(config.mode.as_str());
    out.push('"');
    out.push_str(",\"hosted_ci_is_diagnostic_only\":true");
    out.push_str(",\"production_path_is_construction_certified\":true");
    out.push_str(",\"joins_per_round\":");
    out.push_str(&config.joins_per_round.to_string());
    out.push_str(",\"measured_rounds\":");
    out.push_str(&config.measured_rounds.to_string());
    out.push_str(",\"host\":{\"os\":\"");
    out.push_str(env::consts::OS);
    out.push_str("\",\"arch\":\"");
    out.push_str(env::consts::ARCH);
    out.push_str("\",\"available_parallelism\":");
    out.push_str(&parallelism.to_string());
    out.push('}');
    out.push_str(",\"structural\":{\"reference_packet_id_decodes_per_join\":3");
    out.push_str(",\"certified_packet_id_decodes_per_join\":0");
    out.push_str(",\"additional_allocations_per_join\":0}");
    out.push_str(",\"semantic_checksum\":");
    out.push_str(&samples.semantic_checksum.to_string());
    push_samples(&mut out, "reference_ns", &samples.reference_ns);
    push_samples(&mut out, "certified_ns", &samples.certified_ns);
    out.push_str(",\"reference_p50_ns\":");
    out.push_str(&reference_p50.to_string());
    out.push_str(",\"certified_p50_ns\":");
    out.push_str(&certified_p50.to_string());
    out.push_str(",\"certified_to_reference_ppm\":");
    out.push_str(&ratio_ppm.to_string());
    out.push_str(",\"certified_faster_p50\":");
    out.push_str(if certified_p50 < reference_p50 {
        "true"
    } else {
        "false"
    });
    out.push('}');
    out
}

fn push_samples(out: &mut String, name: &str, values: &[u128]) {
    out.push_str(",\"");
    out.push_str(name);
    out.push_str("\":[");
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str(&value.to_string());
    }
    out.push(']');
}

fn parse_args() -> Result<Config, String> {
    let mut mode = Mode::Smoke;
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bench" => {}
            "--smoke" => mode = Mode::Smoke,
            "--full" => mode = Mode::Full,
            "--output" => {
                output = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--output requires a path".to_owned())?,
                ));
            }
            "--help" | "-h" => {
                println!("usage: r2b_shared_artifact [--smoke|--full] [--output PATH]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let mut config = Config::defaults(mode);
    config.output = output;
    Ok(config)
}

const fn revision(byte: u8) -> ProjectionRevision {
    ProjectionRevision::new([byte; 32])
}

const fn command_key() -> CommandProjectionKey {
    CommandProjectionKey::new(
        revision(1),
        revision(2),
        revision(3),
        revision(4),
        CommandPermissionProfile::DefaultNonOperator,
    )
}

const fn recipe_key() -> RecipeProjectionKey {
    RecipeProjectionKey::new(revision(5), revision(6), revision(7), revision(8))
}

const fn status_key() -> ServerDataProjectionKey {
    ServerDataProjectionKey::new(revision(9), revision(10))
}

fn artifact_error(error: ProjectionArtifactError) -> String {
    format!("projection artifact error: {error:?}")
}
