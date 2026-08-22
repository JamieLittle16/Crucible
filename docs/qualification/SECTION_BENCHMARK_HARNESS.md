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

## Commands

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

Strict real-corpus import/reconstruction:

```text
cargo run --release --locked \
  -p crucible-section-qualification \
  --bin section_bench -- \
  --corpus-check CORPUS \
  --output corpus-import.json
```

Production-decision eligibility gate:

```text
cargo run --release --locked \
  -p crucible-section-qualification \
  --bin section_bench -- \
  --corpus-decision-check CORPUS
```

The currently admitted spawn corpus must fail the last command because its extraction policy is parser-admission evidence, not representative weighting evidence.

The harness refuses debug builds. `--qualification` means “full benchmark sampling settings”; it does **not** by itself make the resulting timing production-qualified. The hardware/run protocol and representative-corpus gate still apply.

## Harness architecture

The binary is split into small modules so benchmark semantics can be regression-tested independently:
- `model` — candidate adapters, typed representation identities, settings, records and logical allocation model;
- `workloads` — state selection, spatial construction, actual-cardinality and contains needles;
- `measure` — timing/lifetime execution and promotion tails;
- `hardware` — machine/toolchain/environment provenance;
- `report` — deterministic synthetic-benchmark evidence serialization;
- `corpus/parser` — fail-closed consumer of untrusted canonical corpus bytes;
- `corpus/verify` — independent semantic reconstruction and aggregate diagnostics;
- `corpus` — corpus-purpose policy and machine-readable import evidence;
- `main` — CLI/release enforcement only.

The corpus importer uses one authoritative streaming pass over section bodies. Each section is parsed once, contributes metadata once, is reconstructed through all five mechanisms, verified, and discarded. Whole-corpus cells are never retained in memory. Parser-admission/unknown policy is checked before expensive reconstruction in decision mode.

The parser reuses its section-line buffer, computes per-section cardinality with a fixed target-state bitset, and parses state IDs directly rather than building token trees or sets. Representation-transition bookkeeping uses the typed `RepresentationCode` rather than allocating strings on each mutation; text is produced only for evidence output.

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

## Real-corpus semantic reconstruction

`--corpus-check` is not a timing shortcut. It is a correctness boundary before corpus timing is allowed.

For each imported 4096-cell image it:
1. validates canonical target/provenance/order/state-ID structure;
2. recomputes the expected `SectionSummary` directly from frozen generated 26.2 facts;
3. reconstructs direct-reference, direct, adaptive, fast-local and packed-local storage;
4. rereads all 4096 cells from every candidate and requires exact state identity;
5. requires every candidate's maintained non-air/fluid/random-block/random-fluid summary to equal the independently recomputed summary;
6. only then records final representation, owned bytes, transitions and logical backing-allocation diagnostics.

Cells equal to the section's initial fill state need not receive redundant replacement calls during image reconstruction. This is intentionally an image-reconstruction optimization, not a substitute for same-state mutation qualification; same-state replacement behavior remains covered by M0.3C and the synthetic benchmark regressions.

## Reproducibility metadata

Every synthetic benchmark artifact records:

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

Corpus import evidence records the exact target generation and input digests, source-inventory digest, extractor/purpose identity, decision eligibility, section/cell/state statistics and per-candidate representation/memory/transition aggregates. The canonical corpus SHA-256 remains independently recomputed by `tools/section_corpus.py` and is bound to the Rust result through the workflow evidence chain rather than adding a hashing dependency to the qualification binary.

Target-hardware qualification should pin CPU affinity externally where practical; the artifact records the resulting allowed set so an unpinned run cannot masquerade as pinned.

## Regression policy

The benchmark harness is itself qualification-critical software. Bugs in benchmark construction can select the wrong production mechanism, so material benchmark bugs receive permanent tests.

Synthetic-harness regressions cover observed cardinality, membership needles, target semantic state classes, timing normalization, allocation-transition accounting, packed widening, JSON escaping and immutable benchmark identity metadata.

The real-corpus importer adds a heavy fail-closed family covering:
- target Minecraft/protocol/data/state-generation drift;
- canonical LF/final-newline/no-blank-line rules;
- source kind/inventory/extractor syntax;
- parser-admission and unknown-policy decision rejection;
- resource locations and canonical signed coordinates;
- strict section ordering and duplicate rejection;
- 4095/4097-cell rejection;
- noncanonical and out-of-range state IDs;
- exact cell order and independently observed cardinality;
- empty-corpus rejection;
- aggregate metadata across multiple sections;
- exact reconstruction through all five candidates;
- 17-state transition-boundary reconstruction;
- exact maintained summary equivalence;
- real generated 26.2 states spanning air, solid, counted-fluid, random-block and random-fluid fact classes.

The Python↔Rust evidence checker is separately unit-tested against target drift, provenance/purpose drift, corpus summary disagreement, missing/duplicate/wrongly-classified candidates, representation-count mismatches and impossible memory totals.

The release-only packed widening defect itself remains documented in the candidate registry and experiment log and is permanently regression-tested in the section implementation.

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

`Section Corpus Probe` now regenerates that class of official-world evidence from the pinned server, independently validates it in Python, reconstructs the exact corpus through the Rust importer, requires `--corpus-decision-check` to reject it, and cross-checks Python/Rust target/provenance/cardinality/dimension/candidate evidence before retaining the artifact.

Corpus evidence therefore has two independent questions:

1. **Is this corpus structurally/semantically admitted?**
2. **Is this corpus sampling/weighting policy eligible to influence production selection?**

A corpus may pass (1) while failing (2). This distinction is mechanically enforced.

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
