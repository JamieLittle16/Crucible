//! Whole-plan black-box confirmation for the selected replay-free R2B bootstrap.
//!
//! The committed fixture is confirmation evidence only: source-backed SEM rules remain authoritative
//! for stage order and branch law. This test deliberately exercises the production-candidate
//! preparation path as one unit so packet IDs, shared projections, dynamic codecs, arena indexing and
//! semantic stage assembly cannot each be locally correct while composing to the wrong bootstrap.

use crucible_packet_core::PacketWriter;

use crate::r2b::{
    CommandPermissionProfile, CommandProjectionKey, PlayBootstrapImage26_2, ProjectionRevision,
    QualifiedProjectionArtifact, RecipeProjectionKey,
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
    BootstrapWeather, FreshR2bBootstrapSnapshot, PlayPacketIds, SELECTED_DYNAMIC_ARENA_CAPACITY,
    ServerDataProjection, ServerDataProjectionKey, TeleportDestination,
};
use crate::r2b_recipe::{RecipeBookSettingFlags, RecipeBookSettingsPayload};
use crate::r2b_spawn::DefaultSpawnPayload;
use crate::r2b_teleport::TeleportTransaction;

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
        total_ticks: 5_503,
        partial_tick: 0.0,
        rate: 1.0,
    },
    ClockUpdate {
        registry_id: 1,
        total_ticks: 5_503,
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

const FIXTURE: &str = include_str!(
    "../../../../../vanilla/fixtures/network/r2b-selected-profile-black-box-fixture-26.2.json"
);

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

fn selected_snapshot<'a>(
    status: &'a QualifiedProjectionArtifact<ServerDataProjectionKey>,
) -> FreshR2bBootstrapSnapshot<'a> {
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
                seed: 0x1439_77a8_ee42_e04a,
                game_mode: BootstrapGameMode::Survival,
                previous_game_mode: None,
                is_debug: false,
                is_flat: false,
                portal_cooldown: 0,
                sea_level: 63,
            },
        },
        difficulty: ChangeDifficultyPayload {
            difficulty: Difficulty26_2::Easy,
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
            x: 10.390_952_126_751_907,
            y: 84.0,
            z: -5.815_159_807_324_014,
            yaw: f32::from_bits(0xc2ee_3335),
            pitch: f32::from_bits(0x4190_0001),
        },
        server_data: Some(ServerDataProjection {
            artifact: status,
            requested: status_key(),
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
            warning_time: 300,
        },
        clock: ClockFullSyncPayload {
            game_time: 5_503,
            updates: &CLOCKS,
        },
        spawn: DefaultSpawnPayload {
            dimension: "minecraft:overworld",
            x: 0,
            y: 90,
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

#[test]
fn selected_prepared_plan_matches_every_committed_black_box_body() {
    let expected = fixture_bodies();
    assert_eq!(expected.len(), 20, "selected fixture body count drifted");

    let image = PlayBootstrapImage26_2::new(
        QualifiedProjectionArtifact::new(command_key(), expected[6].clone().into_boxed_slice())
            .expect("fixture command projection"),
        QualifiedProjectionArtifact::new(recipe_key(), expected[4].clone().into_boxed_slice())
            .expect("fixture recipe projection"),
    );
    let status =
        QualifiedProjectionArtifact::new(status_key(), expected[10].clone().into_boxed_slice())
            .expect("fixture status projection");

    let mut scratch = PacketWriter::new(4_096).expect("selected R2B scratch bound");
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        selected_snapshot(&status),
        &image,
        IDS,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )
    .expect("selected replay-free plan must prepare");

    assert!(scratch.is_empty());
    assert_eq!(plan.body_count(), expected.len());
    assert_eq!(teleport.awaiting().expect("initial teleport pending").id, 1);

    let mut observed = Vec::with_capacity(plan.body_count());
    for stage in 0..10 {
        let mut body = 0;
        loop {
            match plan.lookup(stage, body) {
                PreparedLookup::Body(bytes) => {
                    observed.push(bytes.to_vec());
                    body += 1;
                }
                PreparedLookup::StageComplete => break,
                PreparedLookup::Complete => panic!("plan completed before semantic stage {stage}"),
            }
        }
    }
    assert_eq!(plan.lookup(10, 0), PreparedLookup::Complete);

    assert_eq!(observed.len(), expected.len());
    for (index, (observed, expected)) in observed.iter().zip(&expected).enumerate() {
        assert_eq!(
            observed, expected,
            "selected black-box body {index} drifted"
        );
    }
}

fn fixture_bodies() -> Vec<Vec<u8>> {
    FIXTURE
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let value = line.strip_prefix("\"body_hex\": \"")?;
            let value = value
                .strip_suffix("\",")
                .expect("fixture body_hex line remains canonical JSON");
            Some(decode_hex(value))
        })
        .collect()
}

fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "hex fixture length must be even");
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]))
        .collect()
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("fixture contains non-hex byte"),
    }
}
