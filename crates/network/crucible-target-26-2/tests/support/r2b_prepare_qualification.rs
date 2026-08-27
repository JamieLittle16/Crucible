//! Qualification coverage for transactional replay-free R2B preparation.

use crucible_connection_core::{ConnectionBufferError, ConnectionLimits};
use crucible_connection_driver::{ConnectionDriver, DriverError};
use crucible_packet_core::PacketWriter;
use crucible_publication_core::{
    StagedPublicationCursor, StagedPublicationStep, publish_staged_plan_one,
};

use crate::r2b::{
    CommandPermissionProfile, CommandProjectionArtifact, CommandProjectionKey,
    PlayBootstrapImage26_2, ProjectionArtifactError, ProjectionRevision, RecipeProjectionArtifact,
    RecipeProjectionKey, ServerDataProjectionArtifact,
};
use crate::r2b_border::WorldBorderPayload;
use crate::r2b_clock::{ClockFullSyncPayload, ClockUpdate};
use crate::r2b_difficulty::{ChangeDifficultyPayload, Difficulty26_2};
use crate::r2b_dynamic::{
    HeldSlotPayload, PermissionEntityEventPayload, PermissionLevelEvent, PlayerAbilitiesPayload,
    PlayerAbilityFlags, TickingStatePayload, TickingStepPayload,
};
use crate::r2b_inventory::FreshEmptyInventoryPayload;
use crate::r2b_login::{
    BootstrapGameMode, FreshCommonSpawnInfo, FreshLoginFlags, FreshLoginPayload,
};
use crate::r2b_plan::{PreparedLookup, PreparedR2bPlan};
use crate::r2b_player_info::InitialPlayerInfoEntry;
use crate::r2b_prepare::{
    BootstrapWeather, FreshR2bBootstrapSnapshot, PrepareR2bError, SELECTED_DYNAMIC_ARENA_CAPACITY,
    ServerDataProjection, ServerDataProjectionKey, TeleportDestination,
};
use crate::r2b_recipe::{RecipeBookSettingFlags, RecipeBookSettingsPayload};
use crate::r2b_spawn::DefaultSpawnPayload;
use crate::r2b_teleport::TeleportTransaction;

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

const fn rev(byte: u8) -> ProjectionRevision {
    ProjectionRevision::new([byte; 32])
}

const fn command_key() -> CommandProjectionKey {
    CommandProjectionKey::new(
        rev(1),
        rev(2),
        rev(3),
        rev(4),
        CommandPermissionProfile::DefaultNonOperator,
    )
}

const fn recipe_key() -> RecipeProjectionKey {
    RecipeProjectionKey::new(rev(5), rev(6), rev(7), rev(8))
}

const fn status_key() -> ServerDataProjectionKey {
    ServerDataProjectionKey::new(rev(9), rev(10))
}

fn image() -> PlayBootstrapImage26_2 {
    PlayBootstrapImage26_2::new(
        CommandProjectionArtifact::new(command_key(), vec![16, 0xaa].into_boxed_slice())
            .expect("commands"),
        RecipeProjectionArtifact::new(recipe_key(), vec![0x85, 0x01, 0xbb].into_boxed_slice())
            .expect("recipes"),
    )
}

fn status_artifact() -> ServerDataProjectionArtifact {
    ServerDataProjectionArtifact::new(status_key(), vec![86, 0x00].into_boxed_slice())
        .expect("server data")
}

fn snapshot(
    server_data: Option<ServerDataProjection<'static>>,
    weather: BootstrapWeather,
) -> FreshR2bBootstrapSnapshot<'static> {
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
        server_data,
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
        weather,
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

fn body_id(body: &[u8]) -> i32 {
    let mut value = 0_u32;
    for (index, byte) in body.iter().copied().take(5).enumerate() {
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return i32::try_from(value).expect("selected packet ID fits i32");
        }
    }
    panic!("packet ID must be a bounded VarInt");
}

fn limits(max_body: usize, egress: usize) -> ConnectionLimits {
    ConnectionLimits::new(max_body, max_body + 5, egress)
        .expect("coherent prepared-publication limits")
}

#[test]
fn clear_status_route_has_exact_source_stage_order() {
    let status = Box::leak(Box::new(status_artifact()));
    let image = image();
    let mut scratch = PacketWriter::new(4096).expect("scratch");
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        snapshot(
            Some(ServerDataProjection {
                artifact: status,
                requested: status_key(),
            }),
            BootstrapWeather::Clear,
        ),
        &image,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .expect("selected bootstrap");

    assert!(scratch.is_empty());
    assert_eq!(plan.dynamic_body_count(), 17);
    assert_eq!(plan.body_count(), 20);
    assert_eq!(
        (0..10)
            .map(|stage| plan.stage_body_count(stage).expect("stage"))
            .collect::<Vec<_>>(),
        vec![1, 3, 1, 2, 2, 1, 1, 2, 6, 1]
    );

    let expected: &[&[i32]] = &[
        &[49],
        &[10, 64, 105],
        &[133],
        &[34, 16],
        &[76, 74],
        &[72],
        &[86],
        &[70, 70],
        &[43, 113, 97, 38, 127, 128],
        &[18],
    ];
    for (stage, ids) in expected.iter().copied().enumerate() {
        for (body, expected_id) in ids.iter().copied().enumerate() {
            let PreparedLookup::Body(bytes) = plan.lookup(stage, body) else {
                panic!("missing body {stage}/{body}");
            };
            assert_eq!(body_id(bytes), expected_id);
        }
        assert_eq!(plan.lookup(stage, ids.len()), PreparedLookup::StageComplete);
    }
    assert_eq!(plan.lookup(10, 0), PreparedLookup::Complete);
    assert_eq!(teleport.awaiting().expect("teleport committed").id, 1);
}

#[test]
fn clear_weather_and_absent_status_are_explicit_empty_branches() {
    let image = image();
    let mut scratch = PacketWriter::new(4096).expect("scratch");
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        snapshot(None, BootstrapWeather::Clear),
        &image,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .expect("bootstrap");

    assert_eq!(plan.body_count(), 19);
    assert_eq!(plan.stage_body_count(6), Some(0));
    assert_eq!(plan.lookup(6, 0), PreparedLookup::StageComplete);
}

#[test]
fn raining_route_adds_exact_three_events_before_load_start() {
    let image = image();
    let mut scratch = PacketWriter::new(4096).expect("scratch");
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        snapshot(
            None,
            BootstrapWeather::Raining {
                rain_level: 0.75,
                thunder_level: 0.25,
            },
        ),
        &image,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .expect("raining bootstrap");

    assert_eq!(plan.dynamic_body_count(), 20);
    assert_eq!(plan.stage_body_count(8), Some(9));
    let ids = (0..9)
        .map(|body| match plan.lookup(8, body) {
            PreparedLookup::Body(bytes) => body_id(bytes),
            other => panic!("unexpected lookup: {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, [43, 113, 97, 38, 38, 38, 38, 127, 128]);
}

#[test]
fn failure_does_not_commit_teleport_or_leave_scratch_dirty() {
    let image = image();
    let mut scratch = PacketWriter::new(64).expect("small scratch");
    let mut teleport = TeleportTransaction::new();

    let error = PreparedR2bPlan::prepare(
        snapshot(None, BootstrapWeather::Clear),
        &image,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .expect_err("Login cannot fit");

    assert!(matches!(error, PrepareR2bError::Login(_)));
    assert_eq!(teleport.awaiting(), None);
    assert!(scratch.is_empty());
}

#[test]
fn shared_packet_identity_is_rejected_before_artifact_publication() {
    assert_eq!(
        CommandProjectionArtifact::new(command_key(), vec![17, 0xaa].into_boxed_slice())
            .expect_err("mismatched command packet identity"),
        ProjectionArtifactError::PacketIdMismatch {
            expected: 16,
            actual: 17,
        }
    );
    assert_eq!(
        ServerDataProjectionArtifact::new(status_key(), vec![85, 0x00].into_boxed_slice())
            .expect_err("mismatched status packet identity"),
        ProjectionArtifactError::PacketIdMismatch {
            expected: 86,
            actual: 85,
        }
    );
}

#[test]
fn stale_status_revision_still_fails_closed_before_dynamic_work() {
    let mut scratch = PacketWriter::new(4096).expect("scratch");
    let mut teleport = TeleportTransaction::new();
    let status = Box::leak(Box::new(status_artifact()));
    let stale = ServerDataProjectionKey::new(rev(9), rev(11));
    let error = PreparedR2bPlan::prepare(
        snapshot(
            Some(ServerDataProjection {
                artifact: status,
                requested: stale,
            }),
            BootstrapWeather::Clear,
        ),
        &image(),
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .expect_err("stale status revision");
    assert_eq!(
        error,
        PrepareR2bError::Projection(ProjectionArtifactError::KeyMismatch)
    );
    assert!(scratch.is_empty());
    assert_eq!(teleport.awaiting(), None);
}

#[test]
fn target_packet_identity_is_not_a_runtime_input() {
    let image = image();
    let mut scratch = PacketWriter::new(4096).expect("scratch");
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        snapshot(None, BootstrapWeather::Clear),
        &image,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .expect("static target IDs prepare");

    let PreparedLookup::Body(login) = plan.lookup(0, 0) else {
        panic!("login body");
    };
    assert_eq!(body_id(login), 49);
    assert!(scratch.is_empty());
    assert_eq!(teleport.awaiting().expect("teleport committed").id, 1);
}

#[test]
fn prepared_plan_runs_through_target_neutral_staged_publication() {
    let image = image();
    let mut scratch = PacketWriter::new(4096).expect("scratch");
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        snapshot(None, BootstrapWeather::Clear),
        &image,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .expect("bootstrap");
    let mut cursor = StagedPublicationCursor::new();
    let mut driver = ConnectionDriver::new(limits(4096, 32_768));
    let mut queued = 0_usize;
    let mut stage_boundaries = 0_usize;

    for _ in 0..40 {
        match publish_staged_plan_one::<(), _>(&plan, &mut cursor, &mut driver)
            .expect("bounded publication")
        {
            StagedPublicationStep::Queued { .. } => queued += 1,
            StagedPublicationStep::StageComplete { .. } => stage_boundaries += 1,
            StagedPublicationStep::Complete => break,
        }
    }

    assert_eq!(queued, plan.body_count());
    assert_eq!(stage_boundaries, 10);
    assert_eq!(cursor.stage_index(), 10);
    assert_eq!(cursor.body_index(), 0);
    assert_eq!(
        publish_staged_plan_one::<(), _>(&plan, &mut cursor, &mut driver),
        Ok(StagedPublicationStep::Complete)
    );
    assert!(driver.queued_egress() > plan.dynamic_body_bytes());
}

#[test]
fn prepared_plan_backpressure_preserves_cursor_and_existing_egress() {
    let image = image();
    let mut scratch = PacketWriter::new(4096).expect("scratch");
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        snapshot(None, BootstrapWeather::Clear),
        &image,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .expect("bootstrap");

    let mut probe_cursor = StagedPublicationCursor::new();
    let mut probe = ConnectionDriver::new(limits(4096, 8192));
    publish_staged_plan_one::<(), _>(&plan, &mut probe_cursor, &mut probe)
        .expect("first body queues in probe");
    let first_frame_bytes = probe.queued_egress();
    assert!(first_frame_bytes > 0);

    let PreparedLookup::Body(first_body) = plan.lookup(0, 0) else {
        panic!("first prepared body");
    };
    let mut cursor = StagedPublicationCursor::new();
    let mut driver = ConnectionDriver::new(limits(first_body.len(), first_frame_bytes));
    assert!(matches!(
        publish_staged_plan_one::<(), _>(&plan, &mut cursor, &mut driver),
        Ok(StagedPublicationStep::Queued {
            stage: 0,
            index: 0,
            ..
        })
    ));
    assert_eq!(
        publish_staged_plan_one::<(), _>(&plan, &mut cursor, &mut driver),
        Ok(StagedPublicationStep::StageComplete { stage: 0 })
    );

    let cursor_before = cursor;
    let egress_before = driver.pending_egress().to_vec();
    let error = publish_staged_plan_one::<(), _>(&plan, &mut cursor, &mut driver)
        .expect_err("next body must observe bounded backpressure");
    assert!(matches!(
        error,
        DriverError::Buffer(ConnectionBufferError::EgressLimitExceeded { .. })
    ));
    assert_eq!(cursor, cursor_before);
    assert_eq!(driver.pending_egress(), egress_before);
}
