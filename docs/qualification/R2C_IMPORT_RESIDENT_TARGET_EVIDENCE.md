# R2C Import → Resident Target-Hardware Evidence

Status: **implemented evidence collection/combination tooling; no production performance admission**  
Parent qualification: [`R2C_IMPORT_RESIDENT_QUALIFICATION.md`](R2C_IMPORT_RESIDENT_QUALIFICATION.md)  
Performance law: [`PERFORMANCE_QUALIFICATION_STANDARD.md`](PERFORMANCE_QUALIFICATION_STANDARD.md)

## Purpose

The hosted R2C import → resident workflow proves the complete mechanism executes correctly on a
genuine pinned Minecraft 26.2 save and provides useful diagnostic decomposition. Hosted runners are
not stable enough to select a production mechanism or numerical threshold.

This document defines the explicit operator path from that hosted diagnostic to controlled-hardware
cross-process evidence.

The tools are:

```text
tools/r2c_import_resident_target_run.py
tools/r2c_import_resident_evidence.py
```

The raw Rust benchmark deliberately remains conservative:

```text
mode = hosted-diagnostic
production_decision_eligible = false
```

The target runner does **not** rewrite those fields. Instead it adds a separate
`target_qualification` witness proving that an operator deliberately ran the benchmark under the
required affinity and world-identity controls.

## Required world path

The runner accepts the exact Minecraft 26.2 **dimension root**, not the save root. For the standard
overworld this is:

```text
<save>/dimensions/minecraft/overworld
```

and it must contain:

```text
region/r.<x>.<z>.mca
```

plus any external `region/c.<x>.<z>.mcc` payloads required by those region records.

Legacy root-level `region/` is not the 26.2 standard-dimension layout and must not be substituted.

## Exact input-world identity

Before running the benchmark, the target wrapper computes a deterministic SHA-256 over the complete
selected dimension-region byte set:

- all sorted `r.*.*.mca` members;
- all sorted `c.*.*.mcc` members;
- canonical path names relative to the dimension root;
- exact file lengths;
- exact file bytes.

Files are streamed into the digest; the wrapper does not allocate one whole region file merely to
identify the evidence input.

After the benchmark exits, the wrapper computes the same identity again. Any difference rejects the
run before a target artifact can be written. This closes the obvious time-of-check/time-of-use hole:
a save mutated during measurement cannot later masquerade as the pre-run world identity.

Symlinked or non-regular evidence members are rejected.

## Running one target process

Choose one logical CPU that is appropriate for the controlled machine and keep the machine state as
stable as practical under the project performance standard.

Example:

```bash
python3 tools/r2c_import_resident_target_run.py \
  --cpu 6 \
  --world /srv/helve-fixtures/world/dimensions/minecraft/overworld \
  --warmup-rounds 3 \
  --measured-rounds 12 \
  --output evidence/r2c-import-resident/run-01.json
```

The runner executes the real benchmark through:

```text
taskset -c <CPU>
  cargo run --release --locked
  --package helve-world-load-qualification
  --bin r2c_import_resident_bench
```

with `--require-single-cpu` enabled.

A target run is rejected unless:

- observed process affinity is exactly the requested CPU;
- target profile is the admitted R2C Overworld profile;
- requested warm-up/measured counts match the artifact;
- filesystem I/O remains outside the timed engine path;
- region-file count and bytes agree with the independently hashed input world;
- at least one chunk is imported/resident;
- importer section scratch does not grow after warm-up;
- retained decompression output does not grow after warm-up;
- every timing summary has valid monotone percentile shape;
- the output path is new.

## Why one run is insufficient

One apparently quiet process can still be misleading because of frequency transitions, interrupt
placement, page/cache state or other transient machine noise. The production decision boundary
therefore requires multiple independent processes, not one long run that can hide process-level
variation.

Collect at least three artifacts under the same controlled conditions. Use the same:

- exact Git commit;
- Rust toolchain/flags;
- machine and stable hardware topology;
- logical CPU;
- exact dimension byte identity;
- target profile;
- warm-up/measured workload;
- section/import/decompression mechanism;
- generated state-data identity.

Dynamic observations such as current frequency and load average are retained in raw artifacts but are
not required to be byte-identical across processes. They remain part of human machine-state review.

## Combining independent processes

Example:

```bash
python3 tools/r2c_import_resident_evidence.py \
  evidence/r2c-import-resident/run-01.json \
  evidence/r2c-import-resident/run-02.json \
  evidence/r2c-import-resident/run-03.json \
  --output evidence/r2c-import-resident/combined.json
```

The combiner rejects:

- fewer than three unique artifacts;
- ordinary unstamped benchmark output;
- code/toolchain/stable-hardware drift;
- requested/observed CPU mismatch;
- input-world SHA or byte-accounting drift;
- target profile/workload/mechanism/state-data drift;
- builder materialization/copy/write accounting drift;
- scratch-growth witnesses;
- semantic/resident structural witness drift;
- wrong timing sample counts;
- non-monotone timing summaries.

For each stage it reports per-process p50/p95/p99/max and p99/p50 tail amplification, then computes
cross-process p50 median/MAD/relative MAD and p99/tail medians.

Stages are:

1. dimension setup;
2. region validation;
3. import/decode/build;
4. resident installation;
5. complete chunk handoff;
6. complete world round.

## Admission boundary

A mechanically consistent combined report always emits:

```text
performance_admitted = false
human_baseline_review_required = true
timing_threshold_selected = false
```

That is intentional. The combiner proves that the measurements describe one stable experimental
identity; it does not decide that the machine was sufficiently quiet, that the workload is
representative enough for a production choice, or that an observed difference is materially worth
additional complexity.

Human baseline review must consider at least:

- CPU governor/frequency behavior and turbo state;
- background load and interrupt noise;
- topology/SMT/NUMA placement;
- cross-process MAD and tail stability;
- retained scratch/RSS/allocation/copy evidence;
- whether the measured cost is materially large in the complete server budget;
- whether a proposed optimization adds HOT-path tax elsewhere.

Only a later explicit decision record may select a mechanism or timing threshold.

## Current hosted indication

The first genuine-save hosted diagnostic on the final #209 identity loaded 529 overworld chunks and
12,696 block-bearing sections per round with no importer or decoder scratch growth. Hosted timing
showed resident installation far below import/decode/build cost. That observation is useful for
prioritizing experiments, but it is not decision-grade and therefore does not select an optimization.

## Requalification triggers

Regenerate target evidence when any of the following changes materially:

- importer/decompression/NBT/state-resolution code;
- final section construction or selected section policy;
- resident-install or dimension lifecycle mechanism;
- benchmark timing boundary or workload;
- scratch/reservation policy;
- generated state-data identity;
- selected world bytes;
- compiler/toolchain/flags;
- target machine/topology used for the decision baseline.
