# Section Representation Laboratory

Status: **M0.3D active. Correctness-qualified candidate set frozen for performance measurement; no production default selected.**

Parent issues: #16, #17, #18, #19.

Canonical companion records:
- [`EVIDENCE_AND_EXPERIMENT_RECORDS.md`](EVIDENCE_AND_EXPERIMENT_RECORDS.md) — project-wide evidence/decision discipline;
- [`section/SECTION_CANDIDATE_REGISTRY.md`](section/SECTION_CANDIDATE_REGISTRY.md) — durable record of active, superseded, rejected, and deferred mechanisms;
- [`section/SECTION_EXPERIMENT_LOG.md`](section/SECTION_EXPERIMENT_LOG.md) — chronological section laboratory notebook;
- [`SECTION_BENCHMARK_HARNESS.md`](SECTION_BENCHMARK_HARNESS.md) — M0.3D benchmark protocol.

## Governing model

The section project follows:

```text
OFFICIAL MINECRAFT 26.2 SOURCE / RUNTIME
                 ↓
            SEMANTIC RULES
                 ↓
       DIRECT CRUCIBLE REFERENCE
                 ↓
     OPTIMIZED REPRESENTATION SET
                 ↓
       DIFFERENTIAL QUALIFICATION
                 ↓
        PERFORMANCE LABORATORY
                 ↓
       COMMITTED POLICY DECISION
```

Correctness and performance are separate gates. A semantically failing candidate is not a slower candidate; it is ineligible.

## Frozen target substrate

M0.3A is complete for Minecraft Java 26.2:
- protocol 776;
- data version 4903;
- 32,366 dense block states;
- AIR = state ID 0;
- target identity mapping is vanilla identity;
- narrowest safe current generated state representation is `u16`;
- generated HOT mutation facts cover non-air, counted-fluid, random-block, and random-fluid semantics.

Generated state-data digest:
`79e5803347d6fb6f7ffccea4cef783998a1c6469ed869d26fa48ab5f2328cd3b`.

Production section mutation therefore does not require a global registry or Java-style object traversal to update exact section summaries.

## Correctness-qualified production candidates

The admitted M0.3D set contains four deliberately distinct hypotheses.

### Direct production storage

`DirectN`

- direct semantic state ID for every cell;
- current direct cell payload is 8192 bytes (`4096 × u16`);
- serves as the simple production CPU/memory baseline, distinct from the correctness reference.

### Adaptive stable local hierarchy

`Uniform -> Local4Stable -> Local8Stable -> DirectN`

- Uniform: no 4096-cell backing;
- Local4: 2048 bytes of 4-bit stable local indices plus palette;
- Local8: 4096 bytes of byte local indices plus palette;
- DirectN: direct semantic state IDs.

Stable palette slots carry usage counts. Dead slots are reusable without renumbering live cells. Promotion is based on simultaneously live cardinality, not historical palette allocation.

### Fast-local hierarchy

`Uniform -> Local8Stable -> DirectN`

This tests whether removing the Local4 stage buys enough mutation/transition simplicity to justify spending more bytes at low cardinality.

### Packed memory-frontier hierarchy

`Uniform -> Packed(1..8) -> DirectN`

This tests the opposite frontier: minimum local index width at the price of bit extraction and explicit O(4096) width transitions.

No candidate is selected merely because it is implemented.

## Superseded mechanism: append-only palette cardinality

An earlier stable-palette shape allowed dead entries to accumulate and used allocated palette length as capacity state. That was rejected before performance selection because historical allocation is not simultaneous semantic cardinality. Churn could therefore cause premature promotion.

The corrected stable-slot design maintains exact usage counts and reuses zero-use/last-use slots.

The implementation may disappear; the experiment remains permanently documented in the candidate registry.

## M0.3C qualification state

The production candidate set has passed deterministic differential qualification against the permanent `DirectBlockSection` oracle and source/runtime-backed target evidence.

Qualification covers:
- all-air stability;
- mutation/reversal;
- localized churn;
- random/high-entropy writes;
- dead palette churn;
- exact 15/16/17 and 255/256/257 boundaries;
- all 16 synthetic mutation-fact combinations;
- previous-state/readback semantics;
- exact maintained summaries;
- independent full 4096-cell summary recomputation at deterministic barriers;
- `maybe_contains` no-false-negative behavior;
- clone independence;
- target fact-table consistency;
- source/runtime semantic fixtures.

Full release qualification per candidate currently records:
- 16 deterministic traces;
- 2,013,879 target trace operations;
- 4,112 synthetic operations;
- eight long seeds contributing 2,000,000 mutations;
- common trace fingerprint `6a4814a1551a9e5a`.

### Important release-only defect found

The first full release matrix exposed a real packed-local bug: a pending first-width-growth mutation was executed only inside `debug_assert_eq!`, so optimized release compilation removed the side effect. The failure occurred in `localized-churn`, seed `0x10ca11ced00d`, operation 6.

The mutation now executes unconditionally, the invariant is checked separately, and a focused first-widen regression is retained. All candidates pass the full release suite after the fix.

This is a useful validation of the qualification architecture itself: the larger release gate found a defect that debug tests did not.

## Current M0.3D performance questions

Performance work must answer at least:
- random point read cost;
- sequential full-section scan cost;
- small spatial/volume read cost;
- same-state replacement cost;
- low-entropy replacement cost;
- high-entropy replacement cost;
- palette churn cost;
- positive/negative `maybe_contains` cost;
- representation-transition p50/p95/p99/max latency;
- deterministic owned bytes;
- representative lifetime allocation behavior;
- corpus-scale process/RSS behavior where practical.

Required cardinalities include:
`1, 2, 4, 8, 16, 17, 32, 64, 128, 256, 257, 1024, 4096`.

Synthetic spatial workloads must include at least homogeneous, layered, clustered, checker/noisy high entropy, fluid-containing, survival-like, and build-like distributions.

## Benchmark harness audit status

A dependency-light release benchmark harness exists and has been transplanted cleanly onto the current M0.3D branch. It is **instrumentation, not yet decision evidence**.

Before trusting its qualification numbers, the following audit gaps are being closed:
1. add explicit fluid-containing workloads;
2. make survival-like state selection semantically grounded rather than merely ID-biased;
3. guarantee positive membership queries actually target a present state;
4. record observed actual cardinality rather than requested pool cardinality;
5. add representative lifetime allocation-event accounting without contaminating production structs;
6. strengthen hardware/affinity/frequency/codegen metadata;
7. ingest a real vanilla-derived section corpus before any adaptive threshold is frozen.

Regression tests should accompany each corrected benchmark semantic so the harness itself is qualified as carefully as the mechanisms it measures.

## Real-corpus gate

Synthetic curves are necessary but insufficient. Before the first production policy is frozen, #19 requires a vanilla-derived section corpus large enough to expose realistic cardinality/spatial distributions.

The decision must therefore combine controlled synthetic experiments with corpus-weighted whole-workload evidence.

## Selection/deletion rule

After controlled target-hardware qualification:
1. remove correctness failures;
2. remove mechanisms strictly dominated across official workload classes;
3. retain multiple production mechanisms only for a real Pareto/profile trade-off;
4. reject permanent complexity whose gain is inside benchmark noise;
5. commit a decision record with raw artifact digests and rejection rationale;
6. delete losing implementations from production builds while retaining their candidate/experiment records.

A useful default complexity threshold is approximately >=5% CPU/latency or >=10% resident-byte benefit on a relevant official workload class, with noise/confidence and regressions considered explicitly rather than treating those values as universal constants.

## Explicitly deferred hypotheses

Do not silently add these to the current candidate set:
- thermal/demotion switching;
- alternate Local8 state-to-slot lookup;
- custom allocator/slab/arena policy;
- SIMD;
- unsafe indexing;
- lock-free storage;
- sparse base+exceptions;
- COW publication;
- worldgen-specific builder.

Each becomes a new candidate only when evidence identifies a distinct material hypothesis worth testing.

The objective of M0.3 is not to invent the cleverest representation. It is to leave Crucible with the **simplest strict-fidelity mechanism or profile set that survives adversarial correctness qualification and a reproducible real-workload Pareto decision**.