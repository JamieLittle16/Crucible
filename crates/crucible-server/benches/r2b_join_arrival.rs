use std::env;
use std::fs;
use std::hint::black_box;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Instant;

use crucible_packet_core::PacketWriter;
use crucible_server::{
    R2bEntryOutcome, ServerSessionEpoch, enter_r2b_play_blocking_transport,
};
use crucible_target_26_2::{
    Target26_2R1xContext,
    r2b::{
        BootstrapGameMode, BootstrapWeather, ChangeDifficultyPayload, ClockFullSyncPayload,
        ClockUpdate, CommandPermissionProfile, CommandProjectionArtifact, CommandProjectionKey,
        DefaultSpawnPayload, Difficulty26_2, FreshCommonSpawnInfo, FreshEmptyInventoryPayload,
        FreshLoginFlags, FreshLoginPayload, FreshR2bBootstrapSnapshot, HeldSlotPayload,
        InitialPlayerInfoEntry, PermissionEntityEventPayload, PermissionLevelEvent,
        PlayBootstrapImage26_2, PlayerAbilitiesPayload, PlayerAbilityFlags, ProjectionRevision,
        RecipeBookSettingFlags, RecipeBookSettingsPayload, RecipeProjectionArtifact,
        RecipeProjectionKey, TeleportDestination, TickingStatePayload, TickingStepPayload,
        WorldBorderPayload,
    },
};

const SCHEMA: u32 = 1;
const RATIO_SCALE_PPM: u128 = 1_000_000;
const NANOSECONDS_PER_SECOND: u128 = 1_000_000_000;
const CHECKSUM_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const CHECKSUM_PRIME: u64 = 0x0000_0100_0000_01b3;
const CONFIGURATION_BODY_SIZES: [usize; 34] = [
    25, 20, 22, 1_612, 224, 327, 227, 184, 149, 77, 80, 78, 233, 66, 66, 77, 70, 81, 73, 980,
    282, 116, 1_143, 1_036, 968, 416, 237, 48, 49, 94, 64, 103, 35_204, 1,
];
const LEVELS: [&str; 3] = [
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];
const CLOCKS: [ClockUpdate; 2] = [
    ClockUpdate {
        registry_id: 0,
        total_ticks: 42,
        partial_tick: 0.0,
        rate: 1.0,
    },
    ClockUpdate {
        registry_id: 1,
        total_ticks: 42,
        partial_tick: 0.0,
        rate: 1.0,
    },
];
const SELF: InitialPlayerInfoEntry<'static> = InitialPlayerInfoEntry {
    profile_id: [
        0x68, 0x20, 0x14, 0xfe, 0xad, 0x63, 0x36, 0x99, 0xaa, 0xda, 0x79, 0xaa, 0x08, 0xd9, 0x5b,
        0x45,
    ],
    name: "Stato16",
    game_mode: BootstrapGameMode::Survival,
    listed: true,
    latency: 0,
    list_order: 0,
    show_hat: true,
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
}

#[derive(Clone, Debug)]
struct Config {
    mode: Mode,
    output: Option<PathBuf>,
    warmup_joins: usize,
    measured_joins: usize,
    joins_per_epoch: usize,
}

impl Config {
    const fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                warmup_joins: 64,
                measured_joins: 512,
                joins_per_epoch: 64,
            },
            Mode::Full => Self {
                mode,
                output: None,
                warmup_joins: 512,
                measured_joins: 8_192,
                joins_per_epoch: 256,
            },
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.measured_joins < 128 {
            return Err("at least 128 measured joins are required".to_owned());
        }
        if self.joins_per_epoch == 0 || !self.measured_joins.is_multiple_of(self.joins_per_epoch) {
            return Err("measured joins must divide into complete epochs".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JoinWitness {
    output_bytes: usize,
    output_checksum: u64,
    read_calls: usize,
    read_bytes: usize,
    retained_read_scratch_bytes: usize,
    pending_teleport_id: i32,
}

#[derive(Debug)]
struct Fixture {
    context: Target26_2R1xContext,
    image: PlayBootstrapImage26_2,
    login: Vec<u8>,
    known_pack: Vec<u8>,
    finish_configuration: Vec<u8>,
    epoch: ServerSessionEpoch,
}

impl Fixture {
    fn new() -> Result<Self, String> {
        let configuration = configuration_bodies();
        let context = Target26_2R1xContext::new("{}".into(), configuration, Vec::new())
            .map_err(|error| format!("configuration-only context: {error:?}"))?;
        if context.play_frame_count() != 0 || context.play_body_bytes() != 0 {
            return Err("arrival benchmark requires zero captured Play bodies".to_owned());
        }
        Ok(Self {
            context,
            image: image()?,
            login: login_client_chunk()?,
            known_pack: known_pack_chunk()?,
            finish_configuration: frame(&[3])?,
            epoch: session_epoch()?,
        })
    }

    fn transport(&self) -> CountingTransport<'_> {
        CountingTransport::new([
            self.login.as_slice(),
            self.known_pack.as_slice(),
            self.finish_configuration.as_slice(),
        ])
    }
}

#[derive(Debug)]
struct CountingTransport<'a> {
    reads: [&'a [u8]; 3],
    next_read: usize,
    read_bytes: usize,
    output_bytes: usize,
    output_checksum: u64,
}

impl<'a> CountingTransport<'a> {
    const fn new(reads: [&'a [u8]; 3]) -> Self {
        Self {
            reads,
            next_read: 0,
            read_bytes: 0,
            output_bytes: 0,
            output_checksum: CHECKSUM_OFFSET,
        }
    }
}

impl Read for CountingTransport<'_> {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        let Some(chunk) = self.reads.get(self.next_read).copied() else {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        };
        if chunk.len() > destination.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "scripted client chunk exceeds retained read scratch",
            ));
        }
        destination[..chunk.len()].copy_from_slice(chunk);
        self.next_read += 1;
        self.read_bytes = self.read_bytes.saturating_add(chunk.len());
        Ok(chunk.len())
    }
}

impl Write for CountingTransport<'_> {
    fn write(&mut self, source: &[u8]) -> io::Result<usize> {
        self.output_bytes = self.output_bytes.saturating_add(source.len());
        for byte in source {
            self.output_checksum ^= u64::from(*byte);
            self.output_checksum = self.output_checksum.wrapping_mul(CHECKSUM_PRIME);
        }
        Ok(source.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Distribution {
    count: usize,
    min: u128,
    p01: u128,
    p05: u128,
    p50: u128,
    p95: u128,
    p99: u128,
    p999: u128,
    max: u128,
    mean: u128,
    mad: u128,
    relative_mad_ppm: u128,
}

impl Distribution {
    fn from_samples(values: &[u128]) -> Result<Self, String> {
        if values.is_empty() {
            return Err("empty distribution".to_owned());
        }
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        let p50 = quantile(&sorted, 500)?;
        let mut deviations = Vec::with_capacity(sorted.len());
        deviations.extend(sorted.iter().map(|value| value.abs_diff(p50)));
        deviations.sort_unstable();
        let mad = quantile(&deviations, 500)?;
        let divisor = u128::try_from(sorted.len())
            .map_err(|_| "sample count does not fit u128".to_owned())?;
        let mean = checked_sum(&sorted)?
            .checked_div(divisor)
            .ok_or_else(|| "sample count must be positive".to_owned())?;
        Ok(Self {
            count: sorted.len(),
            min: sorted[0],
            p01: quantile(&sorted, 10)?,
            p05: quantile(&sorted, 50)?,
            p50,
            p95: quantile(&sorted, 950)?,
            p99: quantile(&sorted, 990)?,
            p999: quantile(&sorted, 999)?,
            max: sorted[sorted.len() - 1],
            mean,
            mad,
            relative_mad_ppm: ratio_ppm(mad, p50)?,
        })
    }
}

#[derive(Debug)]
struct ArrivalProfile {
    name: &'static str,
    nominal_interval_ns: u128,
    effective_offered_load_ppm: u128,
    queue_delay_ns: Distribution,
    sojourn_ns: Distribution,
    completion_spacing_ns: Distribution,
    backlog_in_system: Distribution,
    queued_join_rate_ppm: u128,
    compressed_completion_rate_ppm: u128,
    max_queued_run: usize,
    modeled_span_ns: u128,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("R2B join-arrival benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    config.validate()?;
    let fixture = Fixture::new()?;
    let expected = run_live_join(&fixture)?.1;
    validate_witness(expected, expected)?;

    for _ in 0..config.warmup_joins {
        let (_, witness) = run_live_join(&fixture)?;
        validate_witness(witness, expected)?;
    }

    let mut service_ns = Vec::with_capacity(config.measured_joins);
    for _ in 0..config.measured_joins {
        let (elapsed, witness) = run_live_join(&fixture)?;
        validate_witness(witness, expected)?;
        service_ns.push(elapsed);
    }
    black_box(&service_ns);

    let service = Distribution::from_samples(&service_ns)?;
    let epoch_mean_ns = epoch_means(&service_ns, config.joins_per_epoch)?;
    let service_epoch_mean = Distribution::from_samples(&epoch_mean_ns)?;
    let steady_80_interval = mul_div_ceil(service.mean, 5, 4)?;
    let steady_95_interval = mul_div_ceil(service.mean, 21, 20)?;
    let burst_interval = mul_div_ceil(service.mean, 3, 2)?;

    let mut profiles = Vec::with_capacity(6);
    profiles.push(model_arrivals(
        "steady-80pct",
        &service_ns,
        steady_offers(service_ns.len(), steady_80_interval)?,
        steady_80_interval,
    )?);
    profiles.push(model_arrivals(
        "steady-95pct",
        &service_ns,
        steady_offers(service_ns.len(), steady_95_interval)?,
        steady_95_interval,
    )?);
    for (name, phase) in [
        ("burst-8of64-phase0", 0_usize),
        ("burst-8of64-phase16", 16),
        ("burst-8of64-phase32", 32),
        ("burst-8of64-phase48", 48),
    ] {
        profiles.push(model_arrivals(
            name,
            &service_ns,
            burst_offers(service_ns.len(), burst_interval, phase)?,
            burst_interval,
        )?);
    }

    let artifact = render_json(
        &config,
        expected,
        service,
        service_epoch_mean,
        &profiles,
    );
    write_artifact(config.output.as_ref(), &artifact)?;

    println!(
        "R2B join service p50={}ns p99={}ns p999={}ns mean={}ns serial_capacity={} joins/s",
        service.p50,
        service.p99,
        service.p999,
        service.mean,
        NANOSECONDS_PER_SECOND / service.mean.max(1),
    );
    for profile in &profiles {
        println!(
            "R2B arrival profile={} load={}ppm queue_p99={}ns queue_p999={}ns backlog_max={} completion_gap_p01={}ns compressed={}ppm max_queued_run={}",
            profile.name,
            profile.effective_offered_load_ppm,
            profile.queue_delay_ns.p99,
            profile.queue_delay_ns.p999,
            profile.backlog_in_system.max,
            profile.completion_spacing_ns.p01,
            profile.compressed_completion_rate_ppm,
            profile.max_queued_run,
        );
    }
    Ok(())
}

fn run_live_join(fixture: &Fixture) -> Result<(u128, JoinWitness), String> {
    let mut transport = fixture.transport();
    let start = Instant::now();
    let outcome = enter_r2b_play_blocking_transport(
        &mut transport,
        fixture.epoch,
        &fixture.context,
        &fixture.image,
        snapshot(),
    )
    .map_err(|error| format!("live R2B entry failed: {error:?}"))?;
    let elapsed = start.elapsed().as_nanos();

    let R2bEntryOutcome::WorldProjectionReady(session) = outcome else {
        return Err(format!("unexpected non-world R2B outcome: {outcome:?}"));
    };
    if session.buffered_ingress() != 0 || session.queued_egress() != 0 {
        return Err("world handoff must have drained userspace queues".to_owned());
    }
    let pending = session
        .teleport_transaction()
        .awaiting()
        .ok_or_else(|| "world handoff must retain teleport acknowledgement state".to_owned())?;
    let witness = JoinWitness {
        output_bytes: transport.output_bytes,
        output_checksum: transport.output_checksum,
        read_calls: transport.next_read,
        read_bytes: transport.read_bytes,
        retained_read_scratch_bytes: session.read_scratch_bytes(),
        pending_teleport_id: pending.id,
    };
    black_box(&session);
    Ok((elapsed, witness))
}

fn validate_witness(actual: JoinWitness, expected: JoinWitness) -> Result<(), String> {
    if actual != expected {
        return Err(format!(
            "live join semantic witness changed: expected={expected:?} actual={actual:?}"
        ));
    }
    if actual.read_calls != 3 || actual.pending_teleport_id != 1 {
        return Err(format!("unexpected selected-route witness: {actual:?}"));
    }
    if actual.output_bytes == 0 || actual.output_checksum == CHECKSUM_OFFSET {
        return Err("R2B entry produced no qualified output".to_owned());
    }
    Ok(())
}

fn model_arrivals(
    name: &'static str,
    service_ns: &[u128],
    offered_ns: Vec<u128>,
    nominal_interval_ns: u128,
) -> Result<ArrivalProfile, String> {
    if service_ns.len() != offered_ns.len() || service_ns.len() < 2 || nominal_interval_ns == 0 {
        return Err("invalid arrival-model inputs".to_owned());
    }

    let mut completion_ns = Vec::with_capacity(service_ns.len());
    let mut queue_delay_ns = Vec::with_capacity(service_ns.len());
    let mut sojourn_ns = Vec::with_capacity(service_ns.len());
    let mut backlog = Vec::with_capacity(service_ns.len());
    let mut server_free_ns = 0_u128;
    let mut completed_before = 0_usize;
    let mut queued = 0_usize;
    let mut queued_run = 0_usize;
    let mut max_queued_run = 0_usize;

    for (index, (&offered, &service)) in offered_ns.iter().zip(service_ns).enumerate() {
        while completed_before < completion_ns.len() && completion_ns[completed_before] <= offered {
            completed_before += 1;
        }
        let in_system_before = index
            .checked_sub(completed_before)
            .ok_or_else(|| "arrival backlog accounting underflow".to_owned())?;
        backlog.push(
            u128::try_from(in_system_before + 1)
                .map_err(|_| "backlog does not fit u128".to_owned())?,
        );

        let start = server_free_ns.max(offered);
        let queue = start - offered;
        let completion = start
            .checked_add(service)
            .ok_or_else(|| "modeled completion overflow".to_owned())?;
        let sojourn = completion - offered;
        queue_delay_ns.push(queue);
        sojourn_ns.push(sojourn);
        completion_ns.push(completion);
        server_free_ns = completion;

        if queue != 0 {
            queued += 1;
            queued_run += 1;
            max_queued_run = max_queued_run.max(queued_run);
        } else {
            queued_run = 0;
        }
    }

    let completion_spacing_ns = completion_ns
        .windows(2)
        .map(|window| window[1] - window[0])
        .collect::<Vec<_>>();
    let compressed_threshold = nominal_interval_ns / 2;
    let compressed = completion_spacing_ns
        .iter()
        .filter(|spacing| **spacing < compressed_threshold)
        .count();
    let offered_span = offered_ns[offered_ns.len() - 1]
        .checked_sub(offered_ns[0])
        .and_then(|span| span.checked_add(nominal_interval_ns))
        .ok_or_else(|| "offered arrival span overflow".to_owned())?;
    let service_demand = checked_sum(service_ns)?;
    let modeled_span_ns = completion_ns[completion_ns.len() - 1]
        .checked_sub(offered_ns[0])
        .ok_or_else(|| "modeled span underflow".to_owned())?;

    Ok(ArrivalProfile {
        name,
        nominal_interval_ns,
        effective_offered_load_ppm: ratio_ppm(service_demand, offered_span)?,
        queue_delay_ns: Distribution::from_samples(&queue_delay_ns)?,
        sojourn_ns: Distribution::from_samples(&sojourn_ns)?,
        completion_spacing_ns: Distribution::from_samples(&completion_spacing_ns)?,
        backlog_in_system: Distribution::from_samples(&backlog)?,
        queued_join_rate_ppm: rate_ppm(queued, service_ns.len())?,
        compressed_completion_rate_ppm: rate_ppm(compressed, completion_spacing_ns.len())?,
        max_queued_run,
        modeled_span_ns,
    })
}

fn steady_offers(count: usize, interval_ns: u128) -> Result<Vec<u128>, String> {
    let mut offered = Vec::with_capacity(count);
    let mut next = 0_u128;
    for _ in 0..count {
        offered.push(next);
        next = next
            .checked_add(interval_ns)
            .ok_or_else(|| "steady offered time overflow".to_owned())?;
    }
    Ok(offered)
}

fn burst_offers(count: usize, interval_ns: u128, phase: usize) -> Result<Vec<u128>, String> {
    let mut offered = Vec::with_capacity(count);
    let mut next = 0_u128;
    for index in 0..count {
        let position = (index + phase) % 64;
        offered.push(next);
        if position < 56 || position == 63 {
            next = next
                .checked_add(interval_ns)
                .ok_or_else(|| "burst offered time overflow".to_owned())?;
        }
    }
    Ok(offered)
}

fn epoch_means(values: &[u128], epoch_len: usize) -> Result<Vec<u128>, String> {
    if epoch_len == 0 || !values.len().is_multiple_of(epoch_len) {
        return Err("invalid service epoch partition".to_owned());
    }
    let divisor = u128::try_from(epoch_len).map_err(|_| "epoch size overflow".to_owned())?;
    values
        .chunks_exact(epoch_len)
        .map(|epoch| {
            checked_sum(epoch)?.checked_div(divisor).ok_or_else(|| {
                "service epoch divisor must remain positive".to_owned()
            })
        })
        .collect()
}

fn quantile(sorted: &[u128], permille: usize) -> Result<u128, String> {
    if sorted.is_empty() || permille == 0 || permille > 1_000 {
        return Err("invalid quantile request".to_owned());
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

fn checked_sum(values: &[u128]) -> Result<u128, String> {
    values.iter().try_fold(0_u128, |sum, value| {
        sum.checked_add(*value)
            .ok_or_else(|| "timing sum overflow".to_owned())
    })
}

fn ratio_ppm(numerator: u128, denominator: u128) -> Result<u128, String> {
    numerator
        .checked_mul(RATIO_SCALE_PPM)
        .ok_or_else(|| "ratio overflow".to_owned())?
        .checked_div(denominator)
        .ok_or_else(|| "ratio denominator must be positive".to_owned())
}

fn rate_ppm(successes: usize, total: usize) -> Result<u128, String> {
    ratio_ppm(
        u128::try_from(successes).map_err(|_| "success count overflow".to_owned())?,
        u128::try_from(total).map_err(|_| "total count overflow".to_owned())?,
    )
}

fn mul_div_ceil(value: u128, numerator: u128, denominator: u128) -> Result<u128, String> {
    let scaled = value
        .checked_mul(numerator)
        .ok_or_else(|| "interval scaling overflow".to_owned())?;
    scaled
        .checked_add(denominator - 1)
        .ok_or_else(|| "interval rounding overflow".to_owned())?
        .checked_div(denominator)
        .ok_or_else(|| "interval denominator must be positive".to_owned())
}

fn render_json(
    config: &Config,
    witness: JoinWitness,
    service: Distribution,
    service_epoch_mean: Distribution,
    profiles: &[ArrivalProfile],
) -> String {
    let mut out = String::from("{");
    push_u128(&mut out, "schema", u128::from(SCHEMA));
    push_string(&mut out, "benchmark", "r2b-join-arrival-smoothness");
    push_string(&mut out, "mode", config.mode.as_str());
    push_bool(&mut out, "hosted_ci_is_diagnostic_only", true);
    push_bool(&mut out, "runtime_scheduler_selected_by_this_benchmark", false);
    push_string(&mut out, "model_scope", "one-r2b-entry-service-lane");
    push_string(
        &mut out,
        "timed_boundary",
        "enter_r2b_play_blocking_transport-to-WorldProjectionReady",
    );
    out.push_str(",\"sampling\":{");
    push_usize_first(&mut out, "warmup_joins", config.warmup_joins);
    push_usize(&mut out, "measured_joins", config.measured_joins);
    push_usize(&mut out, "joins_per_epoch", config.joins_per_epoch);
    out.push('}');
    out.push_str(",\"host\":{");
    push_string_first(&mut out, "os", env::consts::OS);
    push_string(&mut out, "arch", env::consts::ARCH);
    push_usize(
        &mut out,
        "available_parallelism",
        std::thread::available_parallelism().map_or(1, |value| value.get()),
    );
    if let Ok(cpu) = env::var("CRUCIBLE_BENCH_CPU") {
        push_string(&mut out, "pinned_cpu", &cpu);
    }
    out.push('}');
    out.push_str(",\"semantic_witness\":{");
    push_usize_first(&mut out, "output_bytes", witness.output_bytes);
    push_u128(&mut out, "output_checksum", u128::from(witness.output_checksum));
    push_usize(&mut out, "read_calls", witness.read_calls);
    push_usize(&mut out, "read_bytes", witness.read_bytes);
    push_usize(
        &mut out,
        "retained_read_scratch_bytes",
        witness.retained_read_scratch_bytes,
    );
    push_i32(&mut out, "pending_teleport_id", witness.pending_teleport_id);
    out.push('}');
    push_distribution(&mut out, "service_ns", service);
    push_distribution(&mut out, "service_epoch_mean_ns", service_epoch_mean);
    push_u128(
        &mut out,
        "estimated_single_lane_capacity_joins_per_second",
        NANOSECONDS_PER_SECOND / service.mean.max(1),
    );
    out.push_str(",\"arrival_profiles\":[");
    for (index, profile) in profiles.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        push_profile(&mut out, profile);
    }
    out.push_str("]}");
    out
}

fn push_profile(out: &mut String, profile: &ArrivalProfile) {
    out.push('{');
    push_string_first(out, "name", profile.name);
    push_u128(out, "nominal_interval_ns", profile.nominal_interval_ns);
    push_u128(
        out,
        "effective_offered_load_ppm",
        profile.effective_offered_load_ppm,
    );
    push_distribution(out, "queue_delay_ns", profile.queue_delay_ns);
    push_distribution(out, "sojourn_ns", profile.sojourn_ns);
    push_distribution(
        out,
        "completion_spacing_ns",
        profile.completion_spacing_ns,
    );
    push_distribution(out, "backlog_in_system", profile.backlog_in_system);
    push_u128(out, "queued_join_rate_ppm", profile.queued_join_rate_ppm);
    push_u128(
        out,
        "compressed_completion_rate_ppm",
        profile.compressed_completion_rate_ppm,
    );
    push_usize(out, "max_queued_run", profile.max_queued_run);
    push_u128(out, "modeled_span_ns", profile.modeled_span_ns);
    out.push('}');
}

fn push_distribution(out: &mut String, name: &str, distribution: Distribution) {
    out.push_str(",\"");
    out.push_str(name);
    out.push_str("\":{");
    push_usize_first(out, "count", distribution.count);
    for (field, value) in [
        ("min", distribution.min),
        ("p01", distribution.p01),
        ("p05", distribution.p05),
        ("p50", distribution.p50),
        ("p95", distribution.p95),
        ("p99", distribution.p99),
        ("p999", distribution.p999),
        ("max", distribution.max),
        ("mean", distribution.mean),
        ("mad", distribution.mad),
        ("relative_mad_ppm", distribution.relative_mad_ppm),
    ] {
        push_u128(out, field, value);
    }
    out.push('}');
}

fn push_string(out: &mut String, name: &str, value: &str) {
    out.push_str(",\"");
    out.push_str(name);
    out.push_str("\":\"");
    out.push_str(value);
    out.push('"');
}

fn push_string_first(out: &mut String, name: &str, value: &str) {
    out.push('"');
    out.push_str(name);
    out.push_str("\":\"");
    out.push_str(value);
    out.push('"');
}

fn push_bool(out: &mut String, name: &str, value: bool) {
    out.push_str(",\"");
    out.push_str(name);
    out.push_str("\":");
    out.push_str(if value { "true" } else { "false" });
}

fn push_usize(out: &mut String, name: &str, value: usize) {
    push_u128(out, name, u128::try_from(value).unwrap_or(u128::MAX));
}

fn push_usize_first(out: &mut String, name: &str, value: usize) {
    out.push('"');
    out.push_str(name);
    out.push_str("\":");
    out.push_str(&value.to_string());
}

fn push_i32(out: &mut String, name: &str, value: i32) {
    out.push_str(",\"");
    out.push_str(name);
    out.push_str("\":");
    out.push_str(&value.to_string());
}

fn push_u128(out: &mut String, name: &str, value: u128) {
    out.push_str(",\"");
    out.push_str(name);
    out.push_str("\":");
    out.push_str(&value.to_string());
}

fn write_artifact(path: Option<&PathBuf>, artifact: &str) -> Result<(), String> {
    if let Some(path) = path {
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
    Ok(())
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
                println!("usage: r2b_join_arrival [--smoke|--full] [--output PATH]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let mut config = Config::defaults(mode);
    config.output = output;
    Ok(config)
}

fn image() -> Result<PlayBootstrapImage26_2, String> {
    Ok(PlayBootstrapImage26_2::new(
        CommandProjectionArtifact::new(command_key(), vec![16, 0xaa].into_boxed_slice())
            .map_err(|error| format!("command projection: {error:?}"))?,
        RecipeProjectionArtifact::new(recipe_key(), vec![0x85, 0x01, 0xbb].into_boxed_slice())
            .map_err(|error| format!("recipe projection: {error:?}"))?,
    ))
}

fn snapshot() -> FreshR2bBootstrapSnapshot<'static> {
    FreshR2bBootstrapSnapshot {
        command_key: command_key(),
        recipe_key: recipe_key(),
        login: FreshLoginPayload {
            player_id: 270,
            flags: FreshLoginFlags::SHOW_DEATH_SCREEN,
            levels: &LEVELS,
            max_players: 20,
            chunk_radius: 10,
            simulation_distance: 10,
            spawn: FreshCommonSpawnInfo {
                dimension_type_registry_id: 0,
                dimension: "minecraft:overworld",
                seed: 0,
                game_mode: BootstrapGameMode::Survival,
                previous_game_mode: None,
                is_debug: false,
                is_flat: false,
                portal_cooldown: 0,
                sea_level: 63,
            },
        },
        difficulty: ChangeDifficultyPayload {
            difficulty: Difficulty26_2::Normal,
            locked: false,
        },
        abilities: PlayerAbilitiesPayload {
            flags: PlayerAbilityFlags::NONE,
            flying_speed: 0.05,
            walking_speed: 0.1,
        },
        held_slot: HeldSlotPayload(0),
        permission_event: PermissionEntityEventPayload {
            entity_id: 270,
            event: PermissionLevelEvent::All,
        },
        recipe_settings: RecipeBookSettingsPayload {
            flags: RecipeBookSettingFlags::NONE,
        },
        teleport: TeleportDestination {
            x: 1.0,
            y: 64.0,
            z: 2.0,
            yaw: 0.0,
            pitch: 0.0,
        },
        server_data: None,
        existing_players: &[],
        joining_player: SELF,
        border: WorldBorderPayload {
            center_x: 0.0,
            center_z: 0.0,
            old_size: 60_000_000.0,
            new_size: 60_000_000.0,
            lerp_time: 0,
            absolute_max_size: 29_999_984,
            warning_blocks: 5,
            warning_time: 15,
        },
        clock: ClockFullSyncPayload {
            game_time: 42,
            updates: &CLOCKS,
        },
        spawn: DefaultSpawnPayload {
            dimension: "minecraft:overworld",
            x: 0,
            y: 64,
            z: 0,
            yaw: 0.0,
            pitch: 0.0,
        },
        weather: BootstrapWeather::Clear,
        ticking_state: TickingStatePayload {
            tick_rate: 20.0,
            is_frozen: false,
        },
        ticking_step: TickingStepPayload(0),
        inventory: FreshEmptyInventoryPayload {
            container_id: 0,
            state_id: 1,
            slot_count: 46,
        },
    }
}

fn configuration_bodies() -> Vec<Box<[u8]>> {
    CONFIGURATION_BODY_SIZES
        .into_iter()
        .enumerate()
        .map(|(index, size)| {
            let packet_id = match index {
                0 => 1,
                1 => 12,
                2 => 14,
                3..=31 => 7,
                32 => 13,
                33 => 3,
                _ => unreachable!("configuration body count is sealed"),
            };
            let mut body = vec![0_u8; size];
            body[0] = packet_id;
            body.into_boxed_slice()
        })
        .collect()
}

fn session_epoch() -> Result<ServerSessionEpoch, String> {
    ServerSessionEpoch::from_bytes([
        0x4d, 0x7f, 0x60, 0x4f, 0x19, 0x6a, 0x43, 0xb0, 0x89, 0x87, 0xf0, 0xb2, 0xa2, 0x7c, 0x26,
        0x63,
    ])
    .map_err(|error| format!("session epoch: {error:?}"))
}

fn login_client_chunk() -> Result<Vec<u8>, String> {
    let mut frames = Vec::new();

    let mut handshake = PacketWriter::new(64).map_err(|error| format!("handshake: {error:?}"))?;
    handshake
        .write_var_int(0)
        .map_err(|error| format!("handshake packet id: {error:?}"))?;
    handshake
        .write_var_int(776)
        .map_err(|error| format!("protocol: {error:?}"))?;
    handshake
        .write_string("localhost", 255)
        .map_err(|error| format!("server address: {error:?}"))?;
    handshake
        .write_u16(25_566)
        .map_err(|error| format!("server port: {error:?}"))?;
    handshake
        .write_var_int(2)
        .map_err(|error| format!("login intent: {error:?}"))?;
    frames.extend_from_slice(&frame(handshake.as_slice())?);

    let mut hello = PacketWriter::new(64).map_err(|error| format!("login hello: {error:?}"))?;
    hello
        .write_var_int(0)
        .map_err(|error| format!("login hello packet id: {error:?}"))?;
    hello
        .write_string("Stato16", 16)
        .map_err(|error| format!("player name: {error:?}"))?;
    hello
        .write_u64(0)
        .map_err(|error| format!("client UUID msb: {error:?}"))?;
    hello
        .write_u64(0)
        .map_err(|error| format!("client UUID lsb: {error:?}"))?;
    frames.extend_from_slice(&frame(hello.as_slice())?);
    frames.extend_from_slice(&frame(&[3])?);
    Ok(frames)
}

fn known_pack_chunk() -> Result<Vec<u8>, String> {
    let mut body = PacketWriter::new(64).map_err(|error| format!("known pack: {error:?}"))?;
    body.write_var_int(7)
        .map_err(|error| format!("known pack packet id: {error:?}"))?;
    body.write_var_int(1)
        .map_err(|error| format!("known pack count: {error:?}"))?;
    body.write_string("minecraft", 32_767)
        .map_err(|error| format!("known pack namespace: {error:?}"))?;
    body.write_string("core", 32_767)
        .map_err(|error| format!("known pack id: {error:?}"))?;
    body.write_string("26.2", 32_767)
        .map_err(|error| format!("known pack version: {error:?}"))?;
    frame(body.as_slice())
}

fn frame(body: &[u8]) -> Result<Vec<u8>, String> {
    let mut writer = PacketWriter::new(body.len() + 5)
        .map_err(|error| format!("frame writer: {error:?}"))?;
    writer
        .write_var_int(i32::try_from(body.len()).map_err(|_| "frame body too large".to_owned())?)
        .map_err(|error| format!("frame length: {error:?}"))?;
    writer
        .write_bytes(body)
        .map_err(|error| format!("frame body: {error:?}"))?;
    Ok(writer.into_bytes())
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
