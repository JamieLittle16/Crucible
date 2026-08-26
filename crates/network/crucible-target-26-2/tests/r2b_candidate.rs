//! Qualification-only compiler/test harness for the R2B target model.
//!
//! `src/r2b.rs`, the compact dynamic-body arena and target-owned selected-profile payload codecs are
//! deliberately compiled here without making the live `Target26_2` route depend on them yet. The
//! hardened source boundary is independently admitted; production routing remains isolated until the
//! replay-free runtime qualification suite passes.

#[path = "../src/r2b.rs"]
pub mod r2b;
#[path = "../src/r2b_arena.rs"]
pub mod r2b_arena;
#[path = "../src/r2b_border.rs"]
pub mod r2b_border;
#[path = "../src/r2b_clock.rs"]
pub mod r2b_clock;
#[path = "../src/r2b_difficulty.rs"]
pub mod r2b_difficulty;
#[path = "../src/r2b_dynamic.rs"]
pub mod r2b_dynamic;
#[path = "../src/r2b_login.rs"]
pub mod r2b_login;
#[path = "../src/r2b_recipe.rs"]
pub mod r2b_recipe;
#[path = "../src/r2b_spawn.rs"]
pub mod r2b_spawn;
#[path = "../src/r2b_teleport.rs"]
pub mod r2b_teleport;
#[path = "../src/r2b_wire.rs"]
pub mod r2b_wire;
