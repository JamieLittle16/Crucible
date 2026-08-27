use std::env;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use crucible_packet_core::PacketWriter;
use crucible_target_26_2::r2b::{
    BootstrapGameMode, BootstrapWeather, ChangeDifficultyPayload, ClockFullSyncPayload,
    ClockUpdate, CommandPermissionProfile, CommandProjectionArtifact, CommandProjectionKey,
    DefaultSpawnPayload, Difficulty26_2, FreshCommonSpawnInfo, FreshEmptyInventoryPayload,
    FreshLoginFlags, FreshLoginPayload, FreshR2bBootstrapSnapshot, HeldSlotPayload,
    InitialPlayerInfoEntry, PermissionEntityEventPayload, PermissionLevelEvent,
    PlayBootstrapImage26_2, PlayerAbilitiesPayload, PlayerAbilityFlags, PreparedLookup,
    PreparedR2bPlan, ProjectionRevision, RecipeBookSettingFlags, RecipeBookSettingsPayload,
    RecipeProjectionArtifact, RecipeProjectionKey, SELECTED_DYNAMIC_ARENA_CAPACITY,
    ServerDataProjection, ServerDataProjectionArtifact, ServerDataProjectionKey,
    TeleportDestination, TeleportTransaction, TickingStatePayload, TickingStepPayload,
    WorldBorderPayload,
};

const SCHEMA: u32 = 1;
const SCRATCH_MAXIMUM: usize = 4 * 1_024;
const RESERVATIONS: [usize; 7] = [64, 128, 256, 512, 1_024, 2_048, 4_096];
const RATIO_SCALE_PPM: u128 = 1_000_000;
const CHECKSUM_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const CHECKSUM_PRIME: u64 = 0x0000_0100_0000_01b3;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Workload {
    FreshClear,
    PopulatedRaining,
}

impl Workload {
    const fn as_str(self) -> &'static str {
        match self {
            Self::FreshClear => "fresh-clear",
            Self::PopulatedRaining => "populated-raining",
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
                warmup_blocks: 32,
                measured_blocks: 512,
                joins_per_sample: 16,
                blocks_per_epoch: 64,
            },
            Mode::Full => Self {
                mode,
                output: None,
                warmup_blocks: 256,
                measured_blocks: 4_096,
                joins_per_sample: 32,
                blocks_per_epoch: 256,
            },
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.joins_per_sample == 0 || self.measured_blocks == 0 {
            return Err("measured work must be positive".to_owned());
        }
        if self.blocks_per_epoch == 0 || !self.measured_blocks.is_multiple_of(self.blocks_per_epoch)
        {
            return Err("measured blocks must divide into complete epochs".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Fixture {
    image: PlayBootstrapImage26_2,
    status: ServerDataProjectionArtifact,
    populated_players: Vec<InitialPlayerInfoEntry<'static>>,
}

impl Fixture {
    fn new() -> Result<Self, String> {
        let image = PlayBootstrapImage26_2::new(
            CommandProjectionArtifact::new(command_key(), vec![16, 0xaa].into_boxed_slice())
                .map_err(|error| format!("command artifact: {error:?}"))?,
            RecipeProjectionArtifact::new(recipe_key(), vec![0x85, 0x01, 0xbb].into_boxed_slice())
                .map_err(|error| format!("recipe artifact: {error:?}"))?,
        );
        let status =
            ServerDataProjectionArtifact::new(status_key(), vec![86, 0x00].into_boxed_slice())
                .map_err(|error| format!("status artifact: {error:?}"))?;
        Ok(Self {
            image,
            status,
            populated_players: vec![SELF; 64],
        })
    }

    fn snapshot(&self, workload: Workload) -> FreshR2bBootstrapSnapshot<'_> {
        let populated = workload == Workload::PopulatedRaining;
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
            server_data: populated.then_some(ServerDataProjection {
                artifact: &self.status,
                requested: status_key(),
            }),
            existing_players: if populated {
                &self.populated_players
            } else {
                &[]
            },
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
            weather: if populated {
                BootstrapWeather::Raining {
                    rain_level: 0.75,
                    thunder_level: 0.25,
                }
            } else {
                BootstrapWeather::Clear
            },
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Stats {
    count: usize,
    p50: u128,
    p95: u128,
    p99: u128,
    p999: u128,
    max: u128,
    mean: u128,
    mad: u128,
    relative_mad_ppm: u128,
}

impl Stats {
    fn from_samples(values: &[u128]) -> Result<Self, String> {
        if values.is_empty() {
            return Err("empty timing sample set".to_owned());
        }
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        let p50 = quantile(&sorted, 500)?;
        let p95 = quantile(&sorted, 950)?;
        let p99 = quantile(&sorted, 990)?;
        let p999 = quantile(&sorted, 999)?;
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
            p50,
            p95,
            p99,
            p999,
            max: sorted[sorted.len() - 1],
            mean,
            mad,
            relative_mad_ppm: ratio_ppm(mad, p50)?,
        })
    }
}

#[derive(Debug)]
struct PairSamples {
    baseline_ns: Vec<u128>,
    candidate_ns: Vec<u128>,
    baseline_block_ns: Vec<u128>,
    candidate_block_ns: Vec<u128>,
    block_ratio_ppm: Vec<u128>,
}

impl PairSamples {
    fn new(blocks: usize) -> Self {
        Self {
            baseline_ns: Vec::with_capacity(blocks * 2),
            candidate_ns: Vec::with_capacity(blocks * 2),
            baseline_block_ns: Vec::with_capacity(blocks),
            candidate_block_ns: Vec::with_capacity(blocks),
            block_ratio_ppm: Vec::with_capacity(blocks),
        }
    }
}

#[derive(Debug)]
struct Comparison {
    workload: Workload,
    reservation: usize,
    baseline: Stats,
    candidate: Stats,
    paired_ratio: Stats,
    epoch_ratio: Stats,
    epoch_ratios: Vec<u128>,
    candidate_faster_block_rate_ppm: u128,
    candidate_faster_epoch_rate_ppm: u128,
    semantic_checksum: u64,
    largest_body_bytes: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("R2B prepare scratch benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    config.validate()?;
    let fixture = Fixture::new()?;
    let mut comparisons = Vec::with_capacity(RESERVATIONS.len() * 2);
    for workload in [Workload::FreshClear, Workload::PopulatedRaining] {
        let (expected_checksum, largest_body_bytes) = semantic_digest(&fixture, workload, 0)?;
        for reservation in RESERVATIONS {
            let (candidate_checksum, candidate_largest) =
                semantic_digest(&fixture, workload, reservation)?;
            if candidate_checksum != expected_checksum || candidate_largest != largest_body_bytes {
                return Err(format!(
                    "semantic mismatch workload={} reservation={reservation}",
                    workload.as_str()
                ));
            }
            comparisons.push(measure_candidate(
                &config,
                &fixture,
                workload,
                reservation,
                expected_checksum,
                largest_body_bytes,
            )?);
        }
    }
    let artifact = render_json(&config, &comparisons);
    write_artifact(config.output.as_ref(), &artifact)?;
    for comparison in &comparisons {
        println!(
            "R2B scratch workload={} reserve={} paired_p50={}ppm epoch_p50={}ppm epoch_win={}ppm baseline_p99={}ns candidate_p99={}ns baseline_p999={}ns candidate_p999={}ns mad={}ppm",
            comparison.workload.as_str(),
            comparison.reservation,
            comparison.paired_ratio.p50,
            comparison.epoch_ratio.p50,
            comparison.candidate_faster_epoch_rate_ppm,
            comparison.baseline.p99,
            comparison.candidate.p99,
            comparison.baseline.p999,
            comparison.candidate.p999,
            comparison.candidate.relative_mad_ppm,
        );
    }
    Ok(())
}

fn measure_candidate(
    config: &Config,
    fixture: &Fixture,
    workload: Workload,
    reservation: usize,
    semantic_checksum: u64,
    largest_body_bytes: usize,
) -> Result<Comparison, String> {
    for block in 0..config.warmup_blocks {
        run_balanced_block(
            block,
            fixture,
            workload,
            reservation,
            config.joins_per_sample,
            None,
        )?;
    }
    let mut samples = PairSamples::new(config.measured_blocks);
    for block in 0..config.measured_blocks {
        run_balanced_block(
            block,
            fixture,
            workload,
            reservation,
            config.joins_per_sample,
            Some(&mut samples),
        )?;
    }
    let epoch_ratios = epoch_ratios(
        &samples.baseline_block_ns,
        &samples.candidate_block_ns,
        config.blocks_per_epoch,
    )?;
    let faster_blocks = samples
        .block_ratio_ppm
        .iter()
        .filter(|ratio| **ratio < RATIO_SCALE_PPM)
        .count();
    let faster_epochs = epoch_ratios
        .iter()
        .filter(|ratio| **ratio < RATIO_SCALE_PPM)
        .count();
    Ok(Comparison {
        workload,
        reservation,
        baseline: Stats::from_samples(&samples.baseline_ns)?,
        candidate: Stats::from_samples(&samples.candidate_ns)?,
        paired_ratio: Stats::from_samples(&samples.block_ratio_ppm)?,
        epoch_ratio: Stats::from_samples(&epoch_ratios)?,
        candidate_faster_block_rate_ppm: rate_ppm(faster_blocks, samples.block_ratio_ppm.len())?,
        candidate_faster_epoch_rate_ppm: rate_ppm(faster_epochs, epoch_ratios.len())?,
        epoch_ratios,
        semantic_checksum,
        largest_body_bytes,
    })
}

fn run_balanced_block(
    block: usize,
    fixture: &Fixture,
    workload: Workload,
    reservation: usize,
    joins: usize,
    mut output: Option<&mut PairSamples>,
) -> Result<(), String> {
    let order = if block.is_multiple_of(2) {
        [false, true, true, false]
    } else {
        [true, false, false, true]
    };
    let mut baseline_total = 0_u128;
    let mut candidate_total = 0_u128;
    for candidate in order {
        let selected_reservation = if candidate { reservation } else { 0 };
        let (elapsed, witness) =
            timed(|| timed_prepare_batch(fixture, workload, selected_reservation, joins))?;
        black_box(witness);
        if candidate {
            candidate_total = candidate_total
                .checked_add(elapsed)
                .ok_or_else(|| "candidate block timing overflow".to_owned())?;
            if let Some(samples) = output.as_deref_mut() {
                samples.candidate_ns.push(elapsed);
            }
        } else {
            baseline_total = baseline_total
                .checked_add(elapsed)
                .ok_or_else(|| "baseline block timing overflow".to_owned())?;
            if let Some(samples) = output.as_deref_mut() {
                samples.baseline_ns.push(elapsed);
            }
        }
    }
    if let Some(samples) = output {
        samples.baseline_block_ns.push(baseline_total);
        samples.candidate_block_ns.push(candidate_total);
        samples
            .block_ratio_ppm
            .push(ratio_ppm(candidate_total, baseline_total)?);
    }
    Ok(())
}

fn timed_prepare_batch(
    fixture: &Fixture,
    workload: Workload,
    reservation: usize,
    joins: usize,
) -> Result<usize, String> {
    let mut witness = 0_usize;
    for _ in 0..joins {
        let mut scratch = writer(reservation)?;
        let mut teleport = TeleportTransaction::new();
        let plan = PreparedR2bPlan::prepare(
            fixture.snapshot(workload),
            &fixture.image,
            &mut scratch,
            &mut teleport,
            SELECTED_DYNAMIC_ARENA_CAPACITY,
        )
        .map_err(|error| format!("prepare failed: {error:?}"))?;
        if !scratch.is_empty() {
            return Err("prepare left scratch dirty".to_owned());
        }
        witness = witness
            .wrapping_add(plan.body_count())
            .wrapping_add(plan.dynamic_body_count())
            .wrapping_add(plan.dynamic_body_bytes());
        black_box(plan);
        black_box(teleport);
    }
    Ok(black_box(witness))
}

fn semantic_digest(
    fixture: &Fixture,
    workload: Workload,
    reservation: usize,
) -> Result<(u64, usize), String> {
    let mut scratch = writer(reservation)?;
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        fixture.snapshot(workload),
        &fixture.image,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .map_err(|error| format!("semantic prepare failed: {error:?}"))?;
    let mut checksum = CHECKSUM_OFFSET;
    let mut largest = 0_usize;
    for stage in 0..10 {
        let count = plan
            .stage_body_count(stage)
            .ok_or_else(|| format!("missing stage {stage}"))?;
        for body_index in 0..count {
            let PreparedLookup::Body(body) = plan.lookup(stage, body_index) else {
                return Err(format!("missing body {stage}/{body_index}"));
            };
            largest = largest.max(body.len());
            checksum ^= u64::try_from(body.len()).unwrap_or(u64::MAX);
            checksum = checksum.wrapping_mul(CHECKSUM_PRIME);
            for byte in body {
                checksum ^= u64::from(*byte);
                checksum = checksum.wrapping_mul(CHECKSUM_PRIME);
            }
        }
    }
    Ok((checksum, largest))
}

fn writer(reservation: usize) -> Result<PacketWriter, String> {
    let result = if reservation == 0 {
        PacketWriter::new(SCRATCH_MAXIMUM)
    } else {
        PacketWriter::with_capacity(SCRATCH_MAXIMUM, reservation)
    };
    result.map_err(|error| format!("scratch writer: {error:?}"))
}

fn timed<T>(operation: impl FnOnce() -> Result<T, String>) -> Result<(u128, T), String> {
    let start = Instant::now();
    let value = black_box(operation()?);
    Ok((start.elapsed().as_nanos(), value))
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

fn epoch_ratios(
    baseline: &[u128],
    candidate: &[u128],
    blocks_per_epoch: usize,
) -> Result<Vec<u128>, String> {
    if baseline.len() != candidate.len()
        || blocks_per_epoch == 0
        || !baseline.len().is_multiple_of(blocks_per_epoch)
    {
        return Err("invalid epoch partition".to_owned());
    }
    let mut ratios = Vec::with_capacity(baseline.len() / blocks_per_epoch);
    for (baseline_epoch, candidate_epoch) in baseline
        .chunks_exact(blocks_per_epoch)
        .zip(candidate.chunks_exact(blocks_per_epoch))
    {
        ratios.push(ratio_ppm(
            checked_sum(candidate_epoch)?,
            checked_sum(baseline_epoch)?,
        )?);
    }
    Ok(ratios)
}

fn render_json(config: &Config, comparisons: &[Comparison]) -> String {
    let mut out = String::from("{");
    out.push_str("\"schema\":");
    out.push_str(&SCHEMA.to_string());
    out.push_str(",\"benchmark\":\"r2b-prepare-scratch-reservation\"");
    out.push_str(",\"mode\":\"");
    out.push_str(config.mode.as_str());
    out.push('"');
    out.push_str(",\"hosted_ci_is_diagnostic_only\":true");
    out.push_str(",\"scratch_maximum_bytes\":");
    out.push_str(&SCRATCH_MAXIMUM.to_string());
    out.push_str(",\"sampling\":{\"pattern\":\"balanced-abba-baab\"");
    push_usize(&mut out, "warmup_blocks", config.warmup_blocks);
    push_usize(&mut out, "measured_blocks", config.measured_blocks);
    push_usize(&mut out, "joins_per_sample", config.joins_per_sample);
    push_usize(&mut out, "blocks_per_epoch", config.blocks_per_epoch);
    out.push('}');
    out.push_str(",\"host\":{\"os\":\"");
    out.push_str(env::consts::OS);
    out.push_str("\",\"arch\":\"");
    out.push_str(env::consts::ARCH);
    out.push('"');
    if let Ok(cpu) = env::var("CRUCIBLE_BENCH_CPU") {
        out.push_str(",\"pinned_cpu\":\"");
        out.push_str(&cpu);
        out.push('"');
    }
    out.push_str("},\"comparisons\":[");
    for (index, comparison) in comparisons.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        push_comparison(&mut out, comparison);
    }
    out.push_str("]}");
    out
}

fn push_comparison(out: &mut String, comparison: &Comparison) {
    out.push_str("{\"workload\":\"");
    out.push_str(comparison.workload.as_str());
    out.push('"');
    push_usize(out, "reservation_bytes", comparison.reservation);
    push_usize(out, "largest_body_bytes", comparison.largest_body_bytes);
    out.push_str(",\"semantic_checksum\":");
    out.push_str(&comparison.semantic_checksum.to_string());
    push_stats(out, "baseline_ns", comparison.baseline);
    push_stats(out, "candidate_ns", comparison.candidate);
    push_stats(out, "paired_ratio_ppm", comparison.paired_ratio);
    push_stats(out, "epoch_ratio_ppm", comparison.epoch_ratio);
    out.push_str(",\"candidate_faster_block_rate_ppm\":");
    out.push_str(&comparison.candidate_faster_block_rate_ppm.to_string());
    out.push_str(",\"candidate_faster_epoch_rate_ppm\":");
    out.push_str(&comparison.candidate_faster_epoch_rate_ppm.to_string());
    out.push_str(",\"epoch_ratios\":[");
    for (index, ratio) in comparison.epoch_ratios.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str(&ratio.to_string());
    }
    out.push_str("]}");
}

fn push_stats(out: &mut String, name: &str, stats: Stats) {
    out.push_str(",\"");
    out.push_str(name);
    out.push_str("\":{\"count\":");
    out.push_str(&stats.count.to_string());
    for (field, value) in [
        ("p50", stats.p50),
        ("p95", stats.p95),
        ("p99", stats.p99),
        ("p999", stats.p999),
        ("max", stats.max),
        ("mean", stats.mean),
        ("mad", stats.mad),
        ("relative_mad_ppm", stats.relative_mad_ppm),
    ] {
        out.push_str(",\"");
        out.push_str(field);
        out.push_str("\":");
        out.push_str(&value.to_string());
    }
    out.push('}');
}

fn push_usize(out: &mut String, name: &str, value: usize) {
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
                println!("usage: r2b_prepare_scratch [--smoke|--full] [--output PATH]");
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
