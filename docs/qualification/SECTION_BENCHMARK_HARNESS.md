# Section benchmark harness — M0.3D

Parent: #19  
Status: **benchmark instrumentation; real corpus boundary admitted; production policy not yet selected**

The section benchmark harness measures already-correct live block-section mechanisms. Correctness is not traded against speed and is not inferred from benchmark output; M0.3C qualification remains a separate prerequisite.

Canonical companion records:
- [`EVIDENCE_AND_EXPERIMENT_RECORDS.md`](EVIDENCE_AND_EXPERIMENT_RECORDS.md);
- [`section/SECTION_CANDIDATE_REGISTRY.md`](section/SECTION_CANDIDATE_REGISTRY.md);
- [`section/SECTION_EXPERIMENT_LOG.md`](section/SECTION_EXPERIMENT_LOG.md);
- [`SECTION_VANILLA_CORPUS.md`](SECTION_VANILLA_CORPUS.md);
- [`section/SECTION_REAL_CORPUS_ADMISSION.md`](section/SECTION_REAL_CORPUS_ADMISSION.md).

## Command

Hosted smoke / harness validation:

```text
cargo run --release --locked \
  -p crucible-section-qualification \
  --bin section_bench -- \
  --smoke \
  --output target/crucible-qualification/section/benchmark-smoke.json
```

Controlled target-hardware measurement:

```text
cargo run --release --locked \
  -p crucible-section-qualification \
  --bin section_bench -- \
  --qualification \
  --output target/crucible-qualification/section/benchmark.json
```

The harness refuses debug builds.

`--qualification` means “full benchmark sampling settings”; it does **not** by itself make the resulting timing production-qualified. The hardware/run protocol and representative-corpus gate still apply.

## Harness architecture

The binary is split into small modules so benchmark semantics can be regression-tested independently:
- `model` — candidate adapters, settings, records and logical allocation model;
- `workloads` — state selection, spatial construction, actual-cardinality and contains needles;
- `measure` — timing/lifetime execution and promotion tails;
- `hardware` — machine/toolchain/environment provenance;
- `report` — deterministic machine-readable evidence serialization;
- `main` — CLI/release enforcement only.

The next M0.3D slice adds a dedicated `corpus` boundary module rather than mixing untrusted corpus parsing into synthetic workload construction.

Benchmark observability does not add fields, locks, counters, trait objects, or other instrumentation to production section structs.

## Candidates

Every run includes:

- `direct-reference` — permanent correctness oracle, explicitly non-production;
- `direct` — direct 4096-state production candidate;
- `adaptive` — `Uniform -> Local4Stable -> Local8Stable -> DirectN`;
- `fast-local` — `Uniform -> Local8Stable -> DirectN`;
- `packed-local` — `Uniform -> packed 1..8-bit local -> DirectN`.

No candidate is selected merely because it exists in this list.

## Workloads

Prepared-section workloads measure:

- random point reads;
- sequential full-section reads;
- deterministic 4×4×4 volume reads;
- same-state replacement;
- low-entropy replacement;
- high-entropy replacement;
- dead/local-palette churn pressure;
- positive `maybe_contains`;
- negative `maybe_contains`.

Synthetic spatial classes include:
- cardinality spread;
- homogeneous;
- layered;
- clustered;
- checker;
- noisy high entropy;
- explicit fluid-containing;
- survival-like air/solid terrain;
- build-like local variation.

### Semantic workload rules

The harness uses actual generated Minecraft 26.2 `BlockStateId` and `GeneratedStateFacts`.

Generic cardinality experiments use deterministic distinct target IDs. Semantic spatial classes select by qualified facts rather than relying on magic names/IDs:
- survival-like pools contain AIR plus states whose mutation flags are exactly non-air/non-fluid/non-random;
- fluid-containing pools explicitly include a target state with the counted-fluid flag;
- positive membership needles are selected only from the **observed final semantic image**;
- negative membership needles are selected from the target universe but outside the observed final image.

These rules have dedicated regressions.

## Cardinality

Qualification mode includes:

`1, 2, 4, 8, 16, 17, 32, 64, 128, 256, 257, 1024, 4096`.

Every timing and memory record distinguishes:
- `pool_cardinality` — allowed synthetic state pool;
- `actual_cardinality` — states actually present in the final 4096-cell semantic image.

The latter is computed from construction rather than copied from the requested pool. Regression tests independently rescan final sections to verify this invariant.

## Transition/tail measurement

Transition latency is measured separately as a **single replacement** from N−1 to N live states at:

`2, 3, 5, 9, 17, 33, 65, 129, 257`.

These points expose packed-width growth and the 16/17 and 256/257 local/direct boundaries without burying spikes inside average write throughput.

Timing records retain raw sample nanoseconds plus p50/p95/p99/max. Normalized p50 is stored as integer **picoseconds per operation**, avoiding floating-point precision/serialization ambiguity.

## Memory and lifetime accounting

The harness records deterministic object-plus-owned-backing bytes for every prepared representation, excluding allocator metadata.

It also records a representative post-construction mutation lifetime:
- representation transitions;
- deterministic **logical backing allocation events** implied by the current mechanism implementation;
- peak owned bytes;
- final owned bytes and representation.

Logical allocation events are deliberately named as such: they count known backing allocation operations implied by representation construction/transitions; they are not a claim about allocator internals, RSS pages, or malloc implementation behavior.

Uniform local candidates begin with zero 4096-cell backing allocation. Direct/reference candidates begin with their direct Box backing.

Process/RSS measurement remains a separate #19 qualification slice. We will not add a custom allocator to production simply to count benchmark allocations.

## Reproducibility metadata

Every artifact records:

- exact commit SHA;
- Minecraft/protocol/data versions;
- generated state-data input and generation digests;
- benchmark harness version/schema and deterministic seed;
- Rust verbose version;
- target triple;
- release/codegen policy;
- `RUSTFLAGS` and `CARGO_ENCODED_RUSTFLAGS`;
- CPU model;
- OS/kernel;
- process CPU affinity (`Cpus_allowed_list`);
- CPU governor when exposed;
- current/min/max cpufreq values when exposed;
- Intel no-turbo state when exposed;
- load average;
- warmup/sample/operation parameters;
- raw timing samples.

Target-hardware qualification should pin CPU affinity externally where practical; the artifact records the resulting allowed set so an unpinned run cannot masquerade as pinned.

## Regression policy

The benchmark harness is itself qualification-critical software. Bugs in benchmark construction can select the wrong production mechanism, so material benchmark bugs receive permanent tests.

Current explicit regressions cover:
- observed cardinality equals an independent rescan of the final image;
- positive membership needle is actually present;
- fluid workload contains a qualified counted-fluid state;
- survival workload contains AIR and only qualified plain-solid non-air states otherwise;
- integer timing normalization avoids float precision loss;
- packed width transitions remain represented as logical allocation events;
- packed first-width growth installs the requested state (in addition to the core representation regression);
- report JSON escaping;
- benchmark metadata has basic immutable identity fields.

The release-only packed widening defect itself remains documented in the candidate registry and experiment log and is permanently regression-tested in the section implementation.

The real-corpus importer adds another explicit regression family: canonical format parsing, target/digest binding, state-range validation, 4096-cell section completeness, strict ordering/duplicate rejection, corpus-purpose policy, deterministic section selection and candidate image equivalence.

## Hosted-runner rule

`Section Benchmark Smoke` proves only that the release harness compiles, runs, emits a valid artifact, includes all candidates/workload classes, and satisfies structural invariants.

GitHub-hosted timing numbers are **diagnostic and non-qualifying**. They must not be copied into the production-policy decision or used to freeze thresholds.

## Real corpus evidence

The normalized `CRUCIBLE-SECTION-CORPUS/1` boundary and a real official 26.2 extractor are now admitted.

The first hosted real-target admission used a deterministic official spawn world and produced:
- 12,696 stored Overworld sections;
- 52,002,816 cells;
- 81 distinct state IDs;
- corpus SHA-256 `8f1b623f4cd323ff8072c3c2722f96190dfe49b624ae65cf612f1e5ba785febf`.

That corpus is **parser-admission evidence**, not representative production weighting. It contains 12,452 all-air sections and 12,453 cardinality-1 sections, so treating it as a final workload distribution would strongly bias the decision toward Uniform-heavy behavior.

Corpus evidence therefore has two independent questions:

1. **Is this corpus structurally/semantically admitted?**
2. **Is this corpus sampling/weighting policy eligible to influence production selection?**

A corpus may pass (1) while failing (2). The benchmark importer must preserve that distinction mechanically rather than relying on human memory.

## Remaining production-decision gates

Two major evidence layers remain before policy freeze.

### 1. Representative vanilla-derived workload policy

The save/corpus path itself is now admitted, but the parser-admission spawn corpus is not representative enough to freeze thresholds. We still require an explicit policy large/broad enough to reveal realistic:
- cardinality distributions;
- homogeneous/low-entropy frequency;
- air/solid/fluid mixtures;
- spatial locality;
- high-entropy outliers;
- standard-dimension differences;
- semantically present all-air sections omitted by stored-section-only serialization.

The final policy must record sampling and weighting rather than silently treating every serialized section as equally representative.

### 2. Controlled process/RSS and target-hardware qualification

The final #19 decision requires controlled runs on target hardware, noise/confidence context, process-scale memory/RSS evidence where practical, corpus-weighted Pareto tables, and explicit rejection/selection rationale.

Only then can a mechanism/profile be frozen and losing implementation code deleted.
