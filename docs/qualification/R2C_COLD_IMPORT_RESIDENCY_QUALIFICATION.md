# R2C Cold Import → Residency Qualification

Status: **reference baseline; production performance not admitted**.

This qualification covers the first complete block-state transaction from already-read Anvil region bytes to a live Helve resident chunk. It exists to prevent R2C.3 persistence work and the R2C.2 resident-world substrate from being evaluated only as isolated micro-components.

## Boundary under test

The measured production path is:

```text
bounded Anvil region bytes
  -> RegionView slot framing
  -> bounded zlib payload decode
  -> schema-directed NBT / block-state decode
  -> exact Minecraft 26.2 persisted-state resolution
  -> final semantic section construction
  -> helve-world-load sparse-to-contiguous composition
  -> DimensionInstance resident admission
```

Disk I/O, path lookup, OS page-cache behaviour, fixture compression, semantic verification and unload are outside the measured transaction.

The baseline keeps decompression scratch and section-decode scratch alive across samples. Per-sample timing therefore describes steady loading-session service cost rather than repeatedly charging one-time loader construction.

## Reference section mechanism

The baseline intentionally builds `DirectBlockSection<BlockStateId>` using `GeneratedStateFacts`.

That choice is a correctness oracle only. It does **not** select the R2C production section representation and is not eligible evidence for M0.3D section-policy selection. The emitted artifact must retain:

```text
reference_section_builder = true
production_section_policy_selected = false
performance_admitted = false
```

The reference builder exposes structural counters for uniform section construction, dense section construction and dense cell writes so transparent-oracle materialization cannot be mistaken for parser or resident-install overhead.

A future production section candidate may reuse this transaction harness only after its independent section qualification remains valid. Its result must be compared as a new mechanism candidate rather than silently replacing the reference baseline.

## Qualification fixture

The repository smoke fixture is deterministic and built outside all timers:

- target: Minecraft Java 26.2 / data version 4903;
- chunk position: `(0, 0)`;
- one stored section at logical section Y `0`;
- uniform `minecraft:stone` resolved through `Target262BlockStateResolver`;
- zlib Anvil compression identity;
- one bounded inline Anvil record;
- selected resident dimension lattice: minimum block Y `-64`, height `384`, skylight enabled.

Keeping the complete 24-section resident lattice is intentional. The stored fixture has one block-bearing section, so the installation seam must also exercise caller-owned empty-section synthesis for omitted vertical slots.

The synthetic fixture is not a substitute for the real-save differential corpus in R2C.3. Its purpose is stable repeated transaction measurement after semantic import correctness has already been independently established.

## Timing domains

Every measured round records three independent service-time samples:

1. `import_ns` — region slot through fully decoded final semantic sections;
2. `install_ns` — an already imported sparse section set through live resident admission;
3. `combined_ns` — a fresh uninterrupted import plus resident-install transaction.

The split import and install timings use one transaction, then the combined timing uses a separate fresh transaction. Correctness checks and unload occur after the corresponding timer stops.

The report retains raw samples and p50 / p95 / p99 / p99.9 / maximum summaries. Small hosted smoke sample counts exist to prove the evidence pipeline and percentile ordering only; they are not statistically sufficient production tail evidence.

## Permanent correctness gates

Every sample must preserve:

- exact stored chunk identity;
- exact target 26.2 block-state identity at a resident read point;
- resident section-summary masks equal to independent recomputation;
- no chunk left resident after verification/unload;
- bounded decoder and parser contracts;
- exact workspace dependency/lock identity;
- no production section-policy claim.

The dedicated CI workflow additionally requires:

- repository dependency policy;
- fresh empty `CARGO_HOME` with `--offline --locked` package build;
- rustfmt;
- strict Clippy with `-D warnings`;
- semantic unit tests;
- release-mode hosted smoke;
- evidence-shape and monotone-percentile assertions;
- rustdoc with `-D warnings`.

## Hosted versus target-hardware evidence

GitHub-hosted timing is diagnostic only. A green hosted smoke proves that the optimized transaction executes repeatedly and emits coherent evidence; it does not establish a production latency threshold or choose a mechanism.

Any production performance admission must use the repository's target-hardware evidence law:

- explicit single-CPU affinity where required by the benchmark protocol;
- exact commit, Rust toolchain and hardware provenance;
- multiple independent process runs;
- raw artifacts retained before aggregation;
- cross-process tail and variability analysis;
- human-reviewed admission decision;
- no threshold invented after looking at one favorable run.

The first target-hardware baseline for this combined cold path is still pending. Until it is recorded and reviewed, `performance_admitted` remains false.

## Non-scope

This qualification does not yet measure or claim:

- file-system / disk I/O throughput or page-cache behaviour;
- multi-region or whole-world loading;
- biome persistence/import;
- heightmap state;
- sky/block lighting state;
- block entities;
- world generation;
- chunk/light network projection;
- production section storage selection;
- R2C world-ready latency.

Those concerns must join the whole-cost qualification progressively as their source-backed semantics and production mechanisms become admitted.
