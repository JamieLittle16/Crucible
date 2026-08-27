//! Replay-free R2B Play-bootstrap target model for Minecraft Java 26.2.
//!
//! This is the public experimental R2B projection surface used by qualification and the development
//! runtime. The normal `Target26_2` Play route remains deliberately isolated until the stock-client
//! runtime gate is green. Encoder, arena and wire implementation modules stay private to this crate;
//! callers receive only compact semantic inputs, immutable projection artifacts, the prepared plan
//! and connection-owned teleport state.
//!
//! The permanent shape is deliberately small:
//!
//! ```text
//! source-frozen semantic stage order
//!          +
//! qualified shared command/recipe/status artifacts
//!          +
//! one contiguous per-join dynamic-body arena
//!          +
//! target-neutral StagedPublicationCursor (owned elsewhere)
//!          +
//! explicit WorldProjection seam
//! ```
//!
//! Dynamic player/world packet bodies are target-owned compact projections prepared once into the
//! contiguous arena. Packet identities remain generated/static target facts; captured replay is
//! qualification evidence only and never becomes the production publication mechanism.

use core::fmt;

pub use crate::r2b_border::WorldBorderPayload;
pub use crate::r2b_clock::{ClockFullSyncPayload, ClockUpdate};
pub use crate::r2b_difficulty::{ChangeDifficultyPayload, Difficulty26_2};
pub use crate::r2b_dynamic::{
    HeldSlotPayload, PermissionEntityEventPayload, PermissionLevelEvent, PlayerAbilitiesPayload,
    PlayerAbilityFlags, TickingStatePayload, TickingStepPayload,
};
pub use crate::r2b_inventory::FreshEmptyInventoryPayload;
pub use crate::r2b_login::{
    BootstrapGameMode, FreshCommonSpawnInfo, FreshLoginFlags, FreshLoginPayload,
};
pub use crate::r2b_plan::{PreparedLookup, PreparedR2bPlan};
pub use crate::r2b_player_info::InitialPlayerInfoEntry;
pub use crate::r2b_prepare::{
    BootstrapWeather, FreshR2bBootstrapSnapshot, PrepareR2bError, SELECTED_DYNAMIC_ARENA_CAPACITY,
    ServerDataProjection, ServerDataProjectionArtifact, ServerDataProjectionKey,
    TeleportDestination,
};
pub use crate::r2b_recipe::{RecipeBookSettingFlags, RecipeBookSettingsPayload};
pub use crate::r2b_spawn::DefaultSpawnPayload;
pub use crate::r2b_teleport::{
    AwaitingTeleport, TeleportAckResult, TeleportTransaction, decode_serverbound_teleport_ack,
};

/// Semantic Play-bootstrap stages frozen by `SEM-NET-R2B-PLAY-001..015`.
///
/// These are semantic groupings rather than packet identities. `WorldProjection` is an explicit
/// ownership handoff and therefore is not part of the finite network-owned publication array.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayBootstrapStage {
    /// Direct fresh-player world-entry prefix.
    EnterWorld,
    /// Difficulty, abilities and held-slot state following entry.
    CorePlayerState,
    /// Composition-stable synchronized recipe publication.
    CompositionRecipes,
    /// Permission event followed by the qualified command projection.
    PermissionAndCommands,
    /// Player-specific recipe-book settings and initial recipe-add state.
    RecipeBook,
    /// Initial absolute position transaction awaiting client acknowledgement.
    Teleport,
    /// Conditional server-status publication for the admitted non-transfer route.
    ConditionalServerStatus,
    /// Existing-player initialization followed by self-visible joining-player publication.
    PlayerInfo,
    /// Border, clock, spawn, optional weather, load-start and tick-rate state.
    LevelBootstrap,
    /// Fresh empty inventory/menu snapshot.
    InventorySnapshot,
    /// Explicit seam where R2C takes ownership of world/chunk/light projection.
    WorldProjection,
    /// Bootstrap is complete after the world owner accepts the handoff.
    Complete,
}

/// Network-owned stages supplied to the target-neutral staged publication primitive.
///
/// The order is source law. Empty conditional stages remain present so the generic staged cursor
/// commits their boundaries explicitly instead of silently skipping work in a loop.
pub const PLAY_PUBLICATION_STAGES: [PlayBootstrapStage; 10] = [
    PlayBootstrapStage::EnterWorld,
    PlayBootstrapStage::CorePlayerState,
    PlayBootstrapStage::CompositionRecipes,
    PlayBootstrapStage::PermissionAndCommands,
    PlayBootstrapStage::RecipeBook,
    PlayBootstrapStage::Teleport,
    PlayBootstrapStage::ConditionalServerStatus,
    PlayBootstrapStage::PlayerInfo,
    PlayBootstrapStage::LevelBootstrap,
    PlayBootstrapStage::InventorySnapshot,
];

/// Resolves the generic staged-publication index to its R2B semantic stage.
///
/// Once every finite network-owned stage is complete the only valid next state is the explicit
/// `WorldProjection` handoff. This function deliberately never returns `Complete`; completion must
/// be acknowledged by the eventual world owner rather than inferred by networking.
#[must_use]
pub const fn stage_for_publication_index(index: usize) -> PlayBootstrapStage {
    if index < PLAY_PUBLICATION_STAGES.len() {
        PLAY_PUBLICATION_STAGES[index]
    } else {
        PlayBootstrapStage::WorldProjection
    }
}

/// Opaque 256-bit revision commitment used in immutable projection keys.
///
/// The producer decides what canonical object is committed by these bytes. R2B compares revisions
/// exactly and never treats a mismatch as permission to reuse stale projection bytes.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProjectionRevision([u8; 32]);

impl ProjectionRevision {
    /// Builds one exact revision commitment.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the canonical commitment bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Permission profile admitted by the first immutable command projection.
///
/// Future operator/plugin permission surfaces must add separately admitted variants rather than
/// broadening this one implicitly.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CommandPermissionProfile {
    /// Selected fresh/default non-operator route.
    DefaultNonOperator,
}

/// Exact invalidation key for the permission-filtered command projection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CommandProjectionKey {
    protocol_contract: ProjectionRevision,
    command_composition: ProjectionRevision,
    argument_registry_composition: ProjectionRevision,
    enabled_features: ProjectionRevision,
    permission: CommandPermissionProfile,
}

impl CommandProjectionKey {
    /// Creates a complete command-projection identity.
    #[must_use]
    pub const fn new(
        protocol_contract: ProjectionRevision,
        command_composition: ProjectionRevision,
        argument_registry_composition: ProjectionRevision,
        enabled_features: ProjectionRevision,
        permission: CommandPermissionProfile,
    ) -> Self {
        Self {
            protocol_contract,
            command_composition,
            argument_registry_composition,
            enabled_features,
            permission,
        }
    }
}

/// Exact invalidation key for the synchronized-recipe projection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RecipeProjectionKey {
    protocol_contract: ProjectionRevision,
    recipe_composition: ProjectionRevision,
    registry_composition: ProjectionRevision,
    enabled_features: ProjectionRevision,
}

impl RecipeProjectionKey {
    /// Creates a complete synchronized-recipe projection identity.
    #[must_use]
    pub const fn new(
        protocol_contract: ProjectionRevision,
        recipe_composition: ProjectionRevision,
        registry_composition: ProjectionRevision,
        enabled_features: ProjectionRevision,
    ) -> Self {
        Self {
            protocol_contract,
            recipe_composition,
            registry_composition,
            enabled_features,
        }
    }
}

/// Exact finite Minecraft Java 26.2 Play packet identities frozen by the admitted R2B source law.
///
/// This value is crate-private target data, not a runtime registry. Keeping every assembler and
/// immutable-artifact identity here prevents duplicate protocol constants while permitting artifact
/// construction to certify shared bodies once, before any connection can borrow them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlayPacketIds {
    pub(crate) change_difficulty: i32,
    pub(crate) commands: i32,
    pub(crate) container_set_content: i32,
    pub(crate) entity_event: i32,
    pub(crate) game_event: i32,
    pub(crate) initialize_border: i32,
    pub(crate) login: i32,
    pub(crate) player_abilities: i32,
    pub(crate) player_info_update: i32,
    pub(crate) player_position: i32,
    pub(crate) recipe_book_add: i32,
    pub(crate) recipe_book_settings: i32,
    pub(crate) server_data: i32,
    pub(crate) set_default_spawn_position: i32,
    pub(crate) set_held_slot: i32,
    pub(crate) set_time: i32,
    pub(crate) ticking_state: i32,
    pub(crate) ticking_step: i32,
    pub(crate) update_recipes: i32,
}

pub(crate) const PLAY_PACKET_IDS: PlayPacketIds = PlayPacketIds {
    change_difficulty: 10,
    commands: 16,
    container_set_content: 18,
    entity_event: 34,
    game_event: 38,
    initialize_border: 43,
    login: 49,
    player_abilities: 64,
    player_info_update: 70,
    player_position: 72,
    recipe_book_add: 74,
    recipe_book_settings: 76,
    server_data: 86,
    set_default_spawn_position: 97,
    set_held_slot: 105,
    set_time: 113,
    ticking_state: 127,
    ticking_step: 128,
    update_recipes: 133,
};

/// Fail-closed immutable projection construction/lookup error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionArtifactError {
    /// A packet-body artifact may never be empty.
    EmptyBody,
    /// A shared body has no canonical non-negative packet-ID `VarInt`.
    InvalidPacketBodyIdentity,
    /// A shared body carries a different packet identity from its typed projection kind.
    PacketIdMismatch {
        /// Target-owned packet ID required by the projection kind.
        expected: i32,
        /// Canonical packet ID decoded from the supplied body.
        actual: i32,
    },
    /// The requested composition/profile identity does not match the encoded artifact.
    KeyMismatch,
}

/// One immutable encoded packet body qualified for exactly one projection key and packet kind.
///
/// Construction certifies the canonical packet identity once. Ordinary connections therefore borrow
/// bytes that are already known to be the correct packet kind and only compare revision keys; the
/// join path never reparses or revalidates a shared body's packet-ID `VarInt`.
pub struct QualifiedProjectionArtifact<K> {
    key: K,
    body: Box<[u8]>,
}

/// Packet-qualified immutable command-tree artifact.
pub type CommandProjectionArtifact = QualifiedProjectionArtifact<CommandProjectionKey>;
/// Packet-qualified immutable synchronized-recipe artifact.
pub type RecipeProjectionArtifact = QualifiedProjectionArtifact<RecipeProjectionKey>;

impl<K> QualifiedProjectionArtifact<K> {
    pub(crate) fn new_with_packet_id(
        key: K,
        body: Box<[u8]>,
        expected_packet_id: i32,
    ) -> Result<Self, ProjectionArtifactError> {
        if body.is_empty() {
            return Err(ProjectionArtifactError::EmptyBody);
        }
        let actual = decode_nonnegative_var_int(&body)
            .ok_or(ProjectionArtifactError::InvalidPacketBodyIdentity)?;
        if actual != expected_packet_id {
            return Err(ProjectionArtifactError::PacketIdMismatch {
                expected: expected_packet_id,
                actual,
            });
        }
        Ok(Self { key, body })
    }

    /// Returns the exact key which qualifies this body.
    #[must_use]
    pub const fn key(&self) -> &K {
        &self.key
    }
}

impl QualifiedProjectionArtifact<CommandProjectionKey> {
    /// Creates a command projection only when its body carries the 26.2 commands packet identity.
    ///
    /// # Errors
    ///
    /// Rejects empty, malformed/non-canonical, or wrong-packet bodies before the artifact can be
    /// shared with any connection.
    pub fn new(
        key: CommandProjectionKey,
        body: Box<[u8]>,
    ) -> Result<Self, ProjectionArtifactError> {
        Self::new_with_packet_id(key, body, PLAY_PACKET_IDS.commands)
    }
}

impl QualifiedProjectionArtifact<RecipeProjectionKey> {
    /// Creates a synchronized-recipe projection only for the 26.2 update-recipes packet identity.
    ///
    /// # Errors
    ///
    /// Rejects empty, malformed/non-canonical, or wrong-packet bodies before the artifact can be
    /// shared with any connection.
    pub fn new(key: RecipeProjectionKey, body: Box<[u8]>) -> Result<Self, ProjectionArtifactError> {
        Self::new_with_packet_id(key, body, PLAY_PACKET_IDS.update_recipes)
    }
}

impl<K> QualifiedProjectionArtifact<K>
where
    K: Eq,
{
    /// Borrows the encoded body only when the requested projection identity matches exactly.
    ///
    /// # Errors
    ///
    /// Returns `KeyMismatch`; stale or differently permissioned composition bytes are never a
    /// fallback path.
    pub fn body_for(&self, requested: &K) -> Result<&[u8], ProjectionArtifactError> {
        if requested != &self.key {
            return Err(ProjectionArtifactError::KeyMismatch);
        }
        Ok(&self.body)
    }
}

impl<K> fmt::Debug for QualifiedProjectionArtifact<K>
where
    K: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QualifiedProjectionArtifact")
            .field("key", &self.key)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// Process/composition-owned immutable R2B publication artifacts for Minecraft 26.2.
///
/// Sharing this image across matching connections makes command-tree filtering and synchronized
/// recipe serialization a composition cost rather than a per-join cost. Dynamic player/world state
/// is intentionally not stored here.
#[derive(Debug)]
pub struct PlayBootstrapImage26_2 {
    commands: CommandProjectionArtifact,
    update_recipes: RecipeProjectionArtifact,
}

impl PlayBootstrapImage26_2 {
    /// Creates one shared image from already packet-qualified immutable artifacts.
    #[must_use]
    pub const fn new(
        commands: CommandProjectionArtifact,
        update_recipes: RecipeProjectionArtifact,
    ) -> Self {
        Self {
            commands,
            update_recipes,
        }
    }

    /// Borrows the permission-filtered command body for an exact composition key.
    ///
    /// # Errors
    ///
    /// Returns `KeyMismatch` when any command invalidation input differs.
    pub fn commands(
        &self,
        requested: &CommandProjectionKey,
    ) -> Result<&[u8], ProjectionArtifactError> {
        self.commands.body_for(requested)
    }

    /// Borrows the synchronized-recipe body for an exact composition key.
    ///
    /// # Errors
    ///
    /// Returns `KeyMismatch` when any recipe invalidation input differs.
    pub fn update_recipes(
        &self,
        requested: &RecipeProjectionKey,
    ) -> Result<&[u8], ProjectionArtifactError> {
        self.update_recipes.body_for(requested)
    }
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
    use super::{
        CommandPermissionProfile, CommandProjectionArtifact, CommandProjectionKey,
        PLAY_PUBLICATION_STAGES, PlayBootstrapImage26_2, PlayBootstrapStage,
        ProjectionArtifactError, ProjectionRevision, RecipeProjectionArtifact, RecipeProjectionKey,
        stage_for_publication_index,
    };

    const fn revision(byte: u8) -> ProjectionRevision {
        ProjectionRevision::new([byte; 32])
    }

    const fn command_key(seed: u8) -> CommandProjectionKey {
        CommandProjectionKey::new(
            revision(seed),
            revision(seed.wrapping_add(1)),
            revision(seed.wrapping_add(2)),
            revision(seed.wrapping_add(3)),
            CommandPermissionProfile::DefaultNonOperator,
        )
    }

    const fn recipe_key(seed: u8) -> RecipeProjectionKey {
        RecipeProjectionKey::new(
            revision(seed),
            revision(seed.wrapping_add(1)),
            revision(seed.wrapping_add(2)),
            revision(seed.wrapping_add(3)),
        )
    }

    #[test]
    fn semantic_publication_order_is_exact_and_world_owned_tail_is_excluded() {
        assert_eq!(
            PLAY_PUBLICATION_STAGES,
            [
                PlayBootstrapStage::EnterWorld,
                PlayBootstrapStage::CorePlayerState,
                PlayBootstrapStage::CompositionRecipes,
                PlayBootstrapStage::PermissionAndCommands,
                PlayBootstrapStage::RecipeBook,
                PlayBootstrapStage::Teleport,
                PlayBootstrapStage::ConditionalServerStatus,
                PlayBootstrapStage::PlayerInfo,
                PlayBootstrapStage::LevelBootstrap,
                PlayBootstrapStage::InventorySnapshot,
            ]
        );
        for (index, expected) in PLAY_PUBLICATION_STAGES.into_iter().enumerate() {
            assert_eq!(stage_for_publication_index(index), expected);
        }
        assert_eq!(
            stage_for_publication_index(PLAY_PUBLICATION_STAGES.len()),
            PlayBootstrapStage::WorldProjection
        );
        assert_eq!(
            stage_for_publication_index(usize::MAX),
            PlayBootstrapStage::WorldProjection
        );
        assert!(!PLAY_PUBLICATION_STAGES.contains(&PlayBootstrapStage::WorldProjection));
        assert!(!PLAY_PUBLICATION_STAGES.contains(&PlayBootstrapStage::Complete));
    }

    #[test]
    fn empty_projection_artifacts_fail_closed() {
        assert_eq!(
            CommandProjectionArtifact::new(command_key(1), Box::<[u8]>::default())
                .expect_err("empty command body must be rejected"),
            ProjectionArtifactError::EmptyBody
        );
    }

    #[test]
    fn packet_identity_is_certified_at_artifact_construction() {
        assert_eq!(
            CommandProjectionArtifact::new(command_key(1), vec![0x11].into_boxed_slice())
                .expect_err("wrong command packet kind must be rejected"),
            ProjectionArtifactError::PacketIdMismatch {
                expected: 16,
                actual: 17,
            }
        );
        assert_eq!(
            CommandProjectionArtifact::new(command_key(1), vec![0x90, 0x00].into_boxed_slice())
                .expect_err("non-canonical command packet id must be rejected"),
            ProjectionArtifactError::InvalidPacketBodyIdentity
        );
    }

    #[test]
    fn command_projection_never_reuses_a_mismatched_key() {
        let key = command_key(10);
        let stale = command_key(11);
        let artifact = CommandProjectionArtifact::new(key, vec![0x10, 0xAA].into_boxed_slice())
            .expect("packet-qualified command artifact");

        assert_eq!(artifact.body_for(&key), Ok(&[0x10, 0xAA][..]));
        assert_eq!(
            artifact.body_for(&stale),
            Err(ProjectionArtifactError::KeyMismatch)
        );
    }

    #[test]
    fn shared_image_borrows_the_same_artifact_bytes_without_per_join_reconstruction() {
        let commands_key = command_key(20);
        let recipes_key = recipe_key(30);
        let image = PlayBootstrapImage26_2::new(
            CommandProjectionArtifact::new(commands_key, vec![0x10, 0x01, 0x02].into_boxed_slice())
                .expect("commands artifact"),
            RecipeProjectionArtifact::new(recipes_key, vec![0x85, 0x01, 0x03].into_boxed_slice())
                .expect("recipes artifact"),
        );

        let first = image.commands(&commands_key).expect("matching command key");
        let second = image.commands(&commands_key).expect("matching command key");
        assert_eq!(first.as_ptr(), second.as_ptr());
        assert_eq!(first, &[0x10, 0x01, 0x02]);
        assert_eq!(
            image.update_recipes(&recipes_key),
            Ok(&[0x85, 0x01, 0x03][..])
        );
    }

    #[test]
    fn recipe_and_command_invalidation_domains_are_independent() {
        let commands_key = command_key(40);
        let recipes_key = recipe_key(50);
        let image = PlayBootstrapImage26_2::new(
            CommandProjectionArtifact::new(commands_key, vec![0x10].into_boxed_slice())
                .expect("commands artifact"),
            RecipeProjectionArtifact::new(recipes_key, vec![0x85, 0x01].into_boxed_slice())
                .expect("recipes artifact"),
        );

        assert_eq!(
            image.commands(&command_key(41)),
            Err(ProjectionArtifactError::KeyMismatch)
        );
        assert_eq!(image.update_recipes(&recipes_key), Ok(&[0x85, 0x01][..]));
    }
}
