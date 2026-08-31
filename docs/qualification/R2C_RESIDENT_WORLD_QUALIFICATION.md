# R2C Resident World Qualification

Status: **benchmark harness implemented; hosted qualification active; target-hardware baseline pending**

This qualification covers the first permanent R2C resident-world substrate in `helve-world-runtime`.
It does **not** qualify Minecraft 26.2 chunk/light wire semantics, NBT projection, world generation,
chunk scheduling, collision, or listener/executor topology.

## What is being qualified

The production boundary under test is `DimensionInstance` over `LiveChunkCore`:

```text
Unloaded
  -> load_chunk
Resident generation N
  -> discover / resolve
  -> direct bounded operation on LiveChunkCore
  -> unload_chunk
Unloaded
  -> load_chunk
Resident generation N+1
  -> stale generation N handle must fail closed
```

The benchmark deliberately separates three questions which must not be collapsed into one headline
throughput number:

1. **Resident lifecycle service cost** — load, discover, resolve, read, unload, reload, stale-handle
   rejection, second resolve/read, and final unload over a pre-sized resident directory.
2. **Directory-size effect** — the same transaction is repeated at increasing resident extents.
3. **HOT contract cost** — repeated `DimensionInstance::resolve_chunk(handle)` on every block read is
   compared with one boundary resolution followed by direct `LiveChunkCore` reads for the bounded
   operation.

The third comparison is not a mechanism tournament. It is an executable qualification of the runtime
contract that sparse directory routing is COLD/BOUNDARY work rather than the per-block HOT path.

## Permanent semantic gates

Every measured lifecycle transaction must prove all of the following while producing a deterministic
semantic checksum:

- every requested chunk becomes resident exactly once;
- `discover_chunk(position)` returns the exact handle returned by `load_chunk`;
- the returned handle resolves to the expected live chunk state;
- unloading every first-epoch handle drains residency to zero;
- reloading the same semantic positions produces fresh generations;
- every first-epoch handle is rejected as `StaleGeneration` while the replacement is resident;
- every replacement handle resolves to the same deterministic semantic block state;
- the second unload drains residency to zero;
- repeated-directory HOT reads and resolve-once HOT reads return the exact same checksum.

The cross-crate integration qualification additionally exercises the production runtime with the real
26.2 generated `BlockStateId` universe and `GeneratedStateFacts`, including a signed 3x3 resident grid,
a real block mutation/revision update, mask recomputation agreement, reverse-order unload, reload with
replacement semantic state, and 64 repeated load/unload generations at one negative-coordinate chunk.

The hosted smoke gate additionally protects structural facts:

- `ResidentChunkHandle` stays at or below 24 bytes;
- `DimensionRuntimeProfile` stays at or below 24 bytes;
- the standard Overworld-like vertical lattice remains 24 logical sections (`-64 .. 320`);
- repeated-resolution workload performs one directory resolution per HOT read;
- resolve-once workload performs exactly one directory resolution per bounded HOT sample.

These are correctness/architecture gates. They are allowed to fail CI.

## Evidence states

Resident-world benchmark evidence has four deliberately distinct states:

```text
hosted diagnostic artifact
        ↓ explicit target operator action
qualified target-run artifact
        ↓ >= 3 independent matching processes
cross-process consistency report
        ↓ human review / later decision record
accepted baseline or mechanism decision
```

No lower state may be described as a higher one.

### Hosted diagnostic artifact

Produced automatically by GitHub Actions. It proves the benchmark executes, semantic/structural gates
hold and the evidence schema is valid. Hosted timing is **diagnostic only**.

### Qualified target-run artifact

Produced only through `tools/r2c_resident_world_target_run.py`. The wrapper invokes the full release
benchmark with `--require-single-cpu`, verifies the observed CPU affinity, refuses to overwrite an
existing evidence file, and stamps an explicit `target_qualification` witness:

```text
runner = r2c-resident-world-target-run-v1
explicit_operator_action = true
single_cpu_requirement_enforced = true
hosted_ci_eligible = false
```

An ordinary `--full` benchmark artifact is intentionally **not** baseline-eligible even if it happened
to run on one CPU.

### Cross-process consistency report

`tools/r2c_resident_world_evidence.py` accepts only qualified target-run artifacts and requires at least
three unique runs. It rejects mismatched code, machine/toolchain identity, stable CPU configuration,
workload shape, target witness or semantic witness. It summarizes timing without selecting a winner.

### Accepted baseline / decision record

Mechanical consistency is necessary but not sufficient. A human still reviews machine quietness,
frequency/governor context, tails, cross-process variation and whole-cost relevance. A production
mechanism is selected only by the appropriate committed decision record.

## Timing policy

GitHub-hosted timing is **diagnostic only**. No production performance claim or implementation
selection may be made from hosted CI timings.

Each benchmark artifact records raw samples plus p50/p95/p99/max for:

- full resident lifecycle transaction latency;
- repeated-resolve HOT read latency;
- resolve-once HOT read latency.

No timing threshold is currently selected. This is intentional: the first accepted target-hardware
run set establishes the R2C resident-world baseline. Later changes can then be judged against a pinned
baseline with direction stability and tail behaviour rather than against an invented number.

## Hosted qualification

The dedicated workflow first chooses one logical CPU from the runner's allowed affinity set. It then
runs **three independent smoke processes**, each pinned to that CPU:

```bash
taskset -c <cpu> cargo run --release --locked \
  --package helve-world-access-qualification \
  --bin resident_world_lifecycle_bench -- \
  --smoke \
  --output target/helve-qualification/r2c-resident-world/smoke-<n>.json
```

Smoke cases:

- `resident-1x1-positive`
- `resident-3x3-signed`

All three processes must reproduce the same semantic witness. The workflow validates artifact shape,
semantic equivalence, lifecycle invariants, compact identity sizes, sample counts, monotone percentile
summaries and non-zero checksums. It prints timing direction and ratios for diagnostics only; it does
**not** assert that one hosted timing must beat another.

The workflow also exercises the complete case matrix with reduced measurement depth:

```bash
taskset -c <cpu> cargo run --release --locked \
  --package helve-world-access-qualification \
  --bin resident_world_lifecycle_bench -- \
  --full \
  --warmup-rounds 1 \
  --measured-rounds 2 \
  --hot-reads 4096 \
  --output target/helve-qualification/r2c-resident-world/full-matrix-structural.json
```

That run continuously validates the 1/9/25/81-chunk paths without treating its two timing samples as
performance evidence. All hosted JSON files are uploaded as the
`helve-r2c-resident-world-diagnostics` workflow artifact and retained for seven days.

## Full target-hardware run

Use a quiet machine and choose one logical CPU. **Do not call the benchmark binary directly for a
baseline run**; use the explicit target runner so the evidence carries the required provenance witness:

```bash
python3 tools/r2c_resident_world_target_run.py \
  --cpu <cpu> \
  --output /tmp/r2c-resident-world-run-1.json

python3 tools/r2c_resident_world_target_run.py \
  --cpu <cpu> \
  --output /tmp/r2c-resident-world-run-2.json

python3 tools/r2c_resident_world_target_run.py \
  --cpu <cpu> \
  --output /tmp/r2c-resident-world-run-3.json
```

The wrapper executes the equivalent of:

```text
taskset -c <cpu> cargo run --release --locked \
  --package helve-world-access-qualification \
  --bin resident_world_lifecycle_bench -- \
  --full --require-single-cpu
```

Full cases:

- `resident-1x1-positive`
- `resident-3x3-signed`
- `resident-5x5-mixed`
- `resident-9x9-negative`

For the baseline qualification, run at least **three independent processes** under the same machine,
CPU-affinity, governor/turbo and toolchain conditions. Preserve every raw qualified JSON artifact. Then
combine the complete run set mechanically rather than selecting or eyeballing individual samples:

```bash
python3 tools/r2c_resident_world_evidence.py \
  /tmp/r2c-resident-world-run-1.json \
  /tmp/r2c-resident-world-run-2.json \
  /tmp/r2c-resident-world-run-3.json \
  --output /tmp/r2c-resident-world-cross-process.json
```

The combiner fails closed unless all inputs have the same commit, pinned toolchain, stable machine
metadata, single-CPU affinity, explicit target-run witness, workload shape, structural witness and
semantic checksums. Dynamic observations such as instantaneous frequency/load may vary; empty
`RUSTFLAGS` and `CARGO_ENCODED_RUSTFLAGS` are accepted as explicit evidence that no caller-supplied
flags were active.

The combined report computes cross-process lifecycle medians/MAD, p99 medians, tail amplification and
HOT-path direction stability, but deliberately emits:

```text
performance_admitted = false
human_baseline_review_required = true
timing_threshold_selected = false
```

Review the combined evidence for:

- p50, p95, p99 and max;
- direction stability across independent processes;
- tail amplification (`p99 / p50`);
- cross-process p50 median absolute deviation;
- whether directory-size growth changes lifecycle or repeated-resolution behaviour materially;
- the whole-operation cost difference between repeated routing and one-time resolution.

A later optimization is acceptable only when it preserves the semantic gates and improves a meaningful
whole-cost metric without introducing new allocations, synchronization, hidden queues, runtime
registries or networking/world-representation coupling merely to win the benchmark.

## Requalification triggers

The resident-world performance baseline must be rerun when any of the following materially changes:

- `DimensionInstance`, resident directory or handle-generation logic on the measured path;
- `LiveChunkCore` access shape or section storage policy used by the benchmark;
- generated state facts or target data used by the integration witness;
- benchmark workload/sample methodology;
- Rust compiler/codegen policy or relevant build flags;
- target hardware, kernel, governor/turbo or affinity policy.

A semantic-only documentation change does not itself invalidate numerical evidence.

## Interpretation boundary

This benchmark measures **resident world ownership and access mechanics only**. A low number here does
not demonstrate fast chunk publication, lighting, collision, persistence or world generation. Those
receive separate R2C qualification at the layer where their actual work exists.
