# Section target-hardware foundation admission

Status: **foundation admitted subject to final representative-member probe on this exact head; no production winner**  
Parent: #19  
Protocol: [`SECTION_TARGET_HARDWARE_QUALIFICATION.md`](SECTION_TARGET_HARDWARE_QUALIFICATION.md)

This record captures the first target-hardware qualification **measurement foundation** for the M0.3D section-representation laboratory. It is intentionally not a Pareto decision and contains no production candidate ranking.

The evidence rule remains:

> A benchmark mechanism may be admitted without admitting any benchmark result as a production decision.

## Code checkpoint

Hardened implementation checkpoint before this documentation-only record:

```text
2c1deae93bbc7e71240beb312aa07a037df2ed3c
```

PR: #43 — `M0.3D: establish controlled target-hardware section qualification`.

The eventual squash-merge SHA is recorded in the parent issue/next chronological record because it does not exist until after this branch is admitted.

## What the foundation provides

The foundation adds two cold qualification boundaries.

### Content-addressed population packs

`tools/section_benchmark_pack.py` consumes only an already admitted `vanilla-section-representative-v1` artifact. It re-verifies:

- the representative artifact manifest and its canonical digest;
- every retained file size and SHA-256;
- the independent population-admission record;
- exact four-member seed/corpus identity;
- decision eligibility and benchmark-handoff eligibility;
- the dimension-separation firewall;
- every source member corpus through the independent corpus validator;
- each source member SHA again while streaming it into the pack, closing the validation-to-pack TOCTOU gap.

It emits one little-endian-u16 pack per dimension and a pack-set manifest carrying exact size/SHA-256 identities. Packs contain only normalized target `BlockStateId` values; they contain no Mojang NBT/palette/source implementation data.

### Candidate-isolated population child

`section_bench` can run one candidate and one dimension per process:

```text
section_bench --population-pack PACK --candidate NAME --population-smoke
section_bench --population-pack PACK --candidate NAME --population-qualification
```

The five identities remain:

- `direct-reference` — diagnostic oracle only;
- `direct`;
- `adaptive`;
- `fast-local`;
- `packed-local`.

Every loaded real section is reconstructed, checked cell-for-cell and checked against an independently recomputed semantic `SectionSummary` before it can enter timing.

## Real-population workloads

The admitted child measures steady-state:

- random cell read;
- sequential full-section read;
- deterministic 4×4×4 volume read;
- positive `maybe_contains`;
- negative `maybe_contains` using a state proven absent from the loaded population;
- candidate-independent integer control work for drift/noise context.

Candidate materialization latency is recorded separately as one sample per section. It excludes pack I/O and semantic verification.

The materialization samples are **not production decode latency**. The eventual packet/persistence decode adapters remain separate semantic/performance work.

## Measurement defects found during admission audit

Three methodology defects were found and corrected before admitting the foundation. They are retained here because benchmark bugs are engineering findings, not disposable implementation trivia.

### 1. Demand-paged common scratch could contaminate candidate RSS

**Original shape**  
Common scratch was allocated before the RSS baseline, but a reserved `Vec` capacity is not proof that its pages are resident. Later writes could fault common harness pages after the baseline and make them appear to be candidate memory.

**Correction**

All common retained scratch used during population construction is now allocated and explicitly dirty-touched/prefaulted before baseline, then restored to its canonical initial contents:

- one 8192-byte raw pack-section buffer;
- one 4096-entry decoded-state buffer;
- observed-state bitset;
- the complete construction-sample array.

Protocol identity:

```text
candidate-delta-after-explicit-prefaulted-common-scratch
```

A permanent regression proves the prefault restores the canonical zero/AIR state.

### 2. Saturating RSS subtraction hid negative/noisy observations

**Original shape**  
`loaded.saturating_sub(baseline)` silently converted `loaded < baseline` into zero.

**Why this was wrong**  
A negative process-level observation is useful evidence that RSS is noisy or otherwise unsuitable for direct ranking. Replacing it with zero invents a cleaner result than was measured.

**Correction**  
RSS delta is an exact signed integer:

```text
rss_loaded_delta_kib = rss_loaded_kib - rss_baseline_kib
```

The smoke validator checks that arithmetic identity, and a permanent regression locks positive, negative and zero cases.

### 3. Positive membership queries privileged cell zero

**Original shape**  
Positive `maybe_contains` used each selected section's cell-zero state. Natural terrain can make that disproportionately AIR/stone and therefore an easier workload than a representative positive membership query.

**Correction**  
The deterministic query plan now chooses `(section, cell)` and the positive needle is the state at that exact planned cell. Every planned positive query is preflighted and required to return `true` before timing.

A focused regression uses AIR at cell zero and a distinct state at a nonzero planned cell, proving the benchmark needle follows the plan rather than cell zero.

## RSS interpretation

The child records separately:

- baseline `VmRSS`;
- loaded `VmRSS`;
- exact signed loaded-minus-baseline RSS delta;
- baseline/loaded `VmHWM`;
- deterministic logical owned bytes;
- maximum per-section logical owned bytes;
- known prebaseline common harness bytes;
- representation transitions;
- logical backing-allocation events.

RSS is process-level evidence. Logical owned bytes are representation-level evidence. Neither substitutes for the other.

Timing query plans and representation-summary maps are allocated after the loaded RSS snapshot so they do not enter the candidate residency delta.

## Exact pack-file identity boundary

The dependency-light Rust child validates the target/population/admission/dimension/section identities in the pack header but deliberately does not implement an independent SHA-256 stack.

Therefore qualifying orchestration must immediately before child launch:

1. validate the pack-set manifest;
2. hash the exact pack file;
3. require equality with the pack manifest SHA-256;
4. launch the child without rewriting the pack;
5. bind that exact pack SHA-256 to the returned child evidence.

A raw child artifact without that wrapper binding remains diagnostic.

## Hosted smoke evidence

Exact hardened head:

```text
2c1deae93bbc7e71240beb312aa07a037df2ed3c
```

`Section Target Benchmark Smoke`:

```text
run:      32597230354
artifact: section-target-benchmark-smoke
artifact SHA-256:
66b655f7dd8b3c070d6038607845801d7676bd61fb3f90215bf4a656b03b9605
```

The uploaded artifact was explicitly inspected after the workflow passed.

For all five candidate records:

- schema/harness identity matched;
- the explicit-prefault RSS protocol matched;
- `rss_loaded_delta_kib` was a signed JSON integer;
- the reported delta exactly equalled loaded RSS minus baseline RSS;
- construction samples covered all three transition-oriented smoke sections;
- steady-state workload set was complete;
- hardware/toolchain/affinity fields were present.

The three-section smoke's deterministic known prebaseline heap scratch was 16,432 bytes:

```text
8192 raw pack bytes
+ 8192 decoded BlockStateId bytes
+ 3 * 16-byte u128 construction samples
= 16,432 bytes
```

Representation coverage in the smoke exercised uniform/local/direct/packed states, including 17-state and 257-state transition pressure.

**Hosted timing and RSS values are diagnostic only and are not candidate-ranking evidence.**

## Gate state on the hardened head

At the time this record was created:

- normal strict CI: **PASS**;
- supply-chain/dependency policy: **PASS**;
- rustfmt: **PASS**;
- all-target Rust check: **PASS**;
- Clippy `-D warnings`: **PASS**;
- Rust tests including new RSS/query regressions: **PASS**;
- Python tooling tests: **PASS**;
- rustdoc: **PASS**;
- synthetic benchmark smoke: **PASS**;
- target benchmark smoke: **PASS**;
- full four-candidate release semantic qualification: **PASS**;
- official parser-corpus probe: **PASS**, including all-five Rust reconstruction and expected decision rejection;
- official representative-member probe: **IN PROGRESS** at bounded official world generation when this documentation record was first written.

The representative-member status must be updated to PASS before #43 is squash-merged.

## What this foundation does not prove

It does **not**:

- select a section representation;
- declare any candidate faster or smaller from GitHub-hosted measurements;
- create a cross-dimension ranking;
- prove the current benchmark-materialization path equals eventual production decode cost;
- freeze adaptive promotion thresholds;
- qualify one target machine run;
- define acceptable noise by hiding unstable samples.

Those are later evidence layers.

## Next admitted layer

The next clean slice is the target-machine orchestrator. It must:

- consume the exact admitted four-seed representative population;
- verify exact pack SHA-256 immediately before every child launch;
- run one candidate/dimension per fresh affinity-pinned process;
- rotate candidate order deterministically across repeated rounds;
- preserve dimensions as independent decision strata;
- verify the child's reported CPU affinity matches the requested CPU;
- retain raw child artifacts and their hashes;
- preserve signed RSS observations;
- use control measurements to diagnose drift/noise;
- run controlled synthetic mutation/churn/promotion evidence on the same hardware with candidate isolation;
- emit diagnostics/Pareto inputs without silently selecting a winner.

Only after repeated controlled target-hardware evidence exists may the #19 Pareto/decision record select winners and reject loser mechanisms.
