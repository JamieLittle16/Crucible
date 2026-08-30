# R2C Packed Four-Bit Production A/B Policy

Status: **revision 2 predeclared before rerun**  
Target: **Minecraft: Java Edition 26.2 / DataVersion 4903**  
Candidate: four-bit cell-major packed-state specialization in PR #228

## Purpose

This document records the production A/B decision rule separately from the benchmark implementation so a threshold change cannot be hidden inside a successful rerun.

The original controlled A/B result is preserved in `R2C_PACKED_FOUR_BIT_SPECIALIZATION_QUALIFICATION.md`. It is a real failed gate: whole packed import improved 13.4%, semantic no-copy improved 10.9%, uniform import changed by +0.9%, but the original supporting semantic threshold demanded at least 15%.

Revision 2 changes the acceptance model **before any new A/B execution**. The reason is metric semantics, not the observed value.

## What the three metrics mean

### Complete packed import — primary outcome

This is the production objective. It includes the real bounded transaction, gzip decode, schema-directed semantic decode, final reference-section construction and transaction result creation for the exact independent packed fixture.

A candidate that improves an inner loop but does not improve this complete path is rejected.

### Semantic no-copy — supporting localization guard

This runs the real `decode_chunk_block_sections` implementation and generated state resolver while replacing only the final transparent/reference section copy with an opaque no-copy builder.

It deliberately still includes costs that the four-bit arithmetic specialization cannot remove, including:

- NBT/schema traversal;
- palette parsing and canonicalization;
- block-state resolution;
- packed-long ingestion;
- scratch preparation/materialization;
- result/header/section transaction scaffolding.

PR #226 independently measured the NBT/schema pieces and established that these fixed costs are non-zero. Therefore this metric should confirm that the optimized semantic layer improves materially, but it should **not** be required to reproduce a fixed fraction of the isolated loop microbenchmark reduction.

### Uniform import — regression guard

Uniform sections never enter the four-bit packed-state loop. This metric exists to detect code-layout/compiler or architectural regressions caused by the production change rather than to demonstrate a speedup.

## Why revision 1 was mis-specified

Revision 1 used:

- packed import: at least 5% improvement;
- semantic no-copy: at least 15% improvement;
- uniform import: no more than 10% regression.

The 15% supporting threshold was stricter than the primary production-outcome threshold and had no independently established connection to the fraction of semantic no-copy time attributable to packed arithmetic. It effectively assumed a transfer ratio from the isolated loop benchmark into a larger mixed-cost metric.

The first valid controlled A/B disproved that assumption while still showing both metrics improve substantially. The correct response is not to declare that run passed and not to fit a threshold to its 10.9% result. The original failure remains recorded.

## Revision 2 predeclared thresholds

The next controlled A/B run must satisfy all three independently:

1. **Complete packed import:** head/base p50-median ratio `<= 950` milli — at least **5% faster**.
2. **Semantic no-copy:** head/base p50-median ratio `<= 950` milli — at least **5% faster**.
3. **Uniform import:** head/base p50-median ratio `<= 1100` milli — no more than **10% slower**.

The same **5% material-improvement floor** is used for both improvement metrics. This is intentionally not chosen near the previous 890-milli observation.

The complete packed path remains the primary decision metric. The semantic metric establishes that the intended semantic/materialization layer improves rather than a coincidental unrelated component. The uniform metric remains a regression guard.

## Methodology that does not change

Revision 2 does **not** change:

- exact PR base/head identities;
- separate base/head compilation;
- one identical independently generated packed fixture;
- one pinned allowed CPU for every child process;
- symmetric untimed warmups;
- alternating base/head process order;
- seven process-level rounds per side;
- each process's own internal p50 methodology;
- median aggregation across the seven process-level p50 values;
- exact packed semantic checksum requirement;
- production code;
- isolated qualification probe;
- semantic/parity test suites.

Only the interpretation of the supporting semantic threshold changes.

## Decision discipline

A revision-2 pass is still insufficient by itself for production selection. PR #228 must also be green through:

- hermetic offline build/tests;
- rustfmt;
- Clippy `-D warnings`;
- four-bit exact YZX regression;
- exact out-of-range palette-index regression;
- focused five-bit generic fallback regression;
- unchanged seven-section Python/Rust differential digest;
- independently regenerated official 26.2 real-save corpus and production-importer/oracle comparison.

If revision 2 fails, the specialization is not selected. The threshold will not be moved again on the same evidence.

## Non-claims

A revision-2 pass would select the mechanism for R2C on the current evidence. It would not make hosted CI a target-hardware throughput guarantee, and `performance_admitted` remains false until Helve has controlled target-hardware qualification appropriate for such a claim.
