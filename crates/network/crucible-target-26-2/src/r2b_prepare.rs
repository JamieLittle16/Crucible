//! Transactional semantic preparation for the finite replay-free R2B bootstrap.
//!
//! Dynamic bodies are encoded through one reused bounded `PacketWriter`, copied once into the
//! contiguous arena owned by [`PreparedR2bPlan`], and indexed in source-backed semantic stage order.
//! Commands, synchronized recipes and optional server data remain borrowed immutable artifacts whose
//! packet identities were certified once at construction. Every Minecraft 26.2 packet identity used
//! here is a private compile-time target fact: callers cannot supply, validate, or vary packet IDs on
//! the join path.

use crucible_packet_core::{PacketCodecError, PacketWriter};

use crate::r2b::{
    CommandProjectionKey, PLAY_PACKET_IDS, PlayBootstrapImage26_2, ProjectionArtifactError,
    ProjectionRevision, QualifiedProjectionArtifact, RecipeProjectionKey,
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
use crate::r2b_recipe_add::encode_fresh_recipe_book_add;
use crate::r2b_spawn::DefaultSpawnPayload;
use crate::r2b_teleport::{AbsoluteTeleportPayload, TeleportTransaction};

/// Selected/default dynamic arena reservation hint.
///
/// This is a capacity hint, not a semantic limit; larger admitted player/clock state may grow the
/// same single arena owner.
pub const SELECTED_DYNAMIC_ARENA_CAPACITY: usize = 512;

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

impl QualifiedProjectionArtifact<ServerDataProjectionKey> {
    /// Creates server-data projection bytes only for the Minecraft 26.2 server-data packet kind.
    ///
    /// # Errors
    ///
    /// Rejects empty, malformed/non-canonical, or wrong-packet bodies before the artifact can be
    /// borrowed by any joining connection.
    pub fn new(
        key: ServerDataProjectionKey,
        body: Box<[u8]>,
    ) -> Result<Self, ProjectionArtifactError> {
        Self::new_with_packet_id(key, body, PLAY_PACKET_IDS.server_data)
    }
}

/// Requested immutable server-data artifact for the current join.
#[derive(Clone, Copy, Debug)]
pub struct ServerDataProjection<'a> {
    /// Cached status body qualified for an exact revision key and packet kind.
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
    /// Immutable shared projection lookup mismatch.
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
    /// on a local copy and committed only after the entire plan succeeds. Packet identities are
    /// compile-time target facts; shared artifacts have already certified their packet kind before
    /// entering this join path.
    ///
    /// # Errors
    ///
    /// Fails closed on projection-key mismatch, semantic codec failure, arena overflow or inline plan
    /// overflow. The caller's teleport state remains unchanged and `scratch` is reset on every error
    /// path.
    pub fn prepare(
        snapshot: FreshR2bBootstrapSnapshot<'a>,
        image: &'a PlayBootstrapImage26_2,
        scratch: &mut PacketWriter,
        teleport_state: &mut TeleportTransaction,
        arena_capacity: usize,
    ) -> Result<Self, PrepareR2bError> {
        scratch.reset();

        let commands = image.commands(&snapshot.command_key)?;
        let update_recipes = image.update_recipes(&snapshot.recipe_key)?;

        let server_data = match snapshot.server_data {
            Some(projection) => Some(projection.artifact.body_for(&projection.requested)?),
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

        if let Err(error) = prepare_stages(&mut plan, &snapshot, teleport, server_data, scratch) {
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
    snapshot: &FreshR2bBootstrapSnapshot<'_>,
    teleport: AbsoluteTeleportPayload,
    server_data: Option<&[u8]>,
    scratch: &mut PacketWriter,
) -> Result<(), PrepareR2bError> {
    let start = plan.len();
    dynamic(plan, PLAY_PACKET_IDS.login, scratch, |writer| {
        snapshot.login.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(0, start)?;

    let start = plan.len();
    dynamic(plan, PLAY_PACKET_IDS.change_difficulty, scratch, |writer| {
        snapshot.difficulty.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, PLAY_PACKET_IDS.player_abilities, scratch, |writer| {
        snapshot.abilities.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, PLAY_PACKET_IDS.set_held_slot, scratch, |writer| {
        snapshot.held_slot.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(1, start)?;

    let start = plan.len();
    plan.push_shared(SharedBody::UpdateRecipes)?;
    plan.finish_stage(2, start)?;

    let start = plan.len();
    dynamic(plan, PLAY_PACKET_IDS.entity_event, scratch, |writer| {
        snapshot.permission_event.encode(writer).map_err(Into::into)
    })?;
    plan.push_shared(SharedBody::Commands)?;
    plan.finish_stage(3, start)?;

    let start = plan.len();
    dynamic(
        plan,
        PLAY_PACKET_IDS.recipe_book_settings,
        scratch,
        |writer| snapshot.recipe_settings.encode(writer).map_err(Into::into),
    )?;
    dynamic(plan, PLAY_PACKET_IDS.recipe_book_add, scratch, |writer| {
        encode_fresh_recipe_book_add(writer).map_err(Into::into)
    })?;
    plan.finish_stage(4, start)?;

    let start = plan.len();
    dynamic(plan, PLAY_PACKET_IDS.player_position, scratch, |writer| {
        teleport.encode(writer).map_err(Into::into)
    })?;
    plan.finish_stage(5, start)?;

    let start = plan.len();
    if server_data.is_some() {
        plan.push_shared(SharedBody::ServerData)?;
    }
    plan.finish_stage(6, start)?;

    let start = plan.len();
    dynamic(
        plan,
        PLAY_PACKET_IDS.player_info_update,
        scratch,
        |writer| encode_initial_player_info(snapshot.existing_players, writer).map_err(Into::into),
    )?;
    dynamic(
        plan,
        PLAY_PACKET_IDS.player_info_update,
        scratch,
        |writer| {
            encode_initial_player_info(core::slice::from_ref(&snapshot.joining_player), writer)
                .map_err(Into::into)
        },
    )?;
    plan.finish_stage(7, start)?;

    prepare_level_stage(plan, snapshot, scratch)?;

    let start = plan.len();
    dynamic(
        plan,
        PLAY_PACKET_IDS.container_set_content,
        scratch,
        |writer| snapshot.inventory.encode(writer).map_err(Into::into),
    )?;
    plan.finish_stage(9, start)?;
    Ok(())
}

fn prepare_level_stage(
    plan: &mut PreparedR2bPlanBuilder<'_>,
    snapshot: &FreshR2bBootstrapSnapshot<'_>,
    scratch: &mut PacketWriter,
) -> Result<(), PrepareR2bError> {
    let start = plan.len();
    dynamic(plan, PLAY_PACKET_IDS.initialize_border, scratch, |writer| {
        snapshot.border.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, PLAY_PACKET_IDS.set_time, scratch, |writer| {
        snapshot.clock.encode(writer).map_err(Into::into)
    })?;
    dynamic(
        plan,
        PLAY_PACKET_IDS.set_default_spawn_position,
        scratch,
        |writer| snapshot.spawn.encode(writer).map_err(Into::into),
    )?;

    if let BootstrapWeather::Raining {
        rain_level,
        thunder_level,
    } = snapshot.weather
    {
        weather_event(plan, scratch, BootstrapGameEvent::StartRaining, 0.0)?;
        weather_event(
            plan,
            scratch,
            BootstrapGameEvent::RainLevelChange,
            rain_level,
        )?;
        weather_event(
            plan,
            scratch,
            BootstrapGameEvent::ThunderLevelChange,
            thunder_level,
        )?;
    }

    weather_event(plan, scratch, BootstrapGameEvent::LevelChunksLoadStart, 0.0)?;
    dynamic(plan, PLAY_PACKET_IDS.ticking_state, scratch, |writer| {
        snapshot.ticking_state.encode(writer).map_err(Into::into)
    })?;
    dynamic(plan, PLAY_PACKET_IDS.ticking_step, scratch, |writer| {
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
    scratch: &mut PacketWriter,
    event: BootstrapGameEvent,
    parameter: f32,
) -> Result<(), PrepareR2bError> {
    dynamic(plan, PLAY_PACKET_IDS.game_event, scratch, |writer| {
        GameEventPayload { event, parameter }
            .encode(writer)
            .map_err(Into::into)
    })
}
