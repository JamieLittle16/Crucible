use std::env;
use std::fs;
use std::hint::black_box;
use std::mem::size_of;
use std::path::PathBuf;
use std::time::Instant;

use helve_benchmark_support::{collect_hardware_metadata, push_json_string};
use helve_connection_core::{ConnectionBufferError, ConnectionLimits};
use helve_connection_driver::{ConnectionDriver, DriverError};

#[path = "../config_publication.rs"]
mod config_publication;

use config_publication::{PublicationCursor, PublicationImage, PublicationStep, publish_one};

const SCHEMA: u32 = 1;
const CHECKSUM_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const CHECKSUM_PRIME: u64 = 0x0000_0100_0000_01b3;
const RATIO_SCALE_PPM: u128 = 1_000_000;
const DRAIN_PATTERN: [usize; 7] = [1, 31, 7, 113, 17, 251, 61];

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
}

impl Config {
    const fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                warmup_rounds: 1,
                measured_rounds: 4,
            },
            Mode::Full => Self {
                mode,
                output: None,
                warmup_rounds: 6,
                measured_rounds: 32,
            },
        }
    }
}

#[derive(Clone, Debug)]
struct Workload {
    name: &'static str,
    lengths: Vec<usize>,
    max_body: usize,
    egress: usize,
    fanout: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PublishStats {
    framed_bytes: usize,
    body_bytes: usize,
    frames: usize,
    checksum: u64,
}

impl PublishStats {
    const fn empty() -> Self {
        Self {
            framed_bytes: 0,
            body_bytes: 0,
            frames: 0,
            checksum: CHECKSUM_OFFSET,
        }
    }

    fn combine(&mut self, other: Self) -> Result<(), String> {
        self.framed_bytes = checked_add(self.framed_bytes, other.framed_bytes)?;
        self.body_bytes = checked_add(self.body_bytes, other.body_bytes)?;
        self.frames = checked_add(self.frames, other.frames)?;
        self.checksum = mix_u64(self.checksum, other.checksum);
        Ok(())
    }
}

#[derive(Debug)]
struct WorkloadResult {
    name: &'static str,
    fanout: usize,
    frame_count: usize,
    image_body_bytes: usize,
    cursor_bytes: usize,
    shared_prepare_ns: u128,
    reference_ns: Vec<u128>,
    shared_ns: Vec<u128>,
    semantic: PublishStats,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("configuration publication benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let hardware = collect_hardware_metadata()?;
    let workloads = workloads(config.mode);
    let mut results = Vec::with_capacity(workloads.len());

    for workload in workloads {
        let prep_start = Instant::now();
        let shared_image = build_image(&workload)?;
        let shared_prepare_ns = prep_start.elapsed().as_nanos();

        let reference_gate = run_reference(&workload, 1)?;
        let shared_gate = run_shared(&workload, &shared_image, 1)?;
        if reference_gate != shared_gate {
            return Err(format!(
                "semantic gate mismatch for {}: reference={reference_gate:?} shared={shared_gate:?}",
                workload.name
            ));
        }

        for warmup in 0..config.warmup_rounds {
            if warmup % 2 == 0 {
                black_box(run_reference(&workload, workload.fanout)?);
                black_box(run_shared(&workload, &shared_image, workload.fanout)?);
            } else {
                black_box(run_shared(&workload, &shared_image, workload.fanout)?);
                black_box(run_reference(&workload, workload.fanout)?);
            }
        }

        let mut reference_ns = Vec::with_capacity(config.measured_rounds);
        let mut shared_ns = Vec::with_capacity(config.measured_rounds);
        let mut semantic = None;

        for round in 0..config.measured_rounds {
            if round % 2 == 0 {
                let (reference_time, reference_stats) =
                    timed(|| run_reference(&workload, workload.fanout))?;
                let (shared_time, shared_stats) =
                    timed(|| run_shared(&workload, &shared_image, workload.fanout))?;
                require_same_stats(workload.name, reference_stats, shared_stats)?;
                semantic = Some(reference_stats);
                reference_ns.push(reference_time);
                shared_ns.push(shared_time);
            } else {
                let (shared_time, shared_stats) =
                    timed(|| run_shared(&workload, &shared_image, workload.fanout))?;
                let (reference_time, reference_stats) =
                    timed(|| run_reference(&workload, workload.fanout))?;
                require_same_stats(workload.name, reference_stats, shared_stats)?;
                semantic = Some(reference_stats);
                reference_ns.push(reference_time);
                shared_ns.push(shared_time);
            }
        }

        results.push(WorkloadResult {
            name: workload.name,
            fanout: workload.fanout,
            frame_count: shared_image.frame_count(),
            image_body_bytes: shared_image.body_bytes(),
            cursor_bytes: size_of::<PublicationCursor>(),
            shared_prepare_ns,
            reference_ns,
            shared_ns,
            semantic: semantic.ok_or_else(|| "measured rounds must be positive".to_owned())?,
        });
    }

    let artifact = render_json(&config, &hardware.to_json(), &results)?;
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
    Ok(())
}

fn workloads(mode: Mode) -> Vec<Workload> {
    match mode {
        Mode::Smoke => vec![
            Workload {
                name: "tiny_control",
                lengths: vec![5; 32],
                max_body: 16,
                egress: 64,
                fanout: 4,
            },
            Workload {
                name: "registry_like",
                lengths: registry_lengths(48, 64, 1_024),
                max_body: 1_024,
                egress: 2_080,
                fanout: 4,
            },
            Workload {
                name: "large_tail",
                lengths: vec![32, 48, 96, 1_024, 24, 64],
                max_body: 1_024,
                egress: 1_040,
                fanout: 4,
            },
        ],
        Mode::Full => vec![
            Workload {
                name: "tiny_control",
                lengths: vec![5; 256],
                max_body: 16,
                egress: 256,
                fanout: 64,
            },
            Workload {
                name: "mixed_control",
                lengths: registry_lengths(128, 8, 512),
                max_body: 512,
                egress: 2_080,
                fanout: 64,
            },
            Workload {
                name: "registry_like",
                lengths: registry_lengths(192, 64, 4_096),
                max_body: 4_096,
                egress: 8_208,
                fanout: 64,
            },
            Workload {
                name: "large_tail",
                lengths: {
                    let mut values = registry_lengths(63, 32, 512);
                    values.insert(31, 4_096);
                    values
                },
                max_body: 4_096,
                egress: 4_112,
                fanout: 64,
            },
        ],
    }
}

fn registry_lengths(count: usize, minimum: usize, maximum: usize) -> Vec<usize> {
    assert!(minimum >= 1);
    assert!(minimum <= maximum);
    let span = maximum - minimum + 1;
    (0..count)
        .map(|index| {
            let mixed = index
                .wrapping_mul(1_103)
                .wrapping_add(index.wrapping_mul(index).wrapping_mul(17));
            minimum + (mixed % span)
        })
        .collect()
}

fn build_image(workload: &Workload) -> Result<PublicationImage, String> {
    let bodies = workload
        .lengths
        .iter()
        .enumerate()
        .map(|(index, length)| synthetic_body(index, *length))
        .collect::<Result<Vec<_>, _>>()?;
    PublicationImage::from_bodies(bodies).map_err(|error| format!("image build failed: {error:?}"))
}

fn synthetic_body(index: usize, len: usize) -> Result<Vec<u8>, String> {
    if len == 0 {
        return Err("synthetic packet body cannot be empty".to_owned());
    }
    let packet_id = u8::try_from(index % 120 + 1).expect("synthetic packet id is below 128");
    let mut body = Vec::with_capacity(len);
    body.push(packet_id);
    for offset in 1..len {
        let low = u8::try_from(offset % 251).expect("modulo fits u8");
        body.push(low ^ packet_id ^ 0x5a);
    }
    Ok(body)
}

fn run_reference(workload: &Workload, fanout: usize) -> Result<PublishStats, String> {
    let mut aggregate = PublishStats::empty();
    for _ in 0..fanout {
        let rebuilt = build_image(workload)?;
        aggregate.combine(publish_connection(workload, &rebuilt)?)?;
    }
    Ok(aggregate)
}

fn run_shared(
    workload: &Workload,
    image: &PublicationImage,
    fanout: usize,
) -> Result<PublishStats, String> {
    let mut aggregate = PublishStats::empty();
    for _ in 0..fanout {
        aggregate.combine(publish_connection(workload, image)?)?;
    }
    Ok(aggregate)
}

fn publish_connection(
    workload: &Workload,
    image: &PublicationImage,
) -> Result<PublishStats, String> {
    let limits = ConnectionLimits::new(workload.max_body, workload.max_body + 5, workload.egress)
        .map_err(|error| format!("invalid workload limits: {error:?}"))?;
    let mut driver = ConnectionDriver::new(limits);
    let mut cursor = PublicationCursor::new();
    let mut stats = PublishStats::empty();
    let mut drain_index = 0usize;

    while !cursor.is_complete(image) || driver.queued_egress() != 0 {
        let mut blocked = false;
        if !cursor.is_complete(image) {
            match publish_one::<()>(image, &mut cursor, &mut driver) {
                Ok(PublicationStep::Queued { index, body_bytes }) => {
                    if cursor.next_index() != index + 1 {
                        return Err(
                            "publication cursor advanced by an unexpected amount".to_owned()
                        );
                    }
                    stats.body_bytes = checked_add(stats.body_bytes, body_bytes)?;
                    stats.frames = checked_add(stats.frames, 1)?;
                }
                Ok(PublicationStep::Complete) => {}
                Err(DriverError::Buffer(ConnectionBufferError::EgressLimitExceeded { .. })) => {
                    blocked = true;
                }
                Err(error) => return Err(format!("publication failed: {error:?}")),
            }
        }

        if blocked || cursor.is_complete(image) {
            let pending = driver.pending_egress();
            if pending.is_empty() {
                if blocked {
                    return Err("publication blocked with no drainable egress".to_owned());
                }
                continue;
            }
            let written = DRAIN_PATTERN[drain_index % DRAIN_PATTERN.len()].min(pending.len());
            drain_index += 1;
            stats.checksum = checksum_bytes(stats.checksum, &pending[..written]);
            stats.framed_bytes = checked_add(stats.framed_bytes, written)?;
            driver
                .consume_written::<()>(written)
                .map_err(|error| format!("drain failed: {error:?}"))?;
        }
    }

    if cursor.next_index() != image.frame_count() {
        return Err("cursor did not finish exactly at image frame count".to_owned());
    }
    Ok(stats)
}

fn timed<T>(operation: impl FnOnce() -> Result<T, String>) -> Result<(u128, T), String> {
    let start = Instant::now();
    let result = black_box(operation()?);
    Ok((start.elapsed().as_nanos(), result))
}

fn require_same_stats(
    name: &str,
    reference: PublishStats,
    shared: PublishStats,
) -> Result<(), String> {
    if reference == shared {
        Ok(())
    } else {
        Err(format!(
            "timed semantic mismatch for {name}: reference={reference:?} shared={shared:?}"
        ))
    }
}

fn checksum_bytes(mut checksum: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        checksum ^= u64::from(*byte);
        checksum = checksum.wrapping_mul(CHECKSUM_PRIME);
    }
    checksum
}

const fn mix_u64(left: u64, right: u64) -> u64 {
    left.rotate_left(13) ^ right.wrapping_mul(CHECKSUM_PRIME)
}

fn checked_add(left: usize, right: usize) -> Result<usize, String> {
    left.checked_add(right)
        .ok_or_else(|| "benchmark accounting overflow".to_owned())
}

fn checked_mul(left: usize, right: usize) -> Result<usize, String> {
    left.checked_mul(right)
        .ok_or_else(|| "benchmark memory accounting overflow".to_owned())
}

fn ratio_ppm(numerator: u128, denominator: u128) -> Result<u128, String> {
    if denominator == 0 {
        return Ok(0);
    }
    numerator
        .checked_mul(RATIO_SCALE_PPM)
        .ok_or_else(|| "benchmark ratio accounting overflow".to_owned())
        .map(|scaled| scaled / denominator)
}

fn parse_args() -> Result<Config, String> {
    let mut mode = Mode::Smoke;
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => mode = Mode::Smoke,
            "--full" => mode = Mode::Full,
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--output requires a path".to_owned())?;
                output = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                println!("usage: config_publication_bench [--smoke|--full] [--output PATH]");
                std::process::exit(0);
            }
            unknown => return Err(format!("unknown argument: {unknown}")),
        }
    }
    let mut config = Config::defaults(mode);
    config.output = output;
    Ok(config)
}

fn percentile(samples: &[u128], numerator: usize, denominator: usize) -> u128 {
    assert!(!samples.is_empty());
    assert!(numerator <= denominator);
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = (sorted.len() - 1) * numerator / denominator;
    sorted[rank]
}

fn render_json(
    config: &Config,
    hardware_json: &str,
    results: &[WorkloadResult],
) -> Result<String, String> {
    let mut output = String::new();
    output.push('{');
    output.push_str("\"schema\":");
    output.push_str(&SCHEMA.to_string());
    output.push_str(",\"mode\":");
    push_json_string(&mut output, config.mode.as_str());
    output.push_str(",\"hardware\":");
    output.push_str(hardware_json);
    output.push_str(",\"warmup_rounds\":");
    output.push_str(&config.warmup_rounds.to_string());
    output.push_str(",\"measured_rounds\":");
    output.push_str(&config.measured_rounds.to_string());
    output.push_str(",\"workloads\":[");

    for (index, result) in results.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push('{');
        output.push_str("\"name\":");
        push_json_string(&mut output, result.name);
        output.push_str(",\"fanout\":");
        output.push_str(&result.fanout.to_string());
        output.push_str(",\"frame_count\":");
        output.push_str(&result.frame_count.to_string());
        output.push_str(",\"image_body_bytes\":");
        output.push_str(&result.image_body_bytes.to_string());
        output.push_str(",\"cursor_bytes\":");
        output.push_str(&result.cursor_bytes.to_string());
        output.push_str(",\"reference_logical_rebuilt_body_bytes\":");
        output.push_str(&checked_mul(result.image_body_bytes, result.fanout)?.to_string());
        output.push_str(",\"shared_logical_body_bytes\":");
        output.push_str(&result.image_body_bytes.to_string());
        output.push_str(",\"shared_prepare_ns\":");
        output.push_str(&result.shared_prepare_ns.to_string());
        output.push_str(",\"semantic\":{");
        output.push_str("\"framed_bytes\":");
        output.push_str(&result.semantic.framed_bytes.to_string());
        output.push_str(",\"body_bytes\":");
        output.push_str(&result.semantic.body_bytes.to_string());
        output.push_str(",\"frames\":");
        output.push_str(&result.semantic.frames.to_string());
        output.push_str(",\"checksum\":");
        output.push_str(&result.semantic.checksum.to_string());
        output.push('}');
        push_samples(&mut output, "reference_ns", &result.reference_ns);
        push_samples(&mut output, "shared_ns", &result.shared_ns);
        push_summary(&mut output, "reference", &result.reference_ns);
        push_summary(&mut output, "shared", &result.shared_ns);
        let reference_p50 = percentile(&result.reference_ns, 50, 100);
        let shared_p50 = percentile(&result.shared_ns, 50, 100);
        output.push_str(",\"shared_over_reference_p50_ppm\":");
        output.push_str(&ratio_ppm(shared_p50, reference_p50)?.to_string());
        output.push('}');
    }

    output.push_str("]}");
    Ok(output)
}

fn push_samples(output: &mut String, name: &str, samples: &[u128]) {
    output.push(',');
    push_json_string(output, name);
    output.push_str(": [");
    for (index, sample) in samples.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(&sample.to_string());
    }
    output.push(']');
}

fn push_summary(output: &mut String, prefix: &str, samples: &[u128]) {
    for (suffix, numerator) in [("p50", 50), ("p95", 95), ("p99", 99)] {
        output.push(',');
        push_json_string(output, &format!("{prefix}_{suffix}_ns"));
        output.push(':');
        output.push_str(&percentile(samples, numerator, 100).to_string());
    }
    output.push(',');
    push_json_string(output, &format!("{prefix}_max_ns"));
    output.push(':');
    output.push_str(
        &samples
            .iter()
            .copied()
            .max()
            .expect("measured samples are non-empty")
            .to_string(),
    );
}
