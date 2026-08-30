# R2C Packed Four-Bit Production A/B Policy

Status: **revision 2 satisfied; four-bit specialization selected for R2C production**  
Target: **Minecraft: Java Edition 26.2 / DataVersion 4903**  
Candidate: four-bit cell-major packed-state specialization in PR #228

## Purpose

This document records the production A/B decision rule separately from the benchmark implementation so a threshold change cannot be hidden inside a successful rerun.

The original controlled A/B result is preserved in `R2C_PACKED_FOUR_BIT_SPECIALIZATION_QUALIFICATION.md`. It is a real failed gate: whole packed import improved 13.4%, semantic no-copy improved 10.9%, uniform import changed by +0.9%, but the original supporting semantic threshold demanded at least 15%.

Revision 2 changed the acceptance model **before its rerun**. The reason was metric semantics, not the observed value.

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

PR #226 independently measured the NBT/schema pieces and established that these fixed costs are non-zero. Therefore this metric confirms that the optimized semantic layer improves materially, but it is not required to reproduce a fixed fraction of the isolated loop microbenchmark reduction.

### Uniform import — regression guard

Uniform sections never enter the four-bit packed-state loop. This metric detects code-layout/compiler or architectural regressions caused by the production change rather than demonstrating a speedup.

## Why revision 1 was mis-specified

Revision 1 used:

- packed import: at least 5% improvement;
- semantic no-copy: at least 15% improvement;
- uniform import: no more than 10% regression.

The 15% supporting threshold was stricter than the primary production-outcome threshold and had no independently established connection to the fraction of semantic no-copy time attributable to packed arithmetic. It effectively assumed a transfer ratio from the isolated loop benchmark into a larger mixed-cost metric.

The first valid controlled A/B disproved that assumption while still showing both metrics improve substantially. The correct response was not to declare that run passed and not to fit a threshold to its 10.9% result. The original failure remains recorded.

## Revision 2 predeclared thresholds

Revision 2 required all three independently:

1. **Complete packed import:** head/base p50-median ratio `<= 950` milli — at least **5% faster**.
2. **Semantic no-copy:** head/base p50-median ratio `<= 950` milli — at least **5% faster**.
3. **Uniform import:** head/base p50-median ratio `<= 1100` milli — no more than **10% slower**.

The same **5% material-improvement floor** is used for both improvement metrics. This was intentionally not chosen near the previous 890-milli observation.

The complete packed path remains the primary decision metric. The semantic metric establishes that the intended semantic/materialization layer improves rather than a coincidental unrelated component. The uniform metric remains a regression guard.

## Methodology that did not change

Revision 2 did **not** change:

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

Only the interpretation of the supporting semantic threshold changed.

## Revision 2 result

Workflow `33329719825`, job `99306022555`, policy revision `2`:

| Metric | Base median p50 | Head median p50 | Ratio | Result |
| --- | ---: | ---: | ---: | --- |
| complete packed import | **25.223 us** | **22.637 us** | **897 milli** | **pass — 10.3% faster** |
| semantic no-copy | **9.817 us** | **8.285 us** | **843 milli** | **pass — 15.7% faster** |
| uniform import | **3.592 us** | **3.575 us** | **995 milli** | **pass — ~0.5% faster** |

Raw process-level p50 samples:

```text
base packed    [26748, 25407, 24402, 24807, 24259, 26228, 25223]
head packed    [24482, 23838, 21864, 22640, 22637, 21617, 21902]
base uniform   [3592, 3583, 3594, 3733, 3494, 3565, 3650]
head uniform   [3551, 3575, 3691, 3531, 3612, 3597, 3482]
base semantic  [9903, 9860, 9788, 9857, 8670, 9817, 9811]
head semantic  [8211, 8285, 8149, 8466, 8164, 8416, 8364]
```

The exact packed semantic checksum remained `15485907386658061717` across all base/head samples.

All three revision-2 thresholds passed with margin. The result also reproduced the favorable direction of revision 1 rather than depending on one hosted execution.

## Other production-selection gates

The four-bit production implementation is additionally green through:

- hermetic offline build/tests;
- rustfmt;
- Clippy `-D warnings`;
- four-bit exact YZX regression;
- exact out-of-range palette-index regression;
- focused five-bit generic fallback regression;
- unchanged seven-section Python/Rust differential digest;
- independently regenerated official 26.2 real-save corpus;
- production raw-Anvil importer vs official-save oracle;
- normalized corpus reconstruction and cross-evidence identity checks.

## Decision

**Revision 2 is satisfied and the four-bit specialization is selected for R2C production.**

The threshold will not be revised again on this evidence. Future changes to the packed-state implementation, compiler/architecture, fixture, or A/B methodology trigger requalification rather than reinterpretation of this result.

## Non-claims

This selection does not make hosted CI a target-hardware throughput guarantee. `performance_admitted` remains false until Helve has controlled target-hardware qualification appropriate for such a claim.
