use std::env;
use std::fs;
use std::hint::black_box;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Instant;

use crucible_benchmark_support::{
    DistributionStats, HardwareMetadata, collect_hardware_metadata, push_json_string,
};
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
        RecipeProjectionKey, ServerDataProjection, ServerDataProjectionArtifact,
        ServerDataProjectionKey, TeleportDestination, TickingStatePayload, TickingStepPayload,
        WorldBorderPayload,
    },
};

const SCHEMA: u32 = 1;
const CONFIGURATION_BODY_SIZES: [usize; 34] = [
    25, 20, 22, 1_612, 224, 327, 227, 184, 149, 77, 80, 78, 233, 66, 66, 77, 70, 81, 73, 980, 282,
    116, 1_143, 1_036, 968, 416, 237, 48, 49, 94, 64, 103, 35_204, 1,
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
    warmup_samples: usize,
    measured_samples: usize,
}

impl Config {
    const fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                warmup_samples: 256,
                measured_samples: 4_096,
            },
            Mode::Full => Self {
                mode,
                output: None,
                warmup_samples: 2_048,
                measured_samples: 32_768,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scenario {
    Core,
    WithServerData,
}

impl Scenario {
    const ALL: [Self; 2] = [Self::Core, Self::WithServerData];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::WithServerData => "with-server-data",
        }
    }

    const fn includes_server_data(self) -> bool {
        matches!(self, Self::WithServerData)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SemanticObservation {
    written_bytes: usize,
    write_calls: usize,
    teleport_id: u32,
    read_chunks: usize,
}

#[derive(Debug)]
struct WorkloadEvidence {
    scenario: Scenario,
    service_ns: Vec<u128>,
    stats: DistributionStats,
    semantic: SemanticObservation,
}

struct Fixture {
    context: Target26_2R1xContext,
    image: PlayBootstrapImage26_2,
    server_data: ServerDataProjectionArtifact,
    server_data_key: ServerDataProjectionKey,
    login: Vec<u8>,
    known_pack: Vec<u8>,
    finish_configuration: Vec<u8>,
}

impl Fixture {
    fn new() -> Result<Self, String> {
        let command_key = command_key();
        let recipe_key = recipe_key();
        let server_data_key = server_data_key();
        let configuration = configuration_bodies();
        let context = Target26_2R1xContext::new("{}".into(), configuration, Vec::new())
            .map_err(|error| format!("configuration-only context failed: {error:?}"))?;
        let image = PlayBootstrapImage26_2::new(
            CommandProjectionArtifact::new(command_key, vec![16, 0xaa].into_boxed_slice())
                .map_err(|error| format!("command projection failed: {error:?}"))?,
            RecipeProjectionArtifact::new(
                recipe_key,
                vec![0x85, 0x01, 0xbb].into_boxed_slice(),
            )
            .map_err(|error| format!("recipe projection failed: {error:?}"))?,
        );
        let server_data = ServerDataProjectionArtifact::new(
            server_data_key,
            vec![86, 0xcc].into_boxed_slice(),
        )
        .map_err(|error| format!("server-data projection failed: {error:?}"))?;

        Ok(Self {
            context,
            image,
            server_data,
            server_data_key,
            login: login_client_chunk()?,
            known_pack: known_pack_chunk()?,
            finish_configuration: finish_configuration_chunk()?,
        })
    }

    fn snapshot(&self, scenario: Scenario) -> FreshR2bBootstrapSnapshot<'_> {
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
            server_data: scenario.includes_server_data().then_some(ServerDataProjection {
                artifact: &self.server_data,
                requested: self.server_data_key,
            }),
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
    written_bytes: usize,
    write_calls: usize,
}

impl<'a> CountingTransport<'a> {
    const fn new(reads: [&'a [u8]; 3]) -> Self {
        Self {
            reads,
            next_read: 0,
            written_bytes: 0,
            write_calls: 0,
        }
    }
}

impl Read for CountingTransport<'_> {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        let Some(chunk) = self.reads.get(self.next_read) else {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        };
        if chunk.len() > destination.len() {
            return Err(io::Error::other(
                "scripted client chunk exceeds retained read scratch",
            ));
        }
        destination[..chunk.len()].copy_from_slice(chunk);
        self.next_read += 1;
        Ok(chunk.len())
    }
}

impl Write for CountingTransport<'_> {
    fn write(&mut self, source: &[u8]) -> io::Result<usize> {
        self.written_bytes = self
            .written_bytes
            .checked_add(source.len())
            .ok_or_else(|| io::Error::other("written-byte accounting overflow"))?;
        self.write_calls = self
            .write_calls
            .checked_add(1)
            .ok_or_else(|| io::Error::other("write-call accounting overflow"))?;
        black_box(source);
        Ok(source.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("R2B entry latency benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let hardware = collect_hardware_metadata()?;
    let fixture = Fixture::new()?;
    let mut workloads = Vec::with_capacity(Scenario::ALL.len());

    for scenario in Scenario::ALL {
        workloads.push(measure_workload(&config, &fixture, scenario)?);
    }

    let artifact = render_json(&config, &hardware, &workloads);
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

    for workload in &workloads {
        println!(
            "R2B entry {}: p50={}ns p95={}ns p99={}ns p999={}ns mad={}ns top1={}ns max={}ns",
            workload.scenario.as_str(),
            workload.stats.p50,
            workload.stats.p95,
            workload.stats.p99,
            workload.stats.p999,
            workload.stats.mad,
            workload.stats.top_1pct_mean,
            workload.stats.max,
        );
    }
    Ok(())
}

fn measure_workload(
    config: &Config,
    fixture: &Fixture,
    scenario: Scenario,
) -> Result<WorkloadEvidence, String> {
    let expected = run_one(fixture, scenario)?.1;
    for _ in 0..config.warmup_samples {
        let (_, semantic) = run_one(fixture, scenario)?;
        require_semantic(expected, semantic)?;
    }

    let mut service_ns = Vec::with_capacity(config.measured_samples);
    for _ in 0..config.measured_samples {
        let (elapsed, semantic) = run_one(fixture, scenario)?;
        require_semantic(expected, semantic)?;
        service_ns.push(elapsed);
    }
    let stats = DistributionStats::from_samples(&service_ns)?;
    Ok(WorkloadEvidence {
        scenario,
        service_ns,
        stats,
        semantic: expected,
    })
}

fn run_one(
    fixture: &Fixture,
    scenario: Scenario,
) -> Result<(u128, SemanticObservation), String> {
    let mut transport = fixture.transport();
    let start = Instant::now();
    let outcome = enter_r2b_play_blocking_transport(
        black_box(&mut transport),
        session_epoch(),
        black_box(&fixture.context),
        black_box(&fixture.image),
        fixture.snapshot(scenario),
    )
    .map_err(|error| format!("R2B entry failed: {error:?}"))?;
    let elapsed = start.elapsed().as_nanos();

    let R2bEntryOutcome::WorldProjectionReady(session) = outcome else {
        return Err("R2B entry did not reach WorldProjectionReady".to_owned());
    };
    let teleport_id = session
        .teleport_transaction()
        .awaiting()
        .ok_or_else(|| "R2B entry did not retain the initial teleport transaction".to_owned())?
        .id;
    if session.buffered_ingress() != 0 || session.queued_egress() != 0 {
        return Err("WorldProjectionReady retained userspace queue bytes".to_owned());
    }
    if session.read_scratch_bytes() != 16 * 1_024 {
        return Err("R2B entry changed the transferred read-scratch contract".to_owned());
    }
    black_box(session);

    Ok((
        elapsed,
        SemanticObservation {
            written_bytes: transport.written_bytes,
            write_calls: transport.write_calls,
            teleport_id,
            read_chunks: transport.next_read,
        },
    ))
}

fn require_semantic(
    expected: SemanticObservation,
    observed: SemanticObservation,
) -> Result<(), String> {
    if expected == observed {
        Ok(())
    } else {
        Err(format!(
            "R2B entry semantic observation drifted: expected={expected:?} observed={observed:?}"
        ))
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

fn login_client_chunk() -> Result<Vec<u8>, String> {
    let mut frames = Vec::new();
    let mut handshake = PacketWriter::new(64).map_err(codec_error)?;
    handshake.write_var_int(0).map_err(codec_error)?;
    handshake.write_var_int(776).map_err(codec_error)?;
    handshake.write_string("localhost", 255).map_err(codec_error)?;
    handshake.write_u16(25_566).map_err(codec_error)?;
    handshake.write_var_int(2).map_err(codec_error)?;
    frames.extend_from_slice(&frame(handshake.as_slice())?);

    let mut hello = PacketWriter::new(64).map_err(codec_error)?;
    hello.write_var_int(0).map_err(codec_error)?;
    hello.write_string("Stato16", 16).map_err(codec_error)?;
    hello.write_u64(0).map_err(codec_error)?;
    hello.write_u64(0).map_err(codec_error)?;
    frames.extend_from_slice(&frame(hello.as_slice())?);
    frames.extend_from_slice(&frame(&[3])?);
    Ok(frames)
}

fn known_pack_chunk() -> Result<Vec<u8>, String> {
    let mut body = PacketWriter::new(64).map_err(codec_error)?;
    body.write_var_int(7).map_err(codec_error)?;
    body.write_var_int(1).map_err(codec_error)?;
    body.write_string("minecraft", 32_767).map_err(codec_error)?;
    body.write_string("core", 32_767).map_err(codec_error)?;
    body.write_string("26.2", 32_767).map_err(codec_error)?;
    frame(body.as_slice())
}

fn finish_configuration_chunk() -> Result<Vec<u8>, String> {
    frame(&[3])
}

fn frame(body: &[u8]) -> Result<Vec<u8>, String> {
    let maximum = body
        .len()
        .checked_add(5)
        .ok_or_else(|| "framed-body bound overflow".to_owned())?;
    let mut writer = PacketWriter::new(maximum).map_err(codec_error)?;
    let body_len = i32::try_from(body.len()).map_err(|_| "body length does not fit VarInt")?;
    writer.write_var_int(body_len).map_err(codec_error)?;
    writer.write_bytes(body).map_err(codec_error)?;
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

const fn server_data_key() -> ServerDataProjectionKey {
    ServerDataProjectionKey::new(revision(9), revision(10))
}

fn session_epoch() -> ServerSessionEpoch {
    ServerSessionEpoch::from_bytes([
        0x4d, 0x7f, 0x60, 0x4f, 0x19, 0x6a, 0x43, 0xb0, 0x89, 0x87, 0xf0, 0xb2, 0xa2, 0x7c, 0x26,
        0x63,
    ])
    .expect("sealed benchmark session epoch is RFC-4122 v4")
}

fn render_json(
    config: &Config,
    hardware: &HardwareMetadata,
    workloads: &[WorkloadEvidence],
) -> String {
    let mut output = String::from("{");
    output.push_str("\"schema\":");
    output.push_str(&SCHEMA.to_string());
    output.push_str(",\"benchmark\":\"r2b-full-entry-service-time\"");
    output.push_str(",\"mode\":");
    push_json_string(&mut output, config.mode.as_str());
    output.push_str(",\"scope\":\"single-blocking-connection-service\"");
    output.push_str(",\"queueing_included\":false");
    output.push_str(",\"kernel_socket_io_included\":false");
    output.push_str(",\"transport_output_allocation_included\":false");
    output.push_str(",\"server_entry_allocations_included\":true");
    output.push_str(",\"warmup_samples\":");
    output.push_str(&config.warmup_samples.to_string());
    output.push_str(",\"measured_samples\":");
    output.push_str(&config.measured_samples.to_string());
    output.push_str(",\"hardware\":");
    output.push_str(&hardware.to_json());
    output.push_str(",\"workloads\":[");
    for (index, workload) in workloads.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        push_workload_json(&mut output, workload);
    }
    output.push_str("]}");
    output
}

fn push_workload_json(output: &mut String, workload: &WorkloadEvidence) {
    output.push('{');
    output.push_str("\"scenario\":");
    push_json_string(output, workload.scenario.as_str());
    output.push_str(",\"semantic\":{");
    push_usize_field(output, "written_bytes", workload.semantic.written_bytes, false);
    push_usize_field(output, "write_calls", workload.semantic.write_calls, true);
    push_u32_field(output, "teleport_id", workload.semantic.teleport_id, true);
    push_usize_field(output, "read_chunks", workload.semantic.read_chunks, true);
    output.push('}');
    output.push_str(",\"service_ns\":");
    push_distribution_json(output, &workload.stats);
    output.push_str(",\"samples_ns\":[");
    for (index, sample) in workload.service_ns.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(&sample.to_string());
    }
    output.push_str("]}");
}

fn push_distribution_json(output: &mut String, stats: &DistributionStats) {
    output.push('{');
    push_usize_field(output, "count", stats.count, false);
    push_u128_field(output, "min", stats.min, true);
    push_u128_field(output, "p50", stats.p50, true);
    push_u128_field(output, "p90", stats.p90, true);
    push_u128_field(output, "p95", stats.p95, true);
    push_u128_field(output, "p99", stats.p99, true);
    push_u128_field(output, "p999", stats.p999, true);
    push_u128_field(output, "max", stats.max, true);
    push_u128_field(output, "mean", stats.mean, true);
    push_u128_field(output, "mad", stats.mad, true);
    push_u128_field(output, "iqr", stats.iqr, true);
    push_u128_field(output, "top_1pct_mean", stats.top_1pct_mean, true);
    push_u128_field(output, "top_0_1pct_mean", stats.top_0_1pct_mean, true);
    push_u128_field(output, "relative_mad_ppm", stats.relative_mad_ppm, true);
    push_u128_field(output, "p99_to_p50_ppm", stats.p99_to_p50_ppm, true);
    push_u128_field(output, "p999_to_p50_ppm", stats.p999_to_p50_ppm, true);
    push_u128_field(output, "max_to_p50_ppm", stats.max_to_p50_ppm, true);
    output.push('}');
}

fn push_usize_field(output: &mut String, name: &str, value: usize, comma: bool) {
    if comma {
        output.push(',');
    }
    push_json_string(output, name);
    output.push(':');
    output.push_str(&value.to_string());
}

fn push_u32_field(output: &mut String, name: &str, value: u32, comma: bool) {
    if comma {
        output.push(',');
    }
    push_json_string(output, name);
    output.push(':');
    output.push_str(&value.to_string());
}

fn push_u128_field(output: &mut String, name: &str, value: u128, comma: bool) {
    if comma {
        output.push(',');
    }
    push_json_string(output, name);
    output.push(':');
    output.push_str(&value.to_string());
}

fn parse_args() -> Result<Config, String> {
    let mut mode = Mode::Smoke;
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--smoke" => mode = Mode::Smoke,
            "--full" => mode = Mode::Full,
            "--output" => {
                output = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--output requires a path".to_owned())?,
                ));
            }
            "--help" | "-h" => {
                println!("usage: r2b_entry_latency_bench [--smoke|--full] [--output PATH]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let mut config = Config::defaults(mode);
    config.output = output;
    Ok(config)
}

fn codec_error(error: impl core::fmt::Debug) -> String {
    format!("packet-codec failure: {error:?}")
}