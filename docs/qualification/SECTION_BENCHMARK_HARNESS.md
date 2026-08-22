# Section benchmark harness — M0.3D

Parent: #19  
Status: **benchmark instrumentation; not yet a production-policy decision**

The section benchmark harness measures already-correct live block-section mechanisms. Correctness is
not traded against speed and is not inferred from benchmark output; M0.3C qualification remains a
separate gate.

## Command

Hosted smoke / harness validation:

```text
cargo run --release --locked \
  -p crucible-section-qualification \
  --bin section_bench -- \
  --smoke \
  --output target/crucible-qualification/section/benchmark-smoke.json
```

Target-hardware qualification:

```text
cargo run --release --locked \
  -p crucible-section-qualification \
  --bin section_bench -- \
  --qualification \
  --output target/crucible-qualification/section/benchmark.json
```

The harness refuses debug builds.

## Candidates

Every run includes:

- `direct-reference` — permanent correctness oracle, explicitly non-production;
- `direct` — direct 4096-state production candidate;
- `adaptive` — `Uniform -> Local4Stable -> Local8Stable -> DirectN`;
- `fast-local` — `Uniform -> Local8Stable -> DirectN`;
- `packed-local` — `Uniform -> packed 1..8-bit local -> DirectN`.

No candidate is selected merely because it exists in this list.

## Workloads

Prepared-section workloads currently measure:

- random point reads;
- sequential full-section reads;
- deterministic 4x4x4 volume reads;
- same-state replacement;
- low-entropy replacement;
- high-entropy replacement;
- dead/local-palette churn pressure;
- positive `maybe_contains`;
- negative `maybe_contains`.

Transition latency is measured independently as a **single replacement** from N-1 to N live states
at N = `2, 3, 5, 9, 17, 33, 65, 129, 257`. Those boundaries expose the packed-width ladder and the
16/17 and 256/257 local/direct transitions without burying spikes inside average write throughput.

Each timing record contains raw sample nanoseconds plus p50/p95/p99/max. The normalized p50 is stored
as integer picoseconds per operation rather than a floating-point estimate. Units are explicit: cell
read, full section scan, 4x4x4 query, replacement, contains query, or single transition replacement.

## Cardinality and spatial cases

The qualification mode includes the required distinct-state pool matrix:

`1, 2, 4, 8, 16, 17, 32, 64, 128, 256, 257, 1024, 4096`.

It also includes independent spatial cases for homogeneous, layered, clustered, checker, noisy,
survival-like mostly-base, and build-like locally varied sections. Every memory **and timing** record
distinguishes the allowed state pool from deterministic actual live cardinality; spatial cases need
not consume every state in their pool.

Synthetic cases are **not sufficient to freeze adaptive thresholds**. A real vanilla-derived section
corpus is still required before the #19 decision artifact can be accepted.

## Memory accounting

The harness records deterministic object-plus-owned-backing bytes for every prepared representation,
excluding allocator metadata. Uniform candidates therefore demonstrate zero 4096-cell backing
allocation through the representation's existing diagnostics.

Process/RSS and allocation-count qualification remain a separate #19 slice. We will not add a custom
allocator to the production engine merely to count benchmark allocations.

## Reproducibility metadata

Every artifact records:

- commit SHA;
- Minecraft/protocol/data versions;
- generated state-data input and generation digests;
- benchmark harness version and deterministic seed;
- Rust verbose version;
- target triple;
- release/codegen policy;
- CPU model;
- OS/kernel;
- CPU0 governor when exposed by the host;
- warmup/sample parameters;
- raw timing samples.

## Hosted-runner rule

`Section Benchmark Smoke` exists only to prove the release harness compiles, runs, and emits a valid
artifact. GitHub-hosted timing numbers are **diagnostic and non-qualifying**. They must not be copied
into the production-policy decision or used to freeze thresholds.

The final #19 decision requires controlled target-hardware runs, noise/confidence context, the real
vanilla-derived corpus, process-scale memory evidence where practical, Pareto tables, and explicit
rejection/selection rationale under the issue's material-benefit threshold.