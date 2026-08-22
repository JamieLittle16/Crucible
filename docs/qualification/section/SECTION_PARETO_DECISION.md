# Section Pareto Decision Protocol — M0.3D

Parent: #19  
Status: **normative decision protocol; no production mechanism selected by this document**

## Purpose

This protocol defines the final evidence boundary between qualified section measurements and a production-policy decision.

The decision layer does not benchmark anything itself. It consumes already-qualified, content-addressed evidence and answers only:

1. which production candidates are mathematically dominated;
2. which candidates remain on each dimension-separated Pareto frontier;
3. whether an optimized mechanism demonstrates a material benefit large enough to justify permanent complexity;
4. whether the evidence is structurally complete enough for a later explicit production-policy selection record.

Correctness remains a prerequisite. Performance evidence can never compensate for a semantic failure.

## Inputs

A decision analysis requires:

- one complete `section-target-combined-orchestration` qualification artifact from controlled target hardware;
- its recursively content-addressed artifact manifest;
- the nested population orchestration and every referenced population child record;
- the combined synthetic mechanism evidence;
- one full M0.3C equivalence record for each production candidate: `direct`, `adaptive`, `fast-local`, and `packed-local`.

The analyzer independently re-opens and re-hashes evidence. Top-level eligibility booleans are necessary but are not sufficient evidence by themselves.

## Eligibility firewall

The combined record is accepted only when all of the following hold:

- mode is `qualification`;
- qualification completed;
- population evidence is eligible;
- synthetic evidence is eligible;
- combined measurement evidence is eligible;
- at least five rounds were run and the round count is a multiple of five;
- decision scope is exactly `dimension-separated-only`;
- cross-dimension scoring is explicitly forbidden;
- the representative policy is exactly `vanilla-section-representative-v1`;
- the production candidate and dimension sets are exact;
- the combined evidence digest and root artifact manifest digest recompute exactly;
- every retained file named by the artifact manifest still has the recorded size and SHA-256;
- nested population evidence agrees with the combined source/binary/population identities;
- all four full semantic qualification records bind to the same source commit and frozen 26.2 target data.

A hosted smoke artifact is therefore structurally useful but decision-ineligible by construction.

## Correctness gate

For M0.3 the analyzer requires the frozen full equivalence surface for every production candidate:

- 16 deterministic traces;
- 2,013,879 trace operations;
- 4,112 synthetic operations;
- FNV-1a trace fingerprint `6a4814a1551a9e5a`;
- the exact current `SEM-WORLD-SECTION-*` set carried by M0.3C full evidence.

The FNV value is an identity/checkpoint value, not a cryptographic digest. The correctness JSON file itself is bound into the decision record by SHA-256.

## Decision metrics

All metrics are lower-is-better.

### Per standard dimension

For each of Overworld, Nether and End independently:

- representative-population random-read p50;
- representative-population sequential-full-read p50;
- representative-population 4x4x4 volume-read p50;
- representative-population positive `maybe_contains` p50;
- representative-population negative `maybe_contains` p50;
- representative-population construction p99;
- process RSS loaded-minus-prefaulted-baseline median;
- deterministic total logical owned bytes over the dimension pack;
- deterministic maximum owned bytes of one section.

The analyzer also records, but does not currently use as strict Pareto axes:

- construction representation transitions;
- deterministic logical backing allocation events;
- final representation distribution.

Those remain decision diagnostics and complexity evidence. They are not silently converted into an arbitrary scalar cost.

### Global mechanism-stress metrics

The already-qualified synthetic mechanism suite contributes the same lower-is-better vector to every dimension frontier:

- p50 for same-state, low-entropy, high-entropy and palette-churn replacements over the frozen synthetic case surface;
- promotion p99 at 2, 3, 5, 9, 17, 33, 65, 129 and 257 live states.

Synthetic record identity is normalized by semantic workload/case/boundary. Candidate-specific representation names are retained as diagnostics but are not part of the cross-candidate metric key.

## No cross-dimension score

There is no numeric combination of Overworld, Nether and End in M0.3D.

A candidate may be dominated in one dimension and useful in another. Such a candidate is **not** globally rejected. A production candidate is classified as globally strictly dominated only when one other production candidate weakly dominates it on every decision axis in **each of all three standard dimensions**, with at least one strict improvement in every dimension comparison.

This rule deliberately prevents the accidental reintroduction of the previously rejected implicit 3:2:2 section-count weighting.

## Mathematical Pareto dominance

For candidate vectors `A` and `B` in one dimension:

- `A` weakly dominates `B` when `A <= B` for every comparable lower-is-better metric;
- dominance is strict when at least one metric is `<`;
- equality on every metric is not strict dominance.

The permanent `direct-reference` oracle may appear in diagnostic tables but is not a production candidate, is never selectable, and cannot by itself eliminate a production candidate.

## Material complexity threshold

Mathematical non-dominance is not sufficient justification for additional permanent machinery.

Relative to the simple production `direct` baseline, an optimized candidate records whether it demonstrates at least one material improvement of approximately:

- **5%** on a CPU/latency/tail metric; or
- **10%** on an RSS/logical-memory metric.

The calculation uses integer parts-per-million arithmetic; no floating-point rounding participates in the qualification record. The threshold is a complexity gate, not a scalar objective and not a license for regressions. Exact observed trade-offs remain visible in the Pareto tables.

A candidate that is Pareto-relevant but never clears a material threshold is retained in the engineering record as an experiment, but it does not automatically earn production complexity.

## Output analysis record

The analyzer emits a canonical `section-pareto-decision-analysis` record containing:

- exact source/target/toolchain/hardware/population identities inherited from the input evidence;
- SHA-256 identity of the combined orchestration, artifact manifest and each full correctness record;
- the exact decision metric registry and complexity thresholds;
- per-dimension candidate metric vectors;
- per-dimension pairwise strict-dominance relations and Pareto frontiers;
- global strictly-dominated candidates and their valid production dominators;
- candidates that survive because their trade-off is dimension-dependent;
- material-benefit diagnostics relative to `direct`;
- deterministic memory/allocation/representation diagnostics;
- unresolved selection blockers;
- a canonical analysis SHA-256.

The analysis does **not** automatically choose a winner. It can make the evidence selection-ready, but the final chosen policy/profile and rationale are a separate explicit record. This prevents a mathematical helper from silently defining product policy.

## Final selection record

A later `section-production-policy-decision` record must name the selected mechanism(s), profile semantics if more than one mechanism survives, and the exact analysis SHA-256 it relies on.

A default single mechanism should normally lie on the Pareto frontier in every standard dimension. If no common frontier candidate exists, the project must either retain a justified profile trade-off, gather stronger evidence, or explicitly admit a workload model before any cross-dimension weighting is introduced.

## Loser preservation

After selection, dominated/unjustified mechanisms may be removed from the normal engine build, but their knowledge record must remain:

- stable candidate ID;
- original hypothesis;
- representation/promotion design;
- correctness evidence;
- benchmark artifact identities;
- defects discovered while developing it;
- measured strengths/weaknesses;
- exact rejection rationale and dominating candidate(s), where applicable.

**Code can disappear; experimental knowledge cannot.**

## Regression obligations

The decision analyzer requires permanent tests for at least:

- accepting a fully self-consistent qualification fixture;
- rejecting smoke/ineligible evidence;
- root and nested digest/file corruption;
- target/source/binary/population identity drift;
- correctness commit/fingerprint/operation/SEM drift;
- missing or duplicate correctness candidates;
- accidental cross-dimension scoring;
- incomplete round protocol;
- deterministic memory evidence changing between repeated child records;
- normalized synthetic metric-set drift;
- equality vs strict dominance;
- all-dimension domination vs one-dimension domination;
- reference-only mechanisms never eliminating production candidates;
- exact 5% CPU and 10% memory materiality boundaries;
- canonical analysis digest stability.

No hosted timing values are permitted to become a production decision merely because this analyzer can parse them.