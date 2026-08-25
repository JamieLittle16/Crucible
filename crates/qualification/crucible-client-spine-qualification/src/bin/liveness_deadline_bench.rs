use std::env;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use crucible_benchmark_support::collect_hardware_metadata;
use crucible_session_core::{KeepAliveReply, LivenessDecision, LivenessPolicy, LivenessState};

const SCHEMA: u32 = 1;
const TICK_MS: u64 = 50;
const KEEP_ALIVE_MS: u64 = 15_000;
const HORIZON_MS: u64 = 60_000;
const ACK_DELAY_MS: u64 = 100;
const CHECKSUM_SEED: u64 = 0xE8B5_7A13_9D42_C601;
const CHECKSUM_MUL: u64 = 0xD6E8_FEB8_6659_FD93;

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
    sessions: usize,
    warmup_rounds: usize,
    measured_rounds: usize,
}

impl Config {
    fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                sessions: 2_048,
                warmup_rounds: 2,
                measured_rounds: 10,
            },
            Mode::Full => Self {
                mode,
                output: None,
                sessions: 8_192,
                warmup_rounds: 6,
                measured_rounds: 16,
            },
        }
    }
}

#[derive(Clone, Debug)]
struct PairEvidence {
    scan_ns: Vec<u128>,
    deadline_ns: Vec<u128>,
    checksum: u64,
    scan_service_calls: u64,
    deadline_service_calls: u64,
    reply_calls: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("liveness deadline benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let hardware = collect_hardware_metadata()?;
    let policy = LivenessPolicy::new(KEEP_ALIVE_MS, KEEP_ALIVE_MS)
        .map_err(|error| format!("invalid benchmark policy: {error:?}"))?;
    let prototype = prepare_sessions(config.sessions)?;

    let scan_reference = || run_scan_reference(&prototype, policy);
    let deadline_candidate = || run_deadline_candidate(&prototype, policy);
    let evidence = benchmark_pair(
        config.warmup_rounds,
        config.measured_rounds,
        scan_reference,
        deadline_candidate,
    )?;

    let artifact = render_report(&config, &hardware.to_json(), &evidence);
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
    let mut args = args.into_iter();
    let mut mode = None;
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--output" => {
                output = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--output requires a path".to_owned())?,
                ));
            }
            "--help" | "-h" => {
                return Err(
                    "usage: liveness_deadline_bench (--smoke|--full) [--output PATH]".to_owned(),
                );
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    Ok(config)
}

fn set_mode(slot: &mut Option<Mode>, value: Mode) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err("specify exactly one benchmark mode".to_owned());
    }
    Ok(())
}

fn prepare_sessions(count: usize) -> Result<Vec<LivenessState>, String> {
    (0..count)
        .map(|index| {
            let latency = i32::try_from(index % 201).expect("bounded initial latency");
            LivenessState::new(0, latency)
                .map_err(|error| format!("could not create session {index}: {error:?}"))
        })
        .collect()
}

fn run_scan_reference(
    prototype: &[LivenessState],
    policy: LivenessPolicy,
) -> Result<(u64, u64, u64), String> {
    let mut sessions = prototype.to_vec();
    let mut checksum = CHECKSUM_SEED;
    let mut service_calls = 0_u64;
    let mut reply_calls = 0_u64;

    for now in (0..=HORIZON_MS).step_by(TICK_MS as usize) {
        let is_reply_tick = now >= KEEP_ALIVE_MS + ACK_DELAY_MS
            && (now - ACK_DELAY_MS).is_multiple_of(KEEP_ALIVE_MS);
        if is_reply_tick {
            for (index, state) in sessions.iter_mut().enumerate() {
                let id = state
                    .pending_challenge()
                    .ok_or_else(|| format!("reference reply without pending challenge at {now}"))?;
                let reply = state
                    .receive_keep_alive(now, id)
                    .map_err(|error| format!("reference reply failed: {error:?}"))?;
                checksum = mix_reply(checksum, index, reply);
                reply_calls += 1;
            }
        }

        for (index, state) in sessions.iter_mut().enumerate() {
            let decision = state
                .service(now, policy)
                .map_err(|error| format!("reference service failed: {error:?}"))?;
            checksum = mix_decision(checksum, index, decision);
            service_calls += 1;
        }
    }

    black_box(Ok((checksum, service_calls, reply_calls)))
}

fn run_deadline_candidate(
    prototype: &[LivenessState],
    policy: LivenessPolicy,
) -> Result<(u64, u64, u64), String> {
    let mut sessions = prototype.to_vec();
    let mut checksum = CHECKSUM_SEED;
    let mut service_calls = 0_u64;
    let mut reply_calls = 0_u64;
    let mut deadline = KEEP_ALIVE_MS;

    while deadline <= HORIZON_MS {
        for (index, state) in sessions.iter_mut().enumerate() {
            if state.next_deadline_ms(policy) != deadline {
                return Err(format!("candidate frontier drift at session {index}"));
            }
            let decision = state
                .service(deadline, policy)
                .map_err(|error| format!("candidate service failed: {error:?}"))?;
            checksum = mix_decision(checksum, index, decision);
            service_calls += 1;
        }

        let reply_at = deadline + ACK_DELAY_MS;
        if reply_at <= HORIZON_MS {
            for (index, state) in sessions.iter_mut().enumerate() {
                let id = state.pending_challenge().ok_or_else(|| {
                    format!("candidate reply without pending challenge at {reply_at}")
                })?;
                let reply = state
                    .receive_keep_alive(reply_at, id)
                    .map_err(|error| format!("candidate reply failed: {error:?}"))?;
                checksum = mix_reply(checksum, index, reply);
                reply_calls += 1;
            }
        }
        deadline += KEEP_ALIVE_MS;
    }

    black_box(Ok((checksum, service_calls, reply_calls)))
}

fn mix_decision(checksum: u64, index: usize, decision: LivenessDecision) -> u64 {
    let encoded = match decision {
        LivenessDecision::Idle => 0_u64,
        LivenessDecision::IssueChallenge { id } => {
            1_u64 ^ u64::from_ne_bytes(id.to_ne_bytes()).rotate_left(7)
        }
        LivenessDecision::KeepAliveTimedOut => 2_u64,
        LivenessDecision::ClosedTimedOut => 3_u64,
    };
    checksum.rotate_left(11)
        ^ encoded.wrapping_mul(CHECKSUM_MUL)
        ^ u64::try_from(index)
            .expect("benchmark session index fits u64")
            .rotate_left(23)
}

fn mix_reply(checksum: u64, index: usize, reply: KeepAliveReply) -> u64 {
    let encoded = match reply {
        KeepAliveReply::Accepted { latency_ms } => {
            0xA11C_E001_u64 ^ u64::from(u32::from_ne_bytes(latency_ms.to_ne_bytes()))
        }
        KeepAliveReply::Rejected => 0xBAD0_0001,
    };
    checksum.rotate_left(13)
        ^ encoded.wrapping_mul(CHECKSUM_MUL)
        ^ u64::try_from(index)
            .expect("benchmark session index fits u64")
            .rotate_left(29)
}

fn benchmark_pair<R, C>(
    warmup_rounds: usize,
    measured_rounds: usize,
    mut reference: R,
    mut candidate: C,
) -> Result<PairEvidence, String>
where
    R: FnMut() -> Result<(u64, u64, u64), String>,
    C: FnMut() -> Result<(u64, u64, u64), String>,
{
    let reference_semantics = reference()?;
    let candidate_semantics = candidate()?;
    if reference_semantics.0 != candidate_semantics.0 {
        return Err(format!(
            "semantic checksum mismatch: scan={} deadline={}",
            reference_semantics.0, candidate_semantics.0
        ));
    }
    if reference_semantics.2 != candidate_semantics.2 {
        return Err("reply work mismatch between candidates".to_owned());
    }

    for round in 0..warmup_rounds {
        if round % 2 == 0 {
            black_box(reference()?);
            black_box(candidate()?);
        } else {
            black_box(candidate()?);
            black_box(reference()?);
        }
    }

    let mut scan_ns = Vec::with_capacity(measured_rounds);
    let mut deadline_ns = Vec::with_capacity(measured_rounds);
    for round in 0..measured_rounds {
        if round % 2 == 0 {
            scan_ns.push(time_once(&mut reference)?);
            deadline_ns.push(time_once(&mut candidate)?);
        } else {
            deadline_ns.push(time_once(&mut candidate)?);
            scan_ns.push(time_once(&mut reference)?);
        }
    }

    Ok(PairEvidence {
        scan_ns,
        deadline_ns,
        checksum: reference_semantics.0,
        scan_service_calls: reference_semantics.1,
        deadline_service_calls: candidate_semantics.1,
        reply_calls: reference_semantics.2,
    })
}

fn time_once<F>(work: &mut F) -> Result<u128, String>
where
    F: FnMut() -> Result<(u64, u64, u64), String>,
{
    let start = Instant::now();
    black_box(work()?);
    Ok(start.elapsed().as_nanos())
}

fn render_report(config: &Config, hardware_json: &str, evidence: &PairEvidence) -> String {
    let mut output = String::new();
    write!(
        output,
        "{{\"schema\":{SCHEMA},\"benchmark\":\"liveness-deadline-frontier\",\"mode\":\"{}\",\"hosted_ci_is_diagnostic_only\":true,\"scheduler_mechanism_selected\":false,\"sessions\":{},\"tick_ms\":{TICK_MS},\"keep_alive_ms\":{KEEP_ALIVE_MS},\"horizon_ms\":{HORIZON_MS},\"ack_delay_ms\":{ACK_DELAY_MS},\"warmup_rounds\":{},\"measured_rounds\":{},\"hardware\":{hardware_json},\"semantic_equivalent\":true,\"checksum\":{},\"scan_service_calls\":{},\"deadline_service_calls\":{},\"reply_calls\":{},\"scan_ns\":",
        config.mode.as_str(),
        config.sessions,
        config.warmup_rounds,
        config.measured_rounds,
        evidence.checksum,
        evidence.scan_service_calls,
        evidence.deadline_service_calls,
        evidence.reply_calls,
    )
    .expect("writing to String cannot fail");
    push_u128_array(&mut output, &evidence.scan_ns);
    output.push_str(",\"deadline_ns\":");
    push_u128_array(&mut output, &evidence.deadline_ns);
    output.push('}');
    output
}

fn push_u128_array(output: &mut String, values: &[u128]) {
    output.push('[');
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(output, "{value}").expect("writing to String cannot fail");
    }
    output.push(']');
}
