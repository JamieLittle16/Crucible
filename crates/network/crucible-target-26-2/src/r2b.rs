//! Replay-free R2B Play-bootstrap target model for Minecraft Java 26.2.
//!
//! This module is intentionally not exported by the production target yet. The base and bounded
//! runtime source gates are independently admitted, and the selected dynamic codecs plus prepared
//! staged plan are compiled through the qualification target. Production routing remains isolated
//! until the replay-free runtime suite and generated finite Play-entry contract are green.
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

/// Fail-closed immutable projection construction/lookup error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionArtifactError {
    /// A packet-body artifact may never be empty.
    EmptyBody,
    /// The requested composition/profile identity does not match the encoded artifact.
    KeyMismatch,
}

/// One immutable encoded packet body qualified for exactly one projection key.
///
/// The body is owned once by the surrounding bootstrap image and ordinary connections only borrow
/// it. Exact source/oracle body commitments remain qualification-layer inputs until the R2B gate is
/// independently admitted; this type therefore does not embed provisional capture hashes.
pub struct QualifiedProjectionArtifact<K> {
    key: K,
    body: Box<[u8]>,
}

impl<K> QualifiedProjectionArtifact<K> {
    /// Creates a non-empty immutable artifact.
    ///
    /// # Errors
    ///
    /// Returns `EmptyBody` instead of creating an impossible packet-body artifact.
    pub fn new(key: K, body: Box<[u8]>) -> Result<Self, ProjectionArtifactError> {
        if body.is_empty() {
            return Err(ProjectionArtifactError::EmptyBody);
        }
        Ok(Self { key, body })
    }

    /// Returns the exact key which qualifies this body.
    #[must_use]
    pub const fn key(&self) -> &K {
        &self.key
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
    commands: QualifiedProjectionArtifact<CommandProjectionKey>,
    update_recipes: QualifiedProjectionArtifact<RecipeProjectionKey>,
}

impl PlayBootstrapImage26_2 {
    /// Creates one shared image from already qualified immutable artifacts.
    #[must_use]
    pub const fn new(
        commands: QualifiedProjectionArtifact<CommandProjectionKey>,
        update_recipes: QualifiedProjectionArtifact<RecipeProjectionKey>,
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

#[cfg(test)]
mod tests {
    use super::{
        CommandPermissionProfile, CommandProjectionKey, PLAY_PUBLICATION_STAGES,
        PlayBootstrapImage26_2, PlayBootstrapStage, ProjectionArtifactError, ProjectionRevision,
        QualifiedProjectionArtifact, RecipeProjectionKey, stage_for_publication_index,
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
            QualifiedProjectionArtifact::new(command_key(1), Box::<[u8]>::default())
                .expect_err("empty command body must be rejected"),
            ProjectionArtifactError::EmptyBody
        );
    }

    #[test]
    fn command_projection_never_reuses_a_mismatched_key() {
        let key = command_key(10);
        let stale = command_key(11);
        let artifact = QualifiedProjectionArtifact::new(key, vec![0x10, 0xAA].into_boxed_slice())
            .expect("non-empty command artifact");

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
            QualifiedProjectionArtifact::new(
                commands_key,
                vec![0x10, 0x01, 0x02].into_boxed_slice(),
            )
            .expect("commands artifact"),
            QualifiedProjectionArtifact::new(
                recipes_key,
                vec![0x85, 0x01, 0x03].into_boxed_slice(),
            )
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
            QualifiedProjectionArtifact::new(commands_key, vec![0x10].into_boxed_slice())
                .expect("commands artifact"),
            QualifiedProjectionArtifact::new(recipes_key, vec![0x85, 0x01].into_boxed_slice())
                .expect("recipes artifact"),
        );

        assert_eq!(
            image.commands(&command_key(41)),
            Err(ProjectionArtifactError::KeyMismatch)
        );
        assert_eq!(image.update_recipes(&recipes_key), Ok(&[0x85, 0x01][..]));
    }
}
