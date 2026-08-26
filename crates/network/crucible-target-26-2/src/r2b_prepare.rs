//! Transactional semantic preparation for the finite replay-free R2B bootstrap.
//!
//! Dynamic bodies are encoded through one reused bounded `PacketWriter`, copied once into the
//! contiguous arena owned by [`PreparedR2bPlan`], and indexed in source-backed semantic stage order.
//! Commands, synchronized recipes and optional server data remain borrowed immutable artifacts.

use crucible_packet_core::{PacketCodecError, PacketWriter};

use crate::r2b::{
    CommandProjectionKey, PlayBootstrapImage26_2, ProjectionArtifactError, ProjectionRevision,
    QualifiedProjectionArtifact, RecipeProjectionKey,
};
use crate::r2b_arena::{DynamicBootstrapArena, DynamicBootstrapArenaError};
use crate::r2b_border::WorldBorderPayload;
use crate::r2b_clock::{ClockFullSyncPayload, ClockProjectionError};
use crate::r2b_difficulty::ChangeDifficultyPayload;
use crate::r2b_dynamic::{
    BootstrapGameEvent, GameEventPayload, HeldSlotPayload, PermissionEntityEventPayload,
    PlayerAbilitiesPayload, TickingStatePayload, TickingStepPayload,
};
use crate::r2b_inventory::{FreshEmptyInventoryPayload, InventoryEncodeError};
use crate::r2b_login::{FreshLoginPayload, LoginEncodeError};
use crate::r2b_plan::{
    MAX_DYNAMIC_BODIES, PlanBuildError, PreparedR2bPlan, PreparedR2bPlanBuilder, SharedBody,
};
use crate::r2b_player_info::{
    InitialPlayerInfoEntry, PlayerInfoEncodeError, encode_initial_player_info,
};
use crate::r2b_recipe::RecipeBookSettingsPayload;
use crate::r2b_recipe_add::FreshRecipeBookAddPayload;
use crate::r2b_spawn::DefaultSpawnPayload;
use crate::r2b_teleport::TeleportTransaction;

/// Selected/default dynamic arena reservation hint.
///
/// This is a capacity hint, not a semantic limit; larger admitted player/clock state may grow the
/// same single arena owner.
pub const SELECTED_DYNAMIC_ARENA_CAPACITY: usize = 512;

/// Exact finite 26.2 Play packet identities consumed by the assembler.
///
/// Production construction belongs to generated protocol-contract code. The preparation mechanism
/// itself is therefore independent of a runtime packet-name registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlayPacketIds([i32; 19]);

impl PlayPacketIds {
    /// Builds IDs in the frozen Play-entry contract order.
    #[must_use]
    pub const fn from_source_order(values: [i32; 19]) -> Self {
        Self(values)
    }

    const fn id(self, index: usize) -> i32 {
        self.0[index]
    }
}

/// Initial absolute teleport destination before assigning the connection-owned sequence ID.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TeleportDestination {
    /// Absolute X coordinate.
    pub x: f64,
    /// Absolute Y coordinate.
    pub y: f64,
    /// Absolute Z coordinate.
    pub z: f64,
    /// Absolute yaw.
    pub yaw: f32,
    /// Absolute pitch.
    pub pitch: f32,
}

/// Source-admitted optional weather branch.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BootstrapWeather {
    /// No weather packets.
    #[default]
    Clear,
    /// Start rain, then publish rain and thunder levels.
    Raining {
        /// Current rain level.
        rain_level: f32,
        /// Current thunder level.
        thunder_level: f32,
    },
}

/// Exact invalidation key for an immutable optional server-data projection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ServerDataProjectionKey {
    protocol_contract: ProjectionRevision,
    status_revision: ProjectionRevision,
}

impl ServerDataProjectionKey {
    /// Creates a complete protocol/status revision identity.
    #[must_use]
    pub const fn new(
        protocol_contract: ProjectionRevision,
        status_revision: ProjectionRevision,
    ) -> Self {
        Self {
            protocol_contract,
            status_revision,
        }
    }
}

/// Requested immutable server-data artifact for the current join.
#[derive(Clone, Copy, Debug)]
pub struct ServerDataProjection<'a> {
    /// Cached status body qualified for an exact revision key.
    pub artifact: &'a QualifiedProjectionArtifact<ServerDataProjectionKey>,
    /// Status revision requested by the semantic/status owner.
    pub requested: ServerDataProjectionKey,
}

/// Complete compact semantic snapshot for the finite network-owned R2B route.
#[derive(Clone, Copy, Debug)]
pub struct FreshR2bBootstrapSnapshot<'a> {
    /// Exact command projection key.
    pub command_key: CommandProjectionKey,
    /// Exact synchronized-recipe projection key.
    pub recipe_key: RecipeProjectionKey,
    /// Direct Login projection.
    pub login: FreshLoginPayload<'a>,
    /// Initial difficulty state.
    pub difficulty: ChangeDifficultyPayload,
    /// Initial player abilities.
    pub abilities: PlayerAbilitiesPayload,
    /// Selected hotbar slot.
    pub held_slot: HeldSlotPayload,
    /// Permission-level entity event.
    pub permission_event: PermissionEntityEventPayload,
    /// Per-player recipe-book settings.
    pub recipe_settings: RecipeBookSettingsPayload,
    /// Absolute teleport destination before sequence-ID assignment.
    pub teleport: TeleportDestination,
    /// Optional status projection.
    pub server_data: Option<ServerDataProjection<'a>>,
    /// Existing players visible before the joining player is inserted.
    pub existing_players: &'a [InitialPlayerInfoEntry<'a>],
    /// Joining player's initialization entry.
    pub joining_player: InitialPlayerInfoEntry<'a>,
    /// Initial world-border projection.
    pub border: WorldBorderPayload,
    /// Full clock synchronization.
    pub clock: ClockFullSyncPayload<'a>,
    /// Default spawn projection.
    pub spawn: DefaultSpawnPayload<'a>,
    /// Conditional weather state.
    pub weather: BootstrapWeather,
    /// Tick-rate state.
    pub ticking_state: TickingStatePayload,
    /// Remaining frozen tick steps.
    pub ticking_step: TickingStepPayload,
    /// Fresh empty inventory snapshot.
    pub inventory: FreshEmptyInventoryPayload,
}

/// Fail-closed preparation error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrepareR2bError {
    /// Packet-core write/preflight failure.
    Codec(PacketCodecError),
    /// Immutable shared projection mismatch.
    Projection(ProjectionArtifactError),
    /// Dynamic arena construction failure.
    Arena(DynamicBootstrapArenaError),
    /// Inline plan construction failure.
    Plan(PlanBuildError),
    /// Login projection failure.
    Login(LoginEncodeError),
    /// Clock projection failure.
    Clock(ClockProjectionError),
    /// Player-info projection failure.
    PlayerInfo(PlayerInfoEncodeError),
    /// Inventory projection failure.
    Inventory(InventoryEncodeError),
    /// A borrowed/shared full body carries the wrong packet identity.
    PacketIdMismatch {
        /// Expected packet ID.
        expected: i32,
        /// Decoded packet ID.
        actual: i32,
    },
    /// A borrowed/shared body has no canonical non-negative packet-ID `VarInt`.
    InvalidPacketBodyIdentity,
}

impl From<PacketCodecError> for PrepareR2bError {
    fn from(value: PacketCodecError) -> Self {
        Self::Codec(value)
    }
}
impl From<ProjectionArtifactError> for PrepareR2bError {
    fn from(value: ProjectionArtifactError) -> Self {
        Self::Projection(value)
    }
}
impl From<DynamicBootstrapArenaError> for PrepareR2bError {
    fn from(value: DynamicBootstrapArenaError) -> Self {
        Self::Arena(value)
    }
}
impl From<PlanBuildError> for PrepareR2bError {
    fn from(value: PlanBuildError) -> Self {
        Self::Plan(value)
    }
}
impl From<LoginEncodeError> for PrepareR2bError {
    fn from(value: LoginEncodeError) -> Self {
        Self::Login(value)
    }
}
impl From<ClockProjectionError> for PrepareR2bError {
    fn from(value: ClockProjectionError) -> Self {
        Self::Clock(value)
    }
}
impl From<PlayerInfoEncodeError> for PrepareR2bError {
    fn from(value: PlayerInfoEncodeError) -> Self {
        Self::PlayerInfo(value)
    }
}
impl From<InventoryEncodeError> for PrepareR2bError {
    fn from(value: InventoryEncodeError) -> Self {
        Self::Inventory(value)
    }
}

impl<'a> PreparedR2bPlan<'a> {
    /// Prepares the complete finite network-owned bootstrap transactionally.
    ///
    /// `scratch` is reused across all dynamic packets. Teleport sequence/awaiting state is advanced
    /// on a local copy and committed only after the entire plan succeeds.
    ///
    /// # Errors
    ///
    /// Fails closed on projection-key mismatch, invalid shared body identity, semantic codec failure,
    /// arena overflow or inline plan overflow. The caller's teleport state remains unchanged and
    /// `scratch` is reset on every error path.
    pub fn prepare(
        snapshot: FreshR2bBootstrapSnapshot<'a>,
        image: &'a PlayBootstrapImage26_2,
        ids: PlayPacketIds,
        scratch: &mut PacketWriter,
        teleport_state: &mut TeleportTransaction,
        arena_capacity: usize,
    ) -> Result<Self, PrepareR2bError> {
        scratch.reset();

        let commands = image.commands(&snapshot.command_key)?;
        let update_recipes = image.update_recipes(&snapshot.recipe_key)?;
        validate_packet_id(commands, ids.id(1))?;
        validate_packet_id(update_recipes, ids.id(18))?;

        let server_data = match snapshot.server_data {
            Some(projection) => {
                let body = projection.artifact.body_for(&projection.requested)?;
                validate_packet_id(body, ids.id(12))?;
                Some(body)
            }
            None => None,
        };

        let mut pending_teleport = *teleport_state;
        let teleport = pending_teleport.issue(
            snapshot.teleport.x,
            snapshot.teleport.y,
            snapshot.teleport.z,
            snapshot.teleport.yaw,
            snapshot.teleport.pitch,
        );

        let mut plan = PreparedR2bPlanBuilder::new(
            DynamicBootstrapArena::<MAX_DYNAMIC_BODIES>::with_capacity(arena_capacity),
            commands,
            update_recipes,
            server_data,
        );

        let result = prepare_stages(&mut plan, snapshot, ids, teleport, server_data, scratch);
        if let Err(error) = result {
            scratch.reset();
            return Err(error);
        }

        scratch.reset();
        let prepared = plan.finish();
        *teleport_state = pending_teleport;
        Ok(prepared)
    }
}

fn prepare_stages(
    plan: &mut PreparedR2bPlanBuilder<'_>,
    snapshot: FreshR2bBootstrapSnapshot<'_>,
    ids: PlayPacketIds,
    teleport: crate::r2b_teleport::AbsoluteTeleportPayload,
    server_data: Option<&[u8]>,
    scratch: &mut PacketWriter,
) -> Result<(), PrepareR2bError> {
    let start = plan.len();
    dynamic(plan, ids.id(6), scratch, |writer| {
        snapshot.login.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(0, start)?;

    let start = plan.len();
    dynamic(plan, ids.id(0), scratch, |writer| {
        snapshot.difficulty.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.id(7), scratch, |writer| {
        snapshot.abilities.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.id(14), scratch, |writer| {
        snapshot.held_slot.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(1, start)?;

    let start = plan.len();
    plan.push_shared(SharedBody::UpdateRecipes)?;
    plan.finish_stage(2, start)?;

    let start = plan.len();
    dynamic(plan, ids.id(3), scratch, |writer| {
        snapshot.permission_event.encode(writer).map_err(Into::into)
    })?;
    plan.push_shared(SharedBody::Commands)?;
    plan.finish_stage(3, start)?;

    let start = plan.len();
    dynamic(plan, ids.id(11), scratch, |writer| {
        snapshot.recipe_settings.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.id(10), scratch, |writer| {
        FreshRecipeBookAddPayload.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(4, start)?;

    let start = plan.len();
    dynamic(plan, ids.id(9), scratch, |writer| {
        teleport.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(5, start)?;

    let start = plan.len();
    if server_data.is_some() {
        plan.push_shared(SharedBody::ServerData)?;
    }
    plan.finish_stage(6, start)?;

    let start = plan.len();
    dynamic(plan, ids.id(8), scratch, |writer| {
        encode_initial_player_info(snapshot.existing_players, writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.id(8), scratch, |writer| {
        encode_initial_player_info(core::slice::from_ref(&snapshot.joining_player), writer)
            .map_err(Into::into)
    })?;
    plan.finish_stage(7, start)?;

    prepare_level_stage(plan, snapshot, ids, scratch)?;

    let start = plan.len();
    dynamic(plan, ids.id(2), scratch, |writer| {
        snapshot.inventory.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(9, start)?;
    Ok(())
}

fn prepare_level_stage(
    plan: &mut PreparedR2bPlanBuilder<'_>,
    snapshot: FreshR2bBootstrapSnapshot<'_>,
    ids: PlayPacketIds,
    scratch: &mut PacketWriter,
) -> Result<(), PrepareR2bError> {
    let start = plan.len();
    dynamic(plan, ids.id(5), scratch, |writer| {
        snapshot.border.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.id(15), scratch, |writer| {
        snapshot.clock.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.id(13), scratch, |writer| {
        snapshot.spawn.encode(writer).map_err(Into::into)
    })?;

    if let BootstrapWeather::Raining {
        rain_level,
        thunder_level,
    } = snapshot.weather
    {
        weather_event(plan, ids.id(4), scratch, BootstrapGameEvent::StartRaining, 0.0)?;
        weather_event(
            plan,
            ids.id(4),
            scratch,
            BootstrapGameEvent::RainLevelChange,
            rain_level,
        )?;
        weather_event(
            plan,
            ids.id(4),
            scratch,
            BootstrapGameEvent::ThunderLevelChange,
            thunder_level,
        )?;
    }

    weather_event(
        plan,
        ids.id(4),
        scratch,
        BootstrapGameEvent::LevelChunksLoadStart,
        0.0,
    )?;
    dynamic(plan, ids.id(16), scratch, |writer| {
        snapshot.ticking_state.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.id(17), scratch, |writer| {
        snapshot.ticking_step.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(8, start)?;
    Ok(())
}

fn dynamic<F>(
    plan: &mut PreparedR2bPlanBuilder<'_>,
    packet_id: i32,
    scratch: &mut PacketWriter,
    encode: F,
) -> Result<(), PrepareR2bError>
where
    F: FnOnce(&mut PacketWriter) -> Result<(), PrepareR2bError>,
{
    scratch.reset();
    if let Err(error) = scratch.write_var_int(packet_id) {
        scratch.reset();
        return Err(error.into());
    }
    if let Err(error) = encode(scratch) {
        scratch.reset();
        return Err(error);
    }
    let index = match plan.arena_mut().seal_from(scratch) {
        Ok(index) => index,
        Err(error) => {
            scratch.reset();
            return Err(error.into());
        }
    };
    plan.push_arena(index)?;
    Ok(())
}

fn weather_event(
    plan: &mut PreparedR2bPlanBuilder<'_>,
    packet_id: i32,
    scratch: &mut PacketWriter,
    event: BootstrapGameEvent,
    parameter: f32,
) -> Result<(), PrepareR2bError> {
    dynamic(plan, packet_id, scratch, |writer| {
        GameEventPayload { event, parameter }
            .encode(writer)
            .map_err(Into::into)
    })
}

fn validate_packet_id(body: &[u8], expected: i32) -> Result<(), PrepareR2bError> {
    let actual =
        decode_nonnegative_var_int(body).ok_or(PrepareR2bError::InvalidPacketBodyIdentity)?;
    if actual != expected {
        return Err(PrepareR2bError::PacketIdMismatch { expected, actual });
    }
    Ok(())
}

fn decode_nonnegative_var_int(body: &[u8]) -> Option<i32> {
    let mut value = 0_u32;
    for (index, byte) in body.iter().copied().take(5).enumerate() {
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            if value > i32::MAX.cast_unsigned() {
                return None;
            }
            let value = i32::try_from(value).ok()?;
            if var_int_len(value) != index + 1 {
                return None;
            }
            return Some(value);
        }
    }
    None
}

const fn var_int_len(value: i32) -> usize {
    let mut remaining = value.cast_unsigned();
    let mut length = 1_usize;
    while remaining & !0x7f != 0 {
        remaining >>= 7;
        length += 1;
    }
    length
}

#[cfg(test)]
mod tests {
    use crucible_packet_core::PacketWriter;

    use super::{
        BootstrapWeather, FreshR2bBootstrapSnapshot, PlayPacketIds,
        SELECTED_DYNAMIC_ARENA_CAPACITY, ServerDataProjection, ServerDataProjectionKey,
        TeleportDestination,
    };
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
            0x68, 0x20, 0x14, 0xfe, 0xad, 0x63, 0x36, 0x99, 0xaa, 0xda, 0x79, 0xaa, 0x08, 0xd9,
            0x5b, 0x45,
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
            QualifiedProjectionArtifact::new(command_key(), vec![16, 0xaa].into_boxed_slice())
                .expect("commands"),
            QualifiedProjectionArtifact::new(
                recipe_key(),
                vec![0x85, 0x01, 0xbb].into_boxed_slice(),
            )
            .expect("recipes"),
        )
    }

    fn status_artifact() -> QualifiedProjectionArtifact<ServerDataProjectionKey> {
        QualifiedProjectionArtifact::new(status_key(), vec![86, 0x00].into_boxed_slice())
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
        super::decode_nonnegative_var_int(body).expect("packet id")
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
            IDS,
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
            IDS,
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
            IDS,
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
            IDS,
            &mut scratch,
            &mut teleport,
            SELECTED_DYNAMIC_ARENA_CAPACITY,
        )
        .expect_err("Login cannot fit");

        assert!(matches!(error, super::PrepareR2bError::Login(_)));
        assert_eq!(teleport.awaiting(), None);
        assert!(scratch.is_empty());
    }

    #[test]
    fn shared_packet_identity_and_status_revision_fail_closed_before_dynamic_work() {
        let bad_image = PlayBootstrapImage26_2::new(
            QualifiedProjectionArtifact::new(command_key(), vec![17, 0xaa].into_boxed_slice())
                .expect("commands"),
            QualifiedProjectionArtifact::new(
                recipe_key(),
                vec![0x85, 0x01, 0xbb].into_boxed_slice(),
            )
            .expect("recipes"),
        );
        let mut scratch = PacketWriter::new(4096).expect("scratch");
        let mut teleport = TeleportTransaction::new();

        let error = PreparedR2bPlan::prepare(
            snapshot(None, BootstrapWeather::Clear),
            &bad_image,
            IDS,
            &mut scratch,
            &mut teleport,
            SELECTED_DYNAMIC_ARENA_CAPACITY,
        )
        .expect_err("mismatched command packet identity");
        assert_eq!(
            error,
            super::PrepareR2bError::PacketIdMismatch {
                expected: 16,
                actual: 17,
            }
        );
        assert!(scratch.is_empty());
        assert_eq!(teleport.awaiting(), None);

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
            &image,
            IDS,
            &mut scratch,
            &mut teleport,
            SELECTED_DYNAMIC_ARENA_CAPACITY,
        )
        .expect_err("stale status revision");
        assert_eq!(
            error,
            super::PrepareR2bError::Projection(
                crate::r2b::ProjectionArtifactError::KeyMismatch
            )
        );
    }
}
