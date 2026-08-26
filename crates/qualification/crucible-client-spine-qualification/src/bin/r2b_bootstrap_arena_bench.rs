use std::env;
use std::fs;
use std::hint::black_box;
use std::mem::size_of;
use std::path::PathBuf;
use std::time::Instant;

use crucible_benchmark_support::{collect_hardware_metadata, push_json_string};
use crucible_packet_core::PacketWriter;
use crucible_protocol_core::encode_var_int;

#[path = "../../../../network/crucible-target-26-2/src/r2b_arena.rs"]
mod r2b_arena;

use r2b_arena::DynamicBootstrapArena;

const SCHEMA: u32 = 1;
const DYNAMIC_BODIES: usize = 18;
const MAX_BODY: usize = 4_096;
const PAYLOAD: [u8; MAX_BODY] = [0xa5; MAX_BODY];
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
                warmup_rounds: 8,
                measured_rounds: 48,
            },
        }
    }
}

#[derive(Clone, Debug)]
struct Workload {
    name: &'static str,
    lengths: [usize; DYNAMIC_BODIES],
    fanout: usize,
}

impl Workload {
    fn body_bytes(&self) -> Result<usize, String> {
        self.lengths.iter().try_fold(0usize, |sum, length| {
            sum.checked_add(*length)
                .ok_or_else(|| "workload byte accounting overflow".to_owned())
        })
    }

    fn max_body(&self) -> usize {
        self.lengths.iter().copied().max().unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SemanticStats {
    bodies: usize,
    bytes: usize,
    checksum: u64,
}

impl SemanticStats {
    const fn empty() -> Self {
        Self {
            bodies: 0,
            bytes: 0,
            checksum: CHECKSUM_OFFSET,
        }
    }

    fn combine(&mut self, other: Self) -> Result<(), String> {
        self.bodies = self
            .bodies
            .checked_add(other.bodies)
            .ok_or_else(|| "body accounting overflow".to_owned())?;
        self.bytes = self
            .bytes
            .checked_add(other.bytes)
            .ok_or_else(|| "byte accounting overflow".to_owned())?;
        self.checksum = self.checksum.rotate_left(13) ^ other.checksum.wrapping_mul(CHECKSUM_PRIME);
        Ok(())
    }
}

#[derive(Debug)]
struct WorkloadResult {
    workload: Workload,
    semantic: SemanticStats,
    owned_ns: Vec<u128>,
    arena_ns: Vec<u128>,
    direct_floor_ns: Vec<u128>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("R2B bootstrap arena benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let hardware = collect_hardware_metadata()?;
    let mut results = Vec::new();

    for workload in workloads(config.mode) {
        validate_workload(&workload)?;

        let owned_gate = run_owned(&workload, 1)?;
        let arena_gate = run_arena(&workload, 1)?;
        let direct_gate = run_direct_floor(&workload, 1)?;
        require_equivalent(&workload, owned_gate, arena_gate, direct_gate)?;

        for round in 0..config.warmup_rounds {
            run_round_order(round, &workload, workload.fanout, false, None)?;
        }

        let mut owned_ns = Vec::with_capacity(config.measured_rounds);
        let mut arena_ns = Vec::with_capacity(config.measured_rounds);
        let mut direct_floor_ns = Vec::with_capacity(config.measured_rounds);
        let mut semantic = None;

        for round in 0..config.measured_rounds {
            let mut samples = RoundSamples {
                owned_ns: &mut owned_ns,
                arena_ns: &mut arena_ns,
                direct_floor_ns: &mut direct_floor_ns,
                semantic: &mut semantic,
            };
            run_round_order(round, &workload, workload.fanout, true, Some(&mut samples))?;
        }

        results.push(WorkloadResult {
            workload,
            semantic: semantic.ok_or_else(|| "measured rounds must be positive".to_owned())?,
            owned_ns,
            arena_ns,
            direct_floor_ns,
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

struct RoundSamples<'a> {
    owned_ns: &'a mut Vec<u128>,
    arena_ns: &'a mut Vec<u128>,
    direct_floor_ns: &'a mut Vec<u128>,
    semantic: &'a mut Option<SemanticStats>,
}

fn run_round_order(
    round: usize,
    workload: &Workload,
    fanout: usize,
    measured: bool,
    mut samples: Option<&mut RoundSamples<'_>>,
) -> Result<(), String> {
    let order = match round % 3 {
        0 => [Candidate::Owned, Candidate::Arena, Candidate::DirectFloor],
        1 => [Candidate::Arena, Candidate::DirectFloor, Candidate::Owned],
        _ => [Candidate::DirectFloor, Candidate::Owned, Candidate::Arena],
    };

    let mut round_semantic = None;
    for candidate in order {
        let (elapsed, semantic) = timed(|| match candidate {
            Candidate::Owned => run_owned(workload, fanout),
            Candidate::Arena => run_arena(workload, fanout),
            Candidate::DirectFloor => run_direct_floor(workload, fanout),
        })?;
        if let Some(expected) = round_semantic {
            if semantic != expected {
                return Err(format!(
                    "timed semantic mismatch for {}: expected={expected:?} actual={semantic:?}",
                    workload.name
                ));
            }
        } else {
            round_semantic = Some(semantic);
        }

        if measured {
            let output = samples
                .as_deref_mut()
                .ok_or_else(|| "measured round requires sample storage".to_owned())?;
            match candidate {
                Candidate::Owned => output.owned_ns.push(elapsed),
                Candidate::Arena => output.arena_ns.push(elapsed),
                Candidate::DirectFloor => output.direct_floor_ns.push(elapsed),
            }
            *output.semantic = Some(semantic);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Candidate {
    Owned,
    Arena,
    DirectFloor,
}

fn workloads(mode: Mode) -> Vec<Workload> {
    let fanout = match mode {
        Mode::Smoke => 16,
        Mode::Full => 1_024,
    };
    vec![
        Workload {
            name: "tiny-control",
            lengths: [2; DYNAMIC_BODIES],
            fanout,
        },
        Workload {
            name: "selected-profile-like",
            lengths: [
                56, 3, 10, 2, 24, 18, 33, 29, 96, 64, 48, 45, 72, 24, 64, 80, 12, 128,
            ],
            fanout,
        },
        Workload {
            name: "wide-metadata",
            lengths: [
                512, 5, 64, 8, 128, 96, 256, 48, 1_024, 384, 96, 160, 768, 80, 320, 192, 32, 2_048,
            ],
            fanout,
        },
    ]
}

fn validate_workload(workload: &Workload) -> Result<(), String> {
    for (index, length) in workload.lengths.iter().copied().enumerate() {
        if length == 0 || length > MAX_BODY {
            return Err(format!(
                "{} body {index} has invalid length {length}",
                workload.name
            ));
        }
    }
    if workload.max_body() == 0 {
        return Err(format!("{} has no bodies", workload.name));
    }
    Ok(())
}

fn run_owned(workload: &Workload, fanout: usize) -> Result<SemanticStats, String> {
    let mut aggregate = SemanticStats::empty();
    for _ in 0..fanout {
        let mut bodies = Vec::with_capacity(DYNAMIC_BODIES);
        for (index, length) in workload.lengths.iter().copied().enumerate() {
            let mut writer = PacketWriter::with_capacity(MAX_BODY, length)
                .map_err(|error| format!("owned writer construction failed: {error:?}"))?;
            encode_synthetic_body(&mut writer, index, length)?;
            bodies.push(writer.into_bytes());
        }
        let semantic = semantic_owned(&bodies)?;
        black_box(&bodies);
        aggregate.combine(semantic)?;
    }
    Ok(aggregate)
}

fn run_arena(workload: &Workload, fanout: usize) -> Result<SemanticStats, String> {
    let mut aggregate = SemanticStats::empty();
    let body_bytes = workload.body_bytes()?;
    let max_body = workload.max_body();

    for _ in 0..fanout {
        let mut arena = DynamicBootstrapArena::<DYNAMIC_BODIES>::with_capacity(body_bytes);
        let mut scratch = PacketWriter::with_capacity(MAX_BODY, max_body)
            .map_err(|error| format!("arena scratch construction failed: {error:?}"))?;
        for (index, length) in workload.lengths.iter().copied().enumerate() {
            encode_synthetic_body(&mut scratch, index, length)?;
            let sealed = arena
                .seal_from(&mut scratch)
                .map_err(|error| format!("arena seal failed: {error:?}"))?;
            if sealed != index {
                return Err("arena insertion index drifted".to_owned());
            }
        }
        let semantic = semantic_arena(&arena)?;
        black_box(&arena);
        aggregate.combine(semantic)?;
    }
    Ok(aggregate)
}

fn run_direct_floor(workload: &Workload, fanout: usize) -> Result<SemanticStats, String> {
    let mut aggregate = SemanticStats::empty();
    let body_bytes = workload.body_bytes()?;

    for _ in 0..fanout {
        let mut bytes = Vec::with_capacity(body_bytes);
        for (index, length) in workload.lengths.iter().copied().enumerate() {
            let start = bytes.len();
            let packet_id = packet_id(index)?;
            encode_var_int(packet_id, &mut bytes);
            let prefix = bytes
                .len()
                .checked_sub(start)
                .ok_or_else(|| "direct prefix accounting underflow".to_owned())?;
            let payload_len = length
                .checked_sub(prefix)
                .ok_or_else(|| "body shorter than packet id".to_owned())?;
            bytes.extend_from_slice(&PAYLOAD[..payload_len]);
            if bytes.len() - start != length {
                return Err("direct floor produced wrong body length".to_owned());
            }
        }
        let semantic = semantic_flat(&workload.lengths, &bytes)?;
        black_box(&bytes);
        aggregate.combine(semantic)?;
    }
    Ok(aggregate)
}

fn encode_synthetic_body(
    writer: &mut PacketWriter,
    index: usize,
    target_len: usize,
) -> Result<(), String> {
    writer
        .write_var_int(packet_id(index)?)
        .map_err(|error| format!("packet id write failed: {error:?}"))?;
    let payload_len = target_len
        .checked_sub(writer.len())
        .ok_or_else(|| "body shorter than packet id".to_owned())?;
    writer
        .write_bytes(&PAYLOAD[..payload_len])
        .map_err(|error| format!("payload write failed: {error:?}"))?;
    if writer.len() != target_len {
        return Err("synthetic writer produced wrong body length".to_owned());
    }
    Ok(())
}

fn packet_id(index: usize) -> Result<i32, String> {
    i32::try_from(index + 1).map_err(|_| "synthetic packet id overflow".to_owned())
}

fn semantic_owned(bodies: &[Vec<u8>]) -> Result<SemanticStats, String> {
    if bodies.len() != DYNAMIC_BODIES {
        return Err("owned body count drifted".to_owned());
    }
    let mut stats = SemanticStats::empty();
    for body in bodies {
        add_body(&mut stats, body)?;
    }
    Ok(stats)
}

fn semantic_arena(arena: &DynamicBootstrapArena<DYNAMIC_BODIES>) -> Result<SemanticStats, String> {
    if arena.body_count() != DYNAMIC_BODIES {
        return Err("arena body count drifted".to_owned());
    }
    let mut stats = SemanticStats::empty();
    for index in 0..DYNAMIC_BODIES {
        let body = arena
            .body(index)
            .ok_or_else(|| format!("arena body {index} is missing"))?;
        add_body(&mut stats, body)?;
    }
    Ok(stats)
}

fn semantic_flat(lengths: &[usize; DYNAMIC_BODIES], bytes: &[u8]) -> Result<SemanticStats, String> {
    let mut stats = SemanticStats::empty();
    let mut start = 0usize;
    for length in lengths {
        let end = start
            .checked_add(*length)
            .ok_or_else(|| "flat body boundary overflow".to_owned())?;
        let body = bytes
            .get(start..end)
            .ok_or_else(|| "flat body boundary exceeds storage".to_owned())?;
        add_body(&mut stats, body)?;
        start = end;
    }
    if start != bytes.len() {
        return Err("flat storage contains trailing bytes".to_owned());
    }
    Ok(stats)
}

fn add_body(stats: &mut SemanticStats, body: &[u8]) -> Result<(), String> {
    stats.bodies = stats
        .bodies
        .checked_add(1)
        .ok_or_else(|| "semantic body count overflow".to_owned())?;
    stats.bytes = stats
        .bytes
        .checked_add(body.len())
        .ok_or_else(|| "semantic byte count overflow".to_owned())?;
    stats.checksum ^= u64::try_from(body.len()).map_err(|_| "body length does not fit u64")?;
    stats.checksum = stats.checksum.wrapping_mul(CHECKSUM_PRIME);
    for byte in body {
        stats.checksum ^= u64::from(*byte);
        stats.checksum = stats.checksum.wrapping_mul(CHECKSUM_PRIME);
    }
    Ok(())
}

fn require_equivalent(
    workload: &Workload,
    owned: SemanticStats,
    arena: SemanticStats,
    direct: SemanticStats,
) -> Result<(), String> {
    if owned == arena && arena == direct {
        Ok(())
    } else {
        Err(format!(
            "semantic gate mismatch for {}: owned={owned:?} arena={arena:?} direct={direct:?}",
            workload.name
        ))
    }
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
    hardware: &str,
    results: &[WorkloadResult],
) -> Result<String, String> {
    let mut out = String::from("{");
    out.push_str("\"schema\":");
    out.push_str(&SCHEMA.to_string());
    out.push_str(",\"benchmark\":\"r2b-dynamic-bootstrap-storage\"");
    out.push_str(",\"mode\":");
    push_json_string(&mut out, config.mode.as_str());
    out.push_str(",\"hosted_ci_is_diagnostic_only\":true");
    out.push_str(",\"production_mechanism_selected_by_this_run\":false");
    out.push_str(",\"dynamic_body_count\":");
    out.push_str(&DYNAMIC_BODIES.to_string());
    out.push_str(",\"measured_rounds\":");
    out.push_str(&config.measured_rounds.to_string());
    out.push_str(",\"hardware\":");
    out.push_str(hardware);
    out.push_str(",\"workloads\":[");

    for (index, result) in results.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        let body_bytes = result.workload.body_bytes()?;
        out.push('{');
        out.push_str("\"name\":");
        push_json_string(&mut out, result.workload.name);
        out.push_str(",\"fanout\":");
        out.push_str(&result.workload.fanout.to_string());
        out.push_str(",\"body_count\":");
        out.push_str(&DYNAMIC_BODIES.to_string());
        out.push_str(",\"body_bytes_per_join\":");
        out.push_str(&body_bytes.to_string());
        out.push_str(",\"byte_equivalent\":true");
        out.push_str(",\"semantic_checksum\":");
        out.push_str(&result.semantic.checksum.to_string());
        out.push_str(",\"structural\":{");
        out.push_str("\"owned_heap_buffer_owners\":");
        out.push_str(&(DYNAMIC_BODIES + 1).to_string());
        out.push_str(",\"arena_heap_buffer_owners\":2");
        out.push_str(",\"direct_floor_heap_buffer_owners\":1");
        out.push_str(",\"arena_seal_copied_bytes_per_join\":");
        out.push_str(&body_bytes.to_string());
        out.push_str(",\"owned_vector_metadata_bytes\":");
        let metadata = size_of::<Vec<u8>>()
            .checked_mul(DYNAMIC_BODIES)
            .ok_or_else(|| "metadata accounting overflow".to_owned())?;
        out.push_str(&metadata.to_string());
        out.push_str(",\"arena_handle_bytes\":");
        out.push_str(&size_of::<DynamicBootstrapArena<DYNAMIC_BODIES>>().to_string());
        out.push_str(",\"direct_floor_is_production_candidate\":false}");
        push_samples(&mut out, "owned_ns", &result.owned_ns);
        push_samples(&mut out, "arena_ns", &result.arena_ns);
        push_samples(&mut out, "direct_floor_ns", &result.direct_floor_ns);
        out.push_str(",\"owned_p50_ns\":");
        out.push_str(&median(&result.owned_ns)?.to_string());
        out.push_str(",\"arena_p50_ns\":");
        out.push_str(&median(&result.arena_ns)?.to_string());
        out.push_str(",\"direct_floor_p50_ns\":");
        out.push_str(&median(&result.direct_floor_ns)?.to_string());
        out.push('}');
    }

    out.push_str("]}");
    Ok(out)
}

fn push_samples(out: &mut String, name: &str, values: &[u128]) {
    out.push(',');
    push_json_string(out, name);
    out.push_str(":[");
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
            "--smoke" => mode = Mode::Smoke,
            "--full" => mode = Mode::Full,
            "--output" => {
                output = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--output requires a path".to_owned())?,
                ));
            }
            "--help" | "-h" => {
                println!("usage: r2b_bootstrap_arena_bench [--smoke|--full] [--output PATH]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let mut config = Config::defaults(mode);
    config.output = output;
    Ok(config)
}
