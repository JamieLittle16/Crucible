use std::io::{self, Read, Write};

use crucible_packet_core::PacketWriter;
use crucible_server::{
    R2bEntryOutcome, R2bServerError, ServerSessionEpoch, enter_r2b_play_blocking_transport,
};
use crucible_target_26_2::{
    Target26_2R1xContext,
    r2b::{
        BootstrapGameMode, BootstrapWeather, ChangeDifficultyPayload, ClockFullSyncPayload,
        ClockUpdate, CommandPermissionProfile, CommandProjectionKey, DefaultSpawnPayload,
        Difficulty26_2, FreshCommonSpawnInfo, FreshEmptyInventoryPayload, FreshLoginFlags,
        FreshLoginPayload, FreshR2bBootstrapSnapshot, HeldSlotPayload, InitialPlayerInfoEntry,
        PermissionEntityEventPayload, PermissionLevelEvent, PlayBootstrapImage26_2, PlayPacketIds,
        PlayerAbilitiesPayload, PlayerAbilityFlags, PreparedLookup, PreparedR2bPlan,
        ProjectionRevision, QualifiedProjectionArtifact, RecipeBookSettingFlags,
        RecipeBookSettingsPayload, RecipeProjectionKey, SELECTED_DYNAMIC_ARENA_CAPACITY,
        TeleportDestination, TeleportTransaction, TickingStatePayload, TickingStepPayload,
        WorldBorderPayload,
    },
};

const CONFIGURATION_BODY_SIZES: [usize; 34] = [
    25, 20, 22, 1_612, 224, 327, 227, 184, 149, 77, 80, 78, 233, 66, 66, 77, 70, 81, 73, 980, 282,
    116, 1_143, 1_036, 968, 416, 237, 48, 49, 94, 64, 103, 35_204, 1,
];

const IDS: PlayPacketIds = PlayPacketIds::from_source_order([
    10, 16, 18, 34, 38, 43, 49, 64, 70, 72, 74, 76, 86, 97, 105, 113, 127, 128, 133,
]);

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

#[derive(Debug)]
struct ScriptedTransport {
    reads: Vec<Vec<u8>>,
    next_read: usize,
    output: Vec<u8>,
}

impl ScriptedTransport {
    fn new(reads: Vec<Vec<u8>>) -> Self {
        Self {
            reads,
            next_read: 0,
            output: Vec::new(),
        }
    }
}

impl Read for ScriptedTransport {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        let Some(chunk) = self.reads.get(self.next_read) else {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        };
        assert!(
            chunk.len() <= destination.len(),
            "scripted client chunk must fit retained read scratch"
        );
        destination[..chunk.len()].copy_from_slice(chunk);
        self.next_read += 1;
        Ok(chunk.len())
    }
}

impl Write for ScriptedTransport {
    fn write(&mut self, source: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(source);
        Ok(source.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
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

fn image() -> PlayBootstrapImage26_2 {
    PlayBootstrapImage26_2::new(
        QualifiedProjectionArtifact::new(command_key(), vec![16, 0xaa].into_boxed_slice())
            .expect("command projection"),
        QualifiedProjectionArtifact::new(
            recipe_key(),
            vec![0x85, 0x01, 0xbb].into_boxed_slice(),
        )
        .expect("recipe projection"),
    )
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

fn empty_play_context(configuration: &[Box<[u8]>]) -> Target26_2R1xContext {
    Target26_2R1xContext::new(
        "{}".into(),
        configuration.iter().cloned().collect(),
        Vec::new(),
    )
    .expect("structurally sealed configuration-only image")
}

fn session_epoch() -> ServerSessionEpoch {
    ServerSessionEpoch::from_bytes([
        0x4d, 0x7f, 0x60, 0x4f, 0x19, 0x6a, 0x43, 0xb0, 0x89, 0x87, 0xf0, 0xb2, 0xa2, 0x7c, 0x26,
        0x63,
    ])
    .expect("captured session epoch is RFC-4122 v4")
}

fn login_client_chunk() -> Vec<u8> {
    let mut frames = Vec::new();

    let mut handshake = PacketWriter::new(64).expect("handshake body bound");
    handshake.write_var_int(0).expect("handshake packet id");
    handshake.write_var_int(776).expect("protocol");
    handshake.write_string("localhost", 255).expect("server address");
    handshake.write_u16(25_566).expect("server port");
    handshake.write_var_int(2).expect("login intent");
    frames.extend_from_slice(&frame(handshake.as_slice()));

    let mut hello = PacketWriter::new(64).expect("login hello body bound");
    hello.write_var_int(0).expect("login hello packet id");
    hello.write_string("Stato16", 16).expect("player name");
    hello.write_u64(0).expect("client UUID msb");
    hello.write_u64(0).expect("client UUID lsb");
    frames.extend_from_slice(&frame(hello.as_slice()));

    frames.extend_from_slice(&frame(&[3]));
    frames
}

fn known_pack_chunk() -> Vec<u8> {
    let mut body = PacketWriter::new(64).expect("known-pack body bound");
    body.write_var_int(7).expect("known-pack packet id");
    body.write_var_int(1).expect("one selected pack");
    body.write_string("minecraft", 32_767).expect("namespace");
    body.write_string("core", 32_767).expect("pack id");
    body.write_string("26.2", 32_767).expect("pack version");
    frame(body.as_slice())
}

fn finish_configuration_chunk() -> Vec<u8> {
    frame(&[3])
}

fn frame(body: &[u8]) -> Vec<u8> {
    let mut writer = PacketWriter::new(body.len() + 5).expect("framed body bound");
    writer
        .write_var_int(i32::try_from(body.len()).expect("test body length fits VarInt"))
        .expect("frame length");
    writer.write_bytes(body).expect("frame body");
    writer.into_bytes()
}

fn prepared_bodies(image: &PlayBootstrapImage26_2) -> Vec<Vec<u8>> {
    let mut scratch = PacketWriter::new(4 * 1_024).expect("prepare scratch");
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        snapshot(),
        image,
        IDS,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .expect("expected R2B plan");

    let mut bodies = Vec::with_capacity(plan.body_count());
    for stage in 0..10 {
        let mut body = 0;
        loop {
            match plan.lookup(stage, body) {
                PreparedLookup::Body(bytes) => {
                    bodies.push(bytes.to_vec());
                    body += 1;
                }
                PreparedLookup::StageComplete => break,
                PreparedLookup::Complete => panic!("network stage ended before stage boundary"),
            }
        }
    }
    assert_eq!(bodies.len(), plan.body_count());
    bodies
}

fn decode_frames(stream: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    let mut cursor = 0;
    while cursor < stream.len() {
        let (length, prefix) = decode_var_int(&stream[cursor..]);
        cursor += prefix;
        let length = usize::try_from(length).expect("non-negative frame length");
        let end = cursor + length;
        assert!(end <= stream.len(), "complete framed output");
        frames.push(&stream[cursor..end]);
        cursor = end;
    }
    frames
}

fn decode_var_int(bytes: &[u8]) -> (i32, usize) {
    let mut value = 0_u32;
    for (index, byte) in bytes.iter().copied().take(5).enumerate() {
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return (
                i32::try_from(value).expect("test frame length is non-negative"),
                index + 1,
            );
        }
    }
    panic!("bounded canonical VarInt frame length")
}

#[test]
fn configuration_only_r1x_hands_one_driver_to_exact_replay_free_r2b() {
    let configuration = configuration_bodies();
    let context = empty_play_context(&configuration);
    let image = image();
    let expected_r2b = prepared_bodies(&image);
    let mut transport = ScriptedTransport::new(vec![
        login_client_chunk(),
        known_pack_chunk(),
        finish_configuration_chunk(),
    ]);

    let outcome = enter_r2b_play_blocking_transport(
        &mut transport,
        session_epoch(),
        &context,
        &image,
        snapshot(),
        IDS,
    )
    .expect("configuration-only R2B entry succeeds");

    let R2bEntryOutcome::WorldProjectionReady(session) = outcome else {
        panic!("expected explicit world-projection handoff");
    };
    assert_eq!(transport.next_read, 3);
    assert_eq!(session.buffered_ingress(), 0);
    assert_eq!(session.queued_egress(), 0);
    assert_eq!(session.teleport_transaction().awaiting().expect("teleport pending").id, 1);

    let observed = decode_frames(&transport.output);
    assert_eq!(observed.len(), 1 + configuration.len() + expected_r2b.len());
    assert_eq!(observed[0][0], 2, "first server frame is LoginFinished");

    for (actual, expected) in observed[1..1 + configuration.len()]
        .iter()
        .zip(&configuration)
    {
        assert_eq!(*actual, expected.as_ref());
    }
    for (actual, expected) in observed[1 + configuration.len()..]
        .iter()
        .zip(&expected_r2b)
    {
        assert_eq!(*actual, expected.as_slice());
    }
}

#[test]
fn any_captured_play_body_is_rejected_before_transport_io() {
    let configuration = configuration_bodies();
    let context = Target26_2R1xContext::new(
        "{}".into(),
        configuration,
        vec![vec![1].into_boxed_slice()],
    )
    .expect("one structurally valid captured Play body");
    let image = image();
    let mut transport = ScriptedTransport::new(Vec::new());

    let error = enter_r2b_play_blocking_transport(
        &mut transport,
        session_epoch(),
        &context,
        &image,
        snapshot(),
        IDS,
    )
    .expect_err("R2B must reject captured Play before any I/O");

    assert!(matches!(
        error,
        R2bServerError::CapturedPlayNotEmpty {
            frames: 1,
            body_bytes: 1,
        }
    ));
    assert_eq!(transport.next_read, 0);
    assert!(transport.output.is_empty());
}
