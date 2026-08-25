//! Qualification-only compiler/test harness for the source-gated R2B target model.
//!
//! `src/r2b.rs` and the reusable final-seam wire primitives are deliberately compiled here without
//! making the live `Target26_2` route depend on them yet. The Play-entry source gate is independently
//! green; repository evidence installation and runtime bootstrap qualification still precede
//! production routing.

#[path = "../src/r2b.rs"]
pub mod r2b;
#[path = "../src/r2b_wire.rs"]
pub mod r2b_wire;
