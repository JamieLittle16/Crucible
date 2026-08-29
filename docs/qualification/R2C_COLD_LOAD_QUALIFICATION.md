# R2C cold-load qualification

Status: **qualification baseline; no production timing decision**.

This qualification measures and structurally audits the complete block-state cold-load seam already assembled by R2C:

```text
already-bounded region bytes
  -> RegionView framing validation
  -> bounded zlib decompression
  -> bounded schema-directed NBT decode
  -> exact Minecraft 26.2 persisted-state resolution
  -> final block-section construction
  -> sparse stored sections -> contiguous dimension lattice
  -> DimensionInstance resident admission
```

Filesystem I/O is deliberately outside this benchmark. Disk/page-cache behaviour is a separate storage-system experiment and must not be mixed with CPU/mechanism evidence for decode and residency.

## Why this gate exists now

R2C has individually qualified region/NBT import, compression, persisted-state resolution, semantic differential correctness, and resident lifecycle. Before biome/light/height/block-entity state makes a resident chunk larger, this gate asks a narrower engineering question:

> What work, copies, retained scratch and tail cost does the **whole currently assembled block cold path** actually perform?

This is intended to catch architectural tax while the seam is still easy to change.

## Deterministic workload

The benchmark embeds one content-addressed target-26.2 NBT fixture and its exact zlib representation:

- DataVersion: `4903`;
- chunk: `(0, 0)`;
- Overworld-height lattice: section Y `-4..19` (24 logical sections);
- 24 stored section compounds;
- 18 block-bearing stored sections;
- 12 uniform block sections;
- 6 non-uniform three-state packed sections;
- 6 persisted sections with no block-state payload, becoming semantic air at resident composition;
- decompressed NBT: 13,848 bytes;
- zlib payload: 367 bytes;
- decompressed SHA-256: `34060101891b2869f1518a7fb52a74ee3d5aa7b49146c470343966f00b25be53`;
- compressed SHA-256: `f36e5862f610a84a7007046c46ade1956dec1c449375c59579d6af946bbc11ac`.

The semantic validation step independently checks representative missing/uniform/non-uniform cells, exact 24-section residency, mask recomputation and complete unload before any timed samples are accepted.

## Section mechanism policy

The benchmark intentionally uses a transparent qualification-only dense final section.

It **does not select a production block-section mechanism**. `helve-world-section` candidates remain mechanism-laboratory entries until the section tournament selects a production policy.

This choice makes current data movement explicit. In the baseline workload the generic importer currently exposes:

- 6 non-uniform sections through a reusable 4,096-state semantic scratch;
- 4,096 semantic state copies into the transparent final section for each such section;
- therefore **24,576 explicit semantic-cell copies per load** at this builder boundary;
- one final 24-slot resident section-vector allocation;
- 6 caller-created empty final sections for omitted block state;
- one reusable decompression output buffer retained at the selected 16 KiB bound;
- reusable palette, packed-word and 4,096-cell decode scratch across loads.

The 24,576 figure is a **baseline structural witness**, not an accepted permanent cost. If whole-load evidence shows it is material, the next mechanism experiment should test a builder that consumes/streams decode scratch or constructs an optimized final section directly. That mechanism must preserve the exact same semantic/differential gates.

## Timing split

Each measured iteration reports:

1. `import` — region validation + decompression + exact state decode + final section construction;
2. `install` — sparse-to-contiguous composition + resident admission;
3. `total` — start of framing through successful resident admission;
4. `unload_drop` — exact-generation removal and release of the resident chunk after the load timer.

The reusable decoder, decode scratch, builder object, region byte buffer, and `DimensionInstance` survive warm-up and measured rounds. The successful resident generation is unloaded between rounds, so measured samples represent repeated cold chunk loading rather than one-time harness setup.

## Hosted CI evidence

`.github/workflows/r2c-cold-load-qualification.yml` performs:

- dependency-policy validation;
- targeted rustfmt and strict Clippy;
- semantic/structural Rust tests;
- one release build;
- three independent process invocations pinned to one allowed logical CPU;
- 20 measured samples per hosted smoke process;
- a reduced full-mode structural run;
- seven-day retention of all raw JSON artifacts.

Hosted timings are diagnostic only. CI requires:

```text
hosted_ci_is_diagnostic_only = true
timing_threshold_selected = false
performance_admitted = false
production_section_policy_selected = false
```

No pull request is allowed to claim a production latency/throughput win from GitHub-hosted timings.

## Structural acceptance

For the frozen fixture every measured process must agree on:

```text
stored sections                    = 24
imported block sections            = 18
uniform final builds               = 12
non-uniform final builds           = 6
synthesized empty sections         = 6
resident sections                  = 24
dense semantic-cell copies/load    = 24,576
resident column Vec allocations    = 1
```

The decoder output buffer must already be retained at the selected 16 KiB bound, and decode scratch must retain enough capacity for the observed three-entry palette, 256 packed words and 4,096 semantic cells.

Timing samples must be positive and percentile summaries monotone. These shape checks are gates; timing magnitude is not.

## What this does not prove

This gate does not prove:

- filesystem or page-cache performance;
- world-save/writeback performance;
- biome, heightmap, light or block-entity import cost;
- client projection cost;
- movement-driven demand/interest cost;
- production section-policy selection;
- target-hardware timing admission;
- that the current 16 KiB decompression high-water policy is memory-optimal;
- that the current 24,576 dense semantic-cell copies should survive the mechanism tournament.

## Next evidence steps

After the hosted harness is stable:

1. run the full benchmark on controlled target hardware using the same explicit hosted -> target-run -> cross-process -> human-decision evidence law used by resident-world qualification;
2. compare section candidates through this **whole-load** harness, not only isolated section microbenchmarks;
3. experiment with consuming/streaming non-uniform decode output if the copy witness is material;
4. extend the workload only after biome/height/light/block-entity source law is admitted, keeping each added state component separately attributable.

The objective is not to minimize one microbenchmark. It is to keep the eventual R2C world-load path simple, bounded, semantically exact and free of data movement that does not buy a measurable end-to-end advantage.
