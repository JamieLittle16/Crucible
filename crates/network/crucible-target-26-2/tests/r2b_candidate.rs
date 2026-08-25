//! Qualification-only compiler/test harness for the source-gated R2B target model.
//!
//! `src/r2b.rs` is deliberately not exported by the production target until the final reusable
//! codec seam and `GATE-NET-PLAY-ENTRY-26_2-001` are independently green. Keeping this harness as a
//! separate integration target lets CI compile and test the candidate architecture without making
//! it reachable from production code prematurely.

#[path = "../src/r2b.rs"]
pub mod r2b;
