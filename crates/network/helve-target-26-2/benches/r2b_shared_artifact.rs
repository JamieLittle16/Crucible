use std::env;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use helve_target_26_2::r2b::{
    CommandPermissionProfile, CommandProjectionArtifact, CommandProjectionKey,
    PlayBootstrapImage26_2, ProjectionArtifactError, ProjectionRevision, RecipeProjectionArtifact,
    RecipeProjectionKey, ServerDataProjectionArtifact, ServerDataProjectionKey,
};

const SCHEMA: u32 = 2;
const COMMAND_PACKET_ID: i32 = 16;
const SERVER_DATA_PACKET_ID: i32 = 86;
const UPDATE_RECIPES_PACKET_ID: i32 = 133;
const CHECKSUM_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const CHECKSUM_PRIME: u64 = 0x0000_0100_0000_01b3;
const RATIO_SCALE_PPM: u128 = 1_000_000;

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
    warmup_blocks: usize,
    measured_blocks: usize,
    joins_per_sample: usize,
    blocks_per_epoch: usize,
}

impl Config {
    const fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                warmup_blocks: 256,
                measured_blocks: 4_096,
                joins_per_sample: 1_024,
                blocks_per_epoch: 256,
            },
            Mode::Full => Self {
                mode,
                output: None,
                warmup_blocks: 2_048,
                measured_blocks: 32_768,
                joins_per_sample: 1_024,
                blocks_per_epoch: 512,
            },
        }
    }

    fn samples_per_candidate(&self) -> Result<usize, String> {
        self.measured_blocks
            .checked_mul(2)
            .ok_or_else(|| "sample count overflow".to_owned())
    }

    fn validate(&self) -> Result<(), String> {
        if self.joins_per_sample == 0 {
            return Err("joins per sample must be positive".to_owned());
        }
        if self.measured_blocks == 0 {
            return Err("measured block count must be positive".to_owned());
        }
        if self.blocks_per_epoch == 0 || !self.measured_blocks.is_multiple_of(self.blocks_per_epoch)
        {
            return Err("measured blocks must be divisible by blocks per epoch".to_owned());
        }
        Ok(())
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
    reference_block_ns: Vec<u128>,
    certified_block_ns: Vec<u128>,
    block_ratio_ppm: Vec<u128>,
    semantic_checksum: u64,
}

impl Samples {
    fn with_capacity(config: &Config, semantic_checksum: u64) -> Result<Self, String> {
        let sample_capacity = config.samples_per_candidate()?;
        Ok(Self {
            reference_ns: Vec::with_capacity(sample_capacity),
            certified_ns: Vec::with_capacity(sample_capacity),
            reference_block_ns: Vec::with_capacity(config.measured_blocks),
            certified_block_ns: Vec::with_capacity(config.measured_blocks),
            block_ratio_ppm: Vec::with_capacity(config.measured_blocks),
            semantic_checksum,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DistributionStats {
    count: usize,
    min: u128,
    p50: u128,
    p90: u128,
    p95: u128,
    p99: u128,
    p999: u128,
    max: u128,
    mean: u128,
    mad: u128,
    iqr: u128,
    top_1pct_mean: u128,
    top_0_1pct_mean: u128,
    relative_mad_ppm: u128,
    p99_to_p50_ppm: u128,
    p999_to_p50_ppm: u128,
    max_to_p50_ppm: u128,
}

impl DistributionStats {
    fn from_samples(values: &[u128]) -> Result<Self, String> {
        if values.is_empty() {
            return Err("cannot summarize an empty sample set".to_owned());
        }

        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        let p25 = quantile_permille_sorted(&sorted, 250)?;
        let p50 = quantile_permille_sorted(&sorted, 500)?;
        let p75 = quantile_permille_sorted(&sorted, 750)?;
        let p90 = quantile_permille_sorted(&sorted, 900)?;
        let p95 = quantile_permille_sorted(&sorted, 950)?;
        let p99 = quantile_permille_sorted(&sorted, 990)?;
        let p999 = quantile_permille_sorted(&sorted, 999)?;
        let sum = checked_sum(&sorted)?;
        let count = u128::try_from(sorted.len())
            .map_err(|_| "sample count does not fit u128".to_owned())?;
        let mean = sum
            .checked_div(count)
            .ok_or_else(|| "sample count must be positive".to_owned())?;

        let mut deviations = Vec::with_capacity(sorted.len());
        deviations.extend(sorted.iter().map(|value| value.abs_diff(p50)));
        deviations.sort_unstable();
        let mad = quantile_permille_sorted(&deviations, 500)?;

        Ok(Self {
            count: sorted.len(),
            min: sorted[0],
            p50,
            p90,
            p95,
            p99,
            p999,
            max: sorted[sorted.len() - 1],
            mean,
            mad,
            iqr: p75.saturating_sub(p25),
            top_1pct_mean: upper_tail_mean(&sorted, 10)?,
            top_0_1pct_mean: upper_tail_mean(&sorted, 1)?,
            relative_mad_ppm: ratio_ppm(mad, p50)?,
            p99_to_p50_ppm: ratio_ppm(p99, p50)?,
            p999_to_p50_ppm: ratio_ppm(p999, p50)?,
            max_to_p50_ppm: ratio_ppm(sorted[sorted.len() - 1], p50)?,
        })
    }
}

#[derive(Debug)]
struct Evidence {
    reference: DistributionStats,
    certified: DistributionStats,
    block_ratio: DistributionStats,
    epoch_ratio: DistributionStats,
    epoch_ratio_ppm: Vec<u128>,
    certified_faster_blocks: usize,
    certified_faster_block_rate_ppm: u128,
    certified_faster_epochs: usize,
    certified_faster_epoch_rate_ppm: u128,
}

impl Evidence {
    fn from_samples(config: &Config, samples: &Samples) -> Result<Self, String> {
        let expected_samples = config.samples_per_candidate()?;
        if samples.reference_ns.len() != expected_samples
            || samples.certified_ns.len() != expected_samples
            || samples.reference_block_ns.len() != config.measured_blocks
            || samples.certified_block_ns.len() != config.measured_blocks
            || samples.block_ratio_ppm.len() != config.measured_blocks
        {
            return Err("measured sample cardinality drifted".to_owned());
        }

        let epoch_ratio_ppm = epoch_ratios(
            &samples.reference_block_ns,
            &samples.certified_block_ns,
            config.blocks_per_epoch,
        )?;
        let certified_faster_blocks = samples
            .block_ratio_ppm
            .iter()
            .filter(|ratio| **ratio < RATIO_SCALE_PPM)
            .count();
        let certified_faster_epochs = epoch_ratio_ppm
            .iter()
            .filter(|ratio| **ratio < RATIO_SCALE_PPM)
            .count();

        Ok(Self {
            reference: DistributionStats::from_samples(&samples.reference_ns)?,
            certified: DistributionStats::from_samples(&samples.certified_ns)?,
            block_ratio: DistributionStats::from_samples(&samples.block_ratio_ppm)?,
            epoch_ratio: DistributionStats::from_samples(&epoch_ratio_ppm)?,
            certified_faster_blocks,
            certified_faster_block_rate_ppm: rate_ppm(
                certified_faster_blocks,
                samples.block_ratio_ppm.len(),
            )?,
            certified_faster_epochs,
            certified_faster_epoch_rate_ppm: rate_ppm(
                certified_faster_epochs,
                epoch_ratio_ppm.len(),
            )?,
            epoch_ratio_ppm,
        })
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("R2B shared-artifact benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    config.validate()?;
    let fixture = Fixture::new()?;

    let reference_gate = run_reference(&fixture, 1)?;
    let certified_gate = run_certified(&fixture, 1)?;
    if reference_gate != certified_gate {
        return Err(format!(
            "semantic gate mismatch: reference={reference_gate} certified={certified_gate}"
        ));
    }

    for block in 0..config.warmup_blocks {
        run_balanced_block(block, &fixture, config.joins_per_sample, None)?;
    }

    let mut samples = Samples::with_capacity(&config, reference_gate)?;
    for block in 0..config.measured_blocks {
        run_balanced_block(block, &fixture, config.joins_per_sample, Some(&mut samples))?;
    }

    let evidence = Evidence::from_samples(&config, &samples)?;
    let artifact = render_json(&config, &samples, &evidence);

    if let Some(path) = config.output.as_ref() {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create output directory: {error}"))?;
        }
        fs::write(path, artifact.as_bytes())
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    } else {
        println!("{artifact}");
    }

    println!(
        "R2B shared artifact: paired_p50={}ppm paired_mad={}ppm epoch_p50={}ppm epoch_win_rate={}ppm reference_p99={}ns certified_p99={}ns reference_p999={}ns certified_p999={}ns",
        evidence.block_ratio.p50,
        evidence.block_ratio.mad,
        evidence.epoch_ratio.p50,
        evidence.certified_faster_epoch_rate_ppm,
        evidence.reference.p99,
        evidence.certified.p99,
        evidence.reference.p999,
        evidence.certified.p999,
    );
    Ok(())
}

fn run_balanced_block(
    block: usize,
    fixture: &Fixture,
    joins: usize,
    mut samples: Option<&mut Samples>,
) -> Result<(), String> {
    let order = if block.is_multiple_of(2) {
        [
            Candidate::Reference,
            Candidate::Certified,
            Candidate::Certified,
            Candidate::Reference,
        ]
    } else {
        [
            Candidate::Certified,
            Candidate::Reference,
            Candidate::Reference,
            Candidate::Certified,
        ]
    };

    let mut expected_checksum = None;
    let mut reference_total = 0_u128;
    let mut certified_total = 0_u128;

    for candidate in order {
        let (elapsed, checksum) = timed(|| match candidate {
            Candidate::Reference => run_reference(fixture, joins),
            Candidate::Certified => run_certified(fixture, joins),
        })?;

        if let Some(expected) = expected_checksum {
            if checksum != expected {
                return Err(format!(
                    "balanced semantic mismatch: expected={expected} actual={checksum}"
                ));
            }
        } else {
            expected_checksum = Some(checksum);
        }

        match candidate {
            Candidate::Reference => {
                reference_total = reference_total
                    .checked_add(elapsed)
                    .ok_or_else(|| "reference block timing overflow".to_owned())?;
                if let Some(output) = samples.as_deref_mut() {
                    output.reference_ns.push(elapsed);
                }
            }
            Candidate::Certified => {
                certified_total = certified_total
                    .checked_add(elapsed)
                    .ok_or_else(|| "certified block timing overflow".to_owned())?;
                if let Some(output) = samples.as_deref_mut() {
                    output.certified_ns.push(elapsed);
                }
            }
        }
    }

    if let Some(output) = samples {
        let checksum = expected_checksum
            .ok_or_else(|| "balanced block did not produce a checksum".to_owned())?;
        output.semantic_checksum = checksum;
        output.reference_block_ns.push(reference_total);
        output.certified_block_ns.push(certified_total);
        output
            .block_ratio_ppm
            .push(ratio_ppm(certified_total, reference_total)?);
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

fn checked_sum(values: &[u128]) -> Result<u128, String> {
    values.iter().try_fold(0_u128, |sum, value| {
        sum.checked_add(*value)
            .ok_or_else(|| "sample sum overflow".to_owned())
    })
}

fn quantile_permille_sorted(sorted: &[u128], permille: usize) -> Result<u128, String> {
    if sorted.is_empty() {
        return Err("cannot compute a quantile of an empty sample set".to_owned());
    }
    if permille == 0 || permille > 1_000 {
        return Err("quantile permille must be in 1..=1000".to_owned());
    }
    let numerator = sorted
        .len()
        .checked_mul(permille)
        .ok_or_else(|| "quantile rank overflow".to_owned())?;
    let rank = numerator
        .checked_add(999)
        .ok_or_else(|| "quantile rank overflow".to_owned())?
        / 1_000;
    Ok(sorted[rank.saturating_sub(1).min(sorted.len() - 1)])
}

fn upper_tail_mean(sorted: &[u128], tail_permille: usize) -> Result<u128, String> {
    if sorted.is_empty() {
        return Err("cannot summarize an empty tail".to_owned());
    }
    if tail_permille == 0 || tail_permille > 1_000 {
        return Err("tail permille must be in 1..=1000".to_owned());
    }
    let numerator = sorted
        .len()
        .checked_mul(tail_permille)
        .ok_or_else(|| "tail rank overflow".to_owned())?;
    let count = numerator
        .checked_add(999)
        .ok_or_else(|| "tail rank overflow".to_owned())?
        / 1_000;
    let tail = &sorted[sorted.len() - count.max(1)..];
    let divisor =
        u128::try_from(tail.len()).map_err(|_| "tail sample count does not fit u128".to_owned())?;
    checked_sum(tail)?
        .checked_div(divisor)
        .ok_or_else(|| "tail sample count must be positive".to_owned())
}

fn ratio_ppm(numerator: u128, denominator: u128) -> Result<u128, String> {
    numerator
        .checked_mul(RATIO_SCALE_PPM)
        .ok_or_else(|| "ratio numerator overflow".to_owned())?
        .checked_div(denominator)
        .ok_or_else(|| "ratio denominator must be positive".to_owned())
}

fn rate_ppm(successes: usize, total: usize) -> Result<u128, String> {
    let successes =
        u128::try_from(successes).map_err(|_| "success count does not fit u128".to_owned())?;
    let total = u128::try_from(total).map_err(|_| "total count does not fit u128".to_owned())?;
    ratio_ppm(successes, total)
}

fn epoch_ratios(
    reference_blocks: &[u128],
    certified_blocks: &[u128],
    blocks_per_epoch: usize,
) -> Result<Vec<u128>, String> {
    if reference_blocks.len() != certified_blocks.len() {
        return Err("paired block cardinality mismatch".to_owned());
    }
    if blocks_per_epoch == 0 || !reference_blocks.len().is_multiple_of(blocks_per_epoch) {
        return Err("block count must be divisible by blocks per epoch".to_owned());
    }

    let mut ratios = Vec::with_capacity(reference_blocks.len() / blocks_per_epoch);
    for (reference, certified) in reference_blocks
        .chunks_exact(blocks_per_epoch)
        .zip(certified_blocks.chunks_exact(blocks_per_epoch))
    {
        ratios.push(ratio_ppm(checked_sum(certified)?, checked_sum(reference)?)?);
    }
    Ok(ratios)
}

fn render_json(config: &Config, samples: &Samples, evidence: &Evidence) -> String {
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
    out.push_str(",\"sampling\":{");
    out.push_str("\"pattern\":\"balanced-abba-baab\"");
    push_usize_field(&mut out, "warmup_blocks", config.warmup_blocks);
    push_usize_field(&mut out, "measured_blocks", config.measured_blocks);
    push_usize_field(&mut out, "joins_per_sample", config.joins_per_sample);
    push_usize_field(&mut out, "samples_per_candidate", evidence.reference.count);
    push_usize_field(&mut out, "blocks_per_epoch", config.blocks_per_epoch);
    push_usize_field(&mut out, "epoch_count", evidence.epoch_ratio.count);
    out.push('}');
    out.push_str(",\"host\":{\"os\":\"");
    out.push_str(env::consts::OS);
    out.push_str("\",\"arch\":\"");
    out.push_str(env::consts::ARCH);
    out.push_str("\",\"available_parallelism\":");
    out.push_str(&parallelism.to_string());
    if let Ok(cpu) = env::var("HELVE_BENCH_CPU") {
        out.push_str(",\"pinned_cpu\":\"");
        out.push_str(&cpu);
        out.push('"');
    }
    out.push('}');
    out.push_str(",\"structural\":{\"reference_packet_id_decodes_per_join\":3");
    out.push_str(",\"certified_packet_id_decodes_per_join\":0");
    out.push_str(",\"additional_allocations_per_join\":0}");
    out.push_str(",\"semantic_checksum\":");
    out.push_str(&samples.semantic_checksum.to_string());

    push_samples(&mut out, "reference_sample_ns", &samples.reference_ns);
    push_samples(&mut out, "certified_sample_ns", &samples.certified_ns);
    push_samples(&mut out, "paired_block_ratio_ppm", &samples.block_ratio_ppm);
    push_samples(&mut out, "epoch_ratio_ppm", &evidence.epoch_ratio_ppm);
    push_distribution(&mut out, "reference_service_ns", &evidence.reference);
    push_distribution(&mut out, "certified_service_ns", &evidence.certified);
    push_distribution(&mut out, "paired_block_ratio", &evidence.block_ratio);
    push_distribution(&mut out, "epoch_ratio", &evidence.epoch_ratio);
    push_direction(&mut out, evidence);
    push_tail(&mut out, evidence);
    out.push('}');
    out
}

fn push_direction(out: &mut String, evidence: &Evidence) {
    out.push_str(",\"direction\":{");
    out.push_str("\"certified_faster_paired_p50\":");
    out.push_str(if evidence.block_ratio.p50 < RATIO_SCALE_PPM {
        "true"
    } else {
        "false"
    });
    push_usize_field(
        out,
        "certified_faster_blocks",
        evidence.certified_faster_blocks,
    );
    push_u128_field(
        out,
        "certified_faster_block_rate_ppm",
        evidence.certified_faster_block_rate_ppm,
    );
    push_usize_field(
        out,
        "certified_faster_epochs",
        evidence.certified_faster_epochs,
    );
    push_u128_field(
        out,
        "certified_faster_epoch_rate_ppm",
        evidence.certified_faster_epoch_rate_ppm,
    );
    out.push('}');
}

fn push_tail(out: &mut String, evidence: &Evidence) {
    out.push_str(",\"tail\":{");
    out.push_str("\"p99_not_worse\":");
    out.push_str(if evidence.certified.p99 <= evidence.reference.p99 {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"p999_not_worse\":");
    out.push_str(if evidence.certified.p999 <= evidence.reference.p999 {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"top_1pct_mean_not_worse\":");
    out.push_str(
        if evidence.certified.top_1pct_mean <= evidence.reference.top_1pct_mean {
            "true"
        } else {
            "false"
        },
    );
    out.push_str(",\"relative_mad_not_worse\":");
    out.push_str(
        if evidence.certified.relative_mad_ppm <= evidence.reference.relative_mad_ppm {
            "true"
        } else {
            "false"
        },
    );
    out.push('}');
}

fn push_distribution(out: &mut String, name: &str, stats: &DistributionStats) {
    out.push_str(",\"");
    out.push_str(name);
    out.push_str("\":{");
    out.push_str("\"count\":");
    out.push_str(&stats.count.to_string());
    push_u128_field(out, "min", stats.min);
    push_u128_field(out, "p50", stats.p50);
    push_u128_field(out, "p90", stats.p90);
    push_u128_field(out, "p95", stats.p95);
    push_u128_field(out, "p99", stats.p99);
    push_u128_field(out, "p999", stats.p999);
    push_u128_field(out, "max", stats.max);
    push_u128_field(out, "mean", stats.mean);
    push_u128_field(out, "mad", stats.mad);
    push_u128_field(out, "iqr", stats.iqr);
    push_u128_field(out, "top_1pct_mean", stats.top_1pct_mean);
    push_u128_field(out, "top_0_1pct_mean", stats.top_0_1pct_mean);
    push_u128_field(out, "relative_mad_ppm", stats.relative_mad_ppm);
    push_u128_field(out, "p99_to_p50_ppm", stats.p99_to_p50_ppm);
    push_u128_field(out, "p999_to_p50_ppm", stats.p999_to_p50_ppm);
    push_u128_field(out, "max_to_p50_ppm", stats.max_to_p50_ppm);
    out.push('}');
}

fn push_u128_field(out: &mut String, name: &str, value: u128) {
    out.push_str(",\"");
    out.push_str(name);
    out.push_str("\":");
    out.push_str(&value.to_string());
}

fn push_usize_field(out: &mut String, name: &str, value: usize) {
    out.push_str(",\"");
    out.push_str(name);
    out.push_str("\":");
    out.push_str(&value.to_string());
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
