//! Qualification-only compiler/test harness for the R2B target model.
//!
//! `src/r2b.rs`, the compact dynamic-body arena, scalar/recipe/border payload codecs and reusable
//! final-seam wire primitives are deliberately compiled here without making the live `Target26_2`
//! route depend on them yet. The earlier Play-entry admission exposed one additional second-order
//! Difficulty codec dependency during implementation; the hardened final seam must be re-admitted
//! before any of these candidate modules are allowed into production routing.

#[path = "../src/r2b.rs"]
pub mod r2b;
#[path = "../src/r2b_arena.rs"]
pub mod r2b_arena;
#[path = "../src/r2b_border.rs"]
pub mod r2b_border;
#[path = "../src/r2b_dynamic.rs"]
pub mod r2b_dynamic;
#[path = "../src/r2b_recipe.rs"]
pub mod r2b_recipe;
#[path = "../src/r2b_wire.rs"]
pub mod r2b_wire;
