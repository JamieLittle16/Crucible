//! Qualification harness for the canonical R2B target-library implementation.
//!
//! R2B is compiled exactly once by `crucible-target-26-2`. These tiny compatibility namespaces keep
//! the existing focused qualification modules readable while every exported type below is merely a
//! re-export of the library facade; no encoder, arena, wire codec or plan source file is path-included
//! into this integration-test crate. Production `Target26_2` Play routing remains isolated until the
//! replay-free runtime qualification suite passes.

pub use crucible_target_26_2::r2b;

pub mod r2b_border {
    pub use crucible_target_26_2::r2b::WorldBorderPayload;
}

pub mod r2b_clock {
    pub use crucible_target_26_2::r2b::{ClockFullSyncPayload, ClockUpdate};
}

pub mod r2b_difficulty {
    pub use crucible_target_26_2::r2b::{ChangeDifficultyPayload, Difficulty26_2};
}

pub mod r2b_dynamic {
    pub use crucible_target_26_2::r2b::{
        HeldSlotPayload, PermissionEntityEventPayload, PermissionLevelEvent, PlayerAbilitiesPayload,
        PlayerAbilityFlags, TickingStatePayload, TickingStepPayload,
    };
}

pub mod r2b_inventory {
    pub use crucible_target_26_2::r2b::FreshEmptyInventoryPayload;
}

pub mod r2b_login {
    pub use crucible_target_26_2::r2b::{
        BootstrapGameMode, FreshCommonSpawnInfo, FreshLoginFlags, FreshLoginPayload,
    };
}

pub mod r2b_plan {
    pub use crucible_target_26_2::r2b::{PreparedLookup, PreparedR2bPlan};
}

pub mod r2b_player_info {
    pub use crucible_target_26_2::r2b::InitialPlayerInfoEntry;
}

pub mod r2b_prepare {
    pub use crucible_target_26_2::r2b::{
        BootstrapWeather, FreshR2bBootstrapSnapshot, PlayPacketIds, PrepareR2bError,
        SELECTED_DYNAMIC_ARENA_CAPACITY, ServerDataProjection, ServerDataProjectionKey,
        TeleportDestination,
    };
}

pub mod r2b_recipe {
    pub use crucible_target_26_2::r2b::{RecipeBookSettingFlags, RecipeBookSettingsPayload};
}

pub mod r2b_spawn {
    pub use crucible_target_26_2::r2b::DefaultSpawnPayload;
}

pub mod r2b_teleport {
    pub use crucible_target_26_2::r2b::TeleportTransaction;
}

#[path = "support/r2b_black_box_qualification.rs"]
mod r2b_black_box_qualification;
#[path = "support/r2b_prepare_qualification.rs"]
mod r2b_prepare_qualification;
