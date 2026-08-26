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
use crate::r2b_teleport::{AbsoluteTeleportPayload, TeleportTransaction};

/// Selected/default dynamic arena reservation hint.
///
/// This is a capacity hint, not a semantic limit; larger admitted player/clock state may grow the
/// same single arena owner.
pub const SELECTED_DYNAMIC_ARENA_CAPACITY: usize = 512;

/// Exact finite 26.2 Play packet identities consumed by the assembler.
///
/// Production construction belongs to generated protocol-contract code. Named fields make semantic
/// assembly independent of runtime packet-name lookup and prevent index arithmetic on the hot path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlayPacketIds {
    change_difficulty: i32,
    commands: i32,
    container_set_content: i32,
    entity_event: i32,
    game_event: i32,
    initialize_border: i32,
    login: i32,
    player_abilities: i32,
    player_info_update: i32,
    player_position: i32,
    recipe_book_add: i32,
    recipe_book_settings: i32,
    server_data: i32,
    set_default_spawn_position: i32,
    set_held_slot: i32,
    set_time: i32,
    ticking_state: i32,
    ticking_step: i32,
    update_recipes: i32,
}

impl PlayPacketIds {
    /// Builds IDs in the frozen Play-entry contract order.
    #[must_use]
    pub const fn from_source_order(values: [i32; 19]) -> Self {
        Self {
            change_difficulty: values[0],
            commands: values[1],
            container_set_content: values[2],
            entity_event: values[3],
            game_event: values[4],
            initialize_border: values[5],
            login: values[6],
            player_abilities: values[7],
            player_info_update: values[8],
            player_position: values[9],
            recipe_book_add: values[10],
            recipe_book_settings: values[11],
            server_data: values[12],
            set_default_spawn_position: values[13],
            set_held_slot: values[14],
            set_time: values[15],
            ticking_state: values[16],
            ticking_step: values[17],
            update_recipes: values[18],
        }
    }

    const fn all_nonnegative(self) -> bool {
        self.change_difficulty >= 0
            && self.commands >= 0
            && self.container_set_content >= 0
            && self.entity_event >= 0
            && self.game_event >= 0
            && self.initialize_border >= 0
            && self.login >= 0
            && self.player_abilities >= 0
            && self.player_info_update >= 0
            && self.player_position >= 0
            && self.recipe_book_add >= 0
            && self.recipe_book_settings >= 0
            && self.server_data >= 0
            && self.set_default_spawn_position >= 0
            && self.set_held_slot >= 0
            && self.set_time >= 0
            && self.ticking_state >= 0
            && self.ticking_step >= 0
            && self.update_recipes >= 0
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
    /// A packet ID supplied by the static contract was negative.
    InvalidPacketIds,
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
    /// Fails closed on invalid/static packet IDs, projection-key mismatch, invalid shared body
    /// identity, semantic codec failure, arena overflow or inline plan overflow. The caller's
    /// teleport state remains unchanged and `scratch` is reset on every error path.
    pub fn prepare(
        snapshot: FreshR2bBootstrapSnapshot<'a>,
        image: &'a PlayBootstrapImage26_2,
        ids: PlayPacketIds,
        scratch: &mut PacketWriter,
        teleport_state: &mut TeleportTransaction,
        arena_capacity: usize,
    ) -> Result<Self, PrepareR2bError> {
        scratch.reset();
        if !ids.all_nonnegative() {
            return Err(PrepareR2bError::InvalidPacketIds);
        }

        let commands = image.commands(&snapshot.command_key)?;
        let update_recipes = image.update_recipes(&snapshot.recipe_key)?;
        validate_packet_id(commands, ids.commands)?;
        validate_packet_id(update_recipes, ids.update_recipes)?;

        let server_data = match snapshot.server_data {
            Some(projection) => {
                let body = projection.artifact.body_for(&projection.requested)?;
                validate_packet_id(body, ids.server_data)?;
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

        if let Err(error) = prepare_stages(&mut plan, snapshot, ids, teleport, server_data, scratch) {
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
    teleport: AbsoluteTeleportPayload,
    server_data: Option<&[u8]>,
    scratch: &mut PacketWriter,
) -> Result<(), PrepareR2bError> {
    let start = plan.len();
    dynamic(plan, ids.login, scratch, |writer| {
        snapshot.login.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(0, start)?;

    let start = plan.len();
    dynamic(plan, ids.change_difficulty, scratch, |writer| {
        snapshot.difficulty.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.player_abilities, scratch, |writer| {
        snapshot.abilities.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.set_held_slot, scratch, |writer| {
        snapshot.held_slot.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(1, start)?;

    let start = plan.len();
    plan.push_shared(SharedBody::UpdateRecipes)?;
    plan.finish_stage(2, start)?;

    let start = plan.len();
    dynamic(plan, ids.entity_event, scratch, |writer| {
        snapshot.permission_event.encode(writer).map_err(Into::into)
    })?;
    plan.push_shared(SharedBody::Commands)?;
    plan.finish_stage(3, start)?;

    let start = plan.len();
    dynamic(plan, ids.recipe_book_settings, scratch, |writer| {
        snapshot.recipe_settings.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.recipe_book_add, scratch, |writer| {
        FreshRecipeBookAddPayload.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(4, start)?;

    let start = plan.len();
    dynamic(plan, ids.player_position, scratch, |writer| {
        teleport.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(5, start)?;

    let start = plan.len();
    if server_data.is_some() {
        plan.push_shared(SharedBody::ServerData)?;
    }
    plan.finish_stage(6, start)?;

    let start = plan.len();
    dynamic(plan, ids.player_info_update, scratch, |writer| {
        encode_initial_player_info(snapshot.existing_players, writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.player_info_update, scratch, |writer| {
        encode_initial_player_info(core::slice::from_ref(&snapshot.joining_player), writer)
            .map_err(Into::into)
    })?;
    plan.finish_stage(7, start)?;

    prepare_level_stage(plan, snapshot, ids, scratch)?;

    let start = plan.len();
    dynamic(plan, ids.container_set_content, scratch, |writer| {
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
    dynamic(plan, ids.initialize_border, scratch, |writer| {
        snapshot.border.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.set_time, scratch, |writer| {
        snapshot.clock.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.set_default_spawn_position, scratch, |writer| {
        snapshot.spawn.encode(writer).map_err(Into::into)
    })?;

    if let BootstrapWeather::Raining {
        rain_level,
        thunder_level,
    } = snapshot.weather
    {
        weather_event(
            plan,
            ids.game_event,
            scratch,
            BootstrapGameEvent::StartRaining,
            0.0,
        )?;
        weather_event(
            plan,
            ids.game_event,
            scratch,
            BootstrapGameEvent::RainLevelChange,
            rain_level,
        )?;
        weather_event(
            plan,
            ids.game_event,
            scratch,
            BootstrapGameEvent::ThunderLevelChange,
            thunder_level,
        )?;
    }

    weather_event(
        plan,
        ids.game_event,
        scratch,
        BootstrapGameEvent::LevelChunksLoadStart,
        0.0,
    )?;
    dynamic(plan, ids.ticking_state, scratch, |writer| {
        snapshot.ticking_state.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, ids.ticking_step, scratch, |writer| {
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
