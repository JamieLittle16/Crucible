use std::env;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use crucible_benchmark_support::{collect_hardware_metadata, push_json_string};
use crucible_connection_core::{ConnectionLimits, IngressBuffer};
use crucible_protocol_core::{encode_frame, encode_var_int};

const SCHEMA: u32 = 1;
const MAX_BODY: usize = 65_536;
const INGRESS_LIMIT: usize = 2 * 1024 * 1024;
const EGRESS_LIMIT: usize = 2 * 1024 * 1024;
const CHECKSUM_SEED: u64 = 0x243F_6A88_85A3_08D3;
const CHECKSUM_MUL: u64 = 0x9E37_79B1_85EB_CA87;

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
    frames: usize,
    warmup_rounds: usize,
    measured_rounds: usize,
}

impl Config {
    const fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                frames: 256,
                warmup_rounds: 1,
                measured_rounds: 6,
            },
            Mode::Full => Self {
                mode,
                output: None,
                frames: 8_192,
                warmup_rounds: 6,
                measured_rounds: 30,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Case {
    name: &'static str,
    fragment_bytes: usize,
    action_budget: usize,
    large_frames: bool,
}

const CASES: [Case; 9] = [
    Case {
        name: "one-byte-drain",
        fragment_bytes: 1,
        action_budget: usize::MAX,
        large_frames: false,
    },
    Case {
        name: "three-byte-drain",
        fragment_bytes: 3,
        action_budget: usize::MAX,
        large_frames: false,
    },
    Case {
        name: "seven-byte-drain",
        fragment_bytes: 7,
        action_budget: usize::MAX,
        large_frames: false,
    },
    Case {
        name: "seventeen-byte-drain",
        fragment_bytes: 17,
        action_budget: usize::MAX,
        large_frames: false,
    },
    Case {
        name: "mtu-drain",
        fragment_bytes: 1_460,
        action_budget: usize::MAX,
        large_frames: false,
    },
    Case {
        name: "64k-drain",
        fragment_bytes: 64 * 1024,
        action_budget: usize::MAX,
        large_frames: false,
    },
    Case {
        name: "mtu-budget-one",
        fragment_bytes: 1_460,
        action_budget: 1,
        large_frames: false,
    },
    Case {
        name: "64k-budget-four",
        fragment_bytes: 64 * 1024,
        action_budget: 4,
        large_frames: false,
    },
    Case {
        name: "near-max-mtu",
        fragment_bytes: 1_460,
        action_budget: usize::MAX,
        large_frames: true,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SemanticResult {
    checksum: u64,
    frames: usize,
    stream_bytes: usize,
    payload_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct CompactionModel {
    physical_len: usize,
    start: usize,
    append_calls: usize,
    compactions: usize,
    moved_bytes: usize,
    peak_logical_bytes: usize,
    peak_physical_len: usize,
}

impl CompactionModel {
    fn push(&mut self, incoming: usize) -> Result<(), String> {
        self.append_calls = self
            .append_calls
            .checked_add(1)
            .ok_or_else(|| "append-call counter overflow".to_owned())?;
        if self.start != 0 {
            let physical_after = self
                .physical_len
                .checked_add(incoming)
                .ok_or_else(|| "modeled physical length overflow".to_owned())?;
            let half_consumed = self.start >= self.physical_len / 2;
            if physical_after > INGRESS_LIMIT || half_consumed {
                let active = self
                    .physical_len
                    .checked_sub(self.start)
                    .ok_or_else(|| "modeled active length underflow".to_owned())?;
                self.compactions = self
                    .compactions
                    .checked_add(1)
                    .ok_or_else(|| "compaction counter overflow".to_owned())?;
                self.moved_bytes = self
                    .moved_bytes
                    .checked_add(active)
                    .ok_or_else(|| "moved-byte counter overflow".to_owned())?;
                self.physical_len = active;
                self.start = 0;
            }
        }
        self.physical_len = self
            .physical_len
            .checked_add(incoming)
            .ok_or_else(|| "modeled physical append overflow".to_owned())?;
        self.observe()?;
        Ok(())
    }

    fn consume(&mut self, bytes: usize) -> Result<(), String> {
        let active = self.active()?;
        if bytes > active {
            return Err("modeled consume exceeds active bytes".to_owned());
        }
        self.start = self
            .start
            .checked_add(bytes)
            .ok_or_else(|| "modeled start overflow".to_owned())?;
        if self.start == self.physical_len {
            self.physical_len = 0;
            self.start = 0;
        }
        self.observe()?;
        Ok(())
    }

    fn active(self) -> Result<usize, String> {
        self.physical_len
            .checked_sub(self.start)
            .ok_or_else(|| "modeled active length underflow".to_owned())
    }

    fn observe(&mut self) -> Result<(), String> {
        self.peak_logical_bytes = self.peak_logical_bytes.max(self.active()?);
        self.peak_physical_len = self.peak_physical_len.max(self.physical_len);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
struct RoundSample {
    round: usize,
    elapsed_ns: u128,
}

#[derive(Clone, Copy, Debug)]
struct Summary {
    p50_ns: u128,
    p95_ns: u128,
    p99_ns: u128,
    max_ns: u128,
}

#[derive(Debug)]
struct CaseEvidence {
    case: Case,
    semantic: SemanticResult,
    model: CompactionModel,
    summary: Summary,
    samples: Vec<RoundSample>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ingress compaction benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let hardware = collect_hardware_metadata()?;
    let limits = ConnectionLimits::new(MAX_BODY, INGRESS_LIMIT, EGRESS_LIMIT)
        .map_err(|error| format!("benchmark limits rejected: {error:?}"))?;
    let mut evidence = Vec::with_capacity(CASES.len());

    for case in CASES {
        let frame_count = if case.large_frames {
            (config.frames / 64).max(4)
        } else {
            config.frames
        };
        let stream = build_stream(frame_count, case.large_frames)?;
        let reference = execute_case(&stream, case, limits)?;
        for _ in 0..config.warmup_rounds {
            let observed = execute_case(black_box(&stream), case, limits)?;
            require_same(reference.0, observed.0, case.name)?;
        }

        let mut samples = Vec::with_capacity(config.measured_rounds);
        for round in 0..config.measured_rounds {
            let start = Instant::now();
            let observed = execute_case(black_box(&stream), case, limits)?;
            let elapsed_ns = start.elapsed().as_nanos();
            require_same(reference.0, observed.0, case.name)?;
            if reference.1 != observed.1 {
                return Err(format!("compaction model drifted for {}", case.name));
            }
            samples.push(RoundSample { round, elapsed_ns });
        }
        let summary = summarize(&samples)?;
        evidence.push(CaseEvidence {
            case,
            semantic: reference.0,
            model: reference.1,
            summary,
            samples,
        });
    }

    let report = render_report(&config, &hardware.to_json(), &evidence)?;
    if let Some(path) = config.output {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
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

fn build_stream(frame_count: usize, large_frames: bool) -> Result<Vec<u8>, String> {
    let mut stream = Vec::new();
    for index in 0..frame_count {
        let payload_len = if large_frames {
            60_000 + index % 4_000
        } else {
            match index % 8 {
                0 | 1 | 2 => 8 + index % 17,
                3 | 4 => 48 + index % 97,
                5 | 6 => 256 + index % 769,
                _ => 4_096 + index % 2_048,
            }
        };
        let packet_id = i32::try_from(index % 127).map_err(|_| "packet id overflow".to_owned())?;
        let mut body = Vec::with_capacity(payload_len.saturating_add(5));
        encode_var_int(packet_id, &mut body);
        for offset in 0..payload_len {
            let mixed = index
                .wrapping_mul(131)
                .wrapping_add(offset.wrapping_mul(17))
                .wrapping_add(offset >> 3);
            body.push(mixed.to_le_bytes()[0]);
        }
        encode_frame(&body, MAX_BODY, &mut stream)
            .map_err(|error| format!("frame construction failed: {error:?}"))?;
    }
    Ok(stream)
}

fn execute_case(
    stream: &[u8],
    case: Case,
    limits: ConnectionLimits,
) -> Result<(SemanticResult, CompactionModel), String> {
    let mut ingress = IngressBuffer::new(limits);
    let mut model = CompactionModel::default();
    let mut cursor = 0usize;
    let mut checksum = CHECKSUM_SEED;
    let mut frames = 0usize;
    let mut payload_bytes = 0usize;

    while cursor < stream.len() || !ingress.is_empty() {
        let mut processed = 0usize;
        while processed < case.action_budget {
            let Some(frame) = ingress
                .peek_frame()
                .map_err(|error| format!("frame decode failed for {}: {error:?}", case.name))?
            else {
                break;
            };
            checksum = mix(checksum, frame.packet_id().cast_unsigned().into());
            checksum = mix(checksum, checksum_bytes(frame.payload()));
            payload_bytes = payload_bytes
                .checked_add(frame.payload().len())
                .ok_or_else(|| "payload-byte counter overflow".to_owned())?;
            let consumed = frame.stream_bytes();
            ingress
                .consume(consumed)
                .map_err(|error| format!("frame consume failed: {error:?}"))?;
            model.consume(consumed)?;
            frames = frames
                .checked_add(1)
                .ok_or_else(|| "frame counter overflow".to_owned())?;
            processed = processed
                .checked_add(1)
                .ok_or_else(|| "processed counter overflow".to_owned())?;
        }

        if processed == case.action_budget && ingress.peek_frame().map_err(|error| {
            format!("post-budget frame decode failed for {}: {error:?}", case.name)
        })?.is_some()
        {
            continue;
        }

        if cursor < stream.len() {
            let end = cursor
                .checked_add(case.fragment_bytes)
                .unwrap_or(usize::MAX)
                .min(stream.len());
            let fragment = &stream[cursor..end];
            ingress
                .push(fragment)
                .map_err(|error| format!("fragment push failed for {}: {error:?}", case.name))?;
            model.push(fragment.len())?;
            cursor = end;
        } else if !ingress.is_empty() && ingress.peek_frame().map_err(|error| {
            format!("final frame decode failed for {}: {error:?}", case.name)
        })?.is_none()
        {
            return Err(format!("{} ended with an incomplete frame", case.name));
        }
    }

    Ok((
        SemanticResult {
            checksum,
            frames,
            stream_bytes: stream.len(),
            payload_bytes,
        },
        model,
    ))
}

fn checksum_bytes(bytes: &[u8]) -> u64 {
    let mut checksum = CHECKSUM_SEED;
    for &byte in bytes {
        checksum = mix(checksum, u64::from(byte));
    }
    checksum
}

fn mix(left: u64, right: u64) -> u64 {
    left.rotate_left(13)
        .wrapping_add(right ^ CHECKSUM_SEED)
        .wrapping_mul(CHECKSUM_MUL)
}

fn require_same(
    expected: SemanticResult,
    observed: SemanticResult,
    name: &str,
) -> Result<(), String> {
    if expected == observed {
        Ok(())
    } else {
        Err(format!(
            "semantic result drifted for {name}: expected {expected:?}, observed {observed:?}"
        ))
    }
}

fn summarize(samples: &[RoundSample]) -> Result<Summary, String> {
    if samples.is_empty() {
        return Err("no measured samples".to_owned());
    }
    let mut values = samples.iter().map(|sample| sample.elapsed_ns).collect::<Vec<_>>();
    values.sort_unstable();
    Ok(Summary {
        p50_ns: percentile(&values, 50),
        p95_ns: percentile(&values, 95),
        p99_ns: percentile(&values, 99),
        max_ns: values.last().copied().unwrap_or(0),
    })
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index]
}

fn render_report(
    config: &Config,
    hardware_json: &str,
    evidence: &[CaseEvidence],
) -> Result<String, String> {
    let mut output = String::new();
    write!(output, "{{\n  \"schema\":{SCHEMA}")
        .map_err(|_| "could not render schema".to_owned())?;
    output.push_str(",\n  \"benchmark\":\"ingress-compaction-baseline\",\n  \"mode\":");
    push_json_string(&mut output, config.mode.as_str());
    output.push_str(",\n  \"hosted_ci_is_diagnostic_only\":true");
    output.push_str(",\n  \"production_path_unchanged\":true");
    output.push_str(",\n  \"hardware\":");
    output.push_str(hardware_json);
    write!(
        output,
        ",\n  \"settings\":{{\"frames\":{},\"warmup_rounds\":{},\"measured_rounds\":{}}}",
        config.frames, config.warmup_rounds, config.measured_rounds
    )
    .map_err(|_| "could not render settings".to_owned())?;
    output.push_str(",\n  \"cases\":[");

    for (case_index, item) in evidence.iter().enumerate() {
        if case_index != 0 {
            output.push(',');
        }
        output.push_str("{\"name\":");
        push_json_string(&mut output, item.case.name);
        write!(
            output,
            ",\"fragment_bytes\":{},\"action_budget\":{},\"large_frames\":{},\"semantic\":{{\"checksum\":{},\"frames\":{},\"stream_bytes\":{},\"payload_bytes\":{}}},\"compaction_model\":{{\"append_calls\":{},\"compactions\":{},\"moved_bytes\":{},\"peak_logical_bytes\":{},\"peak_physical_len\":{}}},\"summary_ns\":{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}},\"rounds\":[",
            item.case.fragment_bytes,
            item.case.action_budget,
            item.case.large_frames,
            item.semantic.checksum,
            item.semantic.frames,
            item.semantic.stream_bytes,
            item.semantic.payload_bytes,
            item.model.append_calls,
            item.model.compactions,
            item.model.moved_bytes,
            item.model.peak_logical_bytes,
            item.model.peak_physical_len,
            item.summary.p50_ns,
            item.summary.p95_ns,
            item.summary.p99_ns,
            item.summary.max_ns
        )
        .map_err(|_| "could not render case".to_owned())?;
        for (sample_index, sample) in item.samples.iter().enumerate() {
            if sample_index != 0 {
                output.push(',');
            }
            write!(
                output,
                "{{\"round\":{},\"elapsed_ns\":{}}}",
                sample.round, sample.elapsed_ns
            )
            .map_err(|_| "could not render sample".to_owned())?;
        }
        output.push_str("]}");
    }
    output.push_str("]\n}\n");
    Ok(output)
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut args = args.into_iter().peekable();
    let mut mode = None;
    let mut output = None;
    let mut frames = None;
    let mut warmup_rounds = None;
    let mut measured_rounds = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            "--frames" => {
                frames = Some(parse_positive(&next_value(&mut args, "--frames")?, "frames")?)
            }
            "--warmup-rounds" => {
                warmup_rounds = Some(parse_positive(
                    &next_value(&mut args, "--warmup-rounds")?,
                    "warmup rounds",
                )?)
            }
            "--measured-rounds" => {
                measured_rounds = Some(parse_positive(
                    &next_value(&mut args, "--measured-rounds")?,
                    "measured rounds",
                )?)
            }
            "--help" | "-h" => {
                return Err("usage: ingress_compaction_bench (--smoke|--full) [--output PATH] [--frames N] [--warmup-rounds N] [--measured-rounds N]".to_owned());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    if let Some(value) = frames {
        config.frames = value;
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
        return Err("benchmark mode specified more than once".to_owned());
    }
    Ok(())
}

fn next_value(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    flag: &str,
) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value after {flag}"))
}

fn parse_positive(value: &str, label: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|error| format!("invalid {label}: {error}"))?;
    if parsed == 0 {
        return Err(format!("{label} must be positive"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{CASES, Config, ConnectionLimits, INGRESS_LIMIT, MAX_BODY, Mode, build_stream, execute_case};

    #[test]
    fn every_case_preserves_one_semantic_stream_and_coherent_model() {
        let config = Config::defaults(Mode::Smoke);
        let limits = ConnectionLimits::new(MAX_BODY, INGRESS_LIMIT, INGRESS_LIMIT)
            .expect("coherent benchmark limits");
        for case in CASES {
            let frames = if case.large_frames {
                (config.frames / 64).max(4)
            } else {
                config.frames
            };
            let stream = build_stream(frames, case.large_frames).expect("build benchmark stream");
            let (semantic, model) = execute_case(&stream, case, limits).expect("execute case");
            assert_eq!(semantic.frames, frames);
            assert_eq!(semantic.stream_bytes, stream.len());
            assert_ne!(semantic.checksum, 0);
            assert!(model.append_calls > 0);
            assert!(model.peak_logical_bytes <= INGRESS_LIMIT);
            assert!(model.peak_physical_len <= INGRESS_LIMIT);
        }
    }
}
