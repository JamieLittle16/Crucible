# Executor Worker and Memory Baseline

Issue: #77

This qualification records the first reproducible 1/2/4-worker cost floor for Crucible's admitted ownership semantics. It is **not** a production scheduler selection.

The law under test is:

> Authority is semantic state. Worker placement is execution topology.

A future executor may change thread placement, queues, work stealing, affinity, NUMA policy or runtime implementation only if it preserves the same semantic results and materially improves whole cost under the same qualification protocol.

## Reference mechanism

`crucible-executor-baseline` deliberately uses only the standard library:

- one persistent scoped thread set per measured execution;
- static deterministic partitioning of domains across workers;
- exclusive ownership of each real `DirectBlockSection<BlockStateId>` by one worker;
- no mutex, `Arc`, atomic, work-stealing queue or runtime service lookup around section work;
- bounded `sync_channel` stage batches;
- an explicit coordinator barrier after every semantic stage;
- canonical `DomainId` sorting before ownership/effect application;
- `OwnershipSimulator` as the already-admitted semantic oracle.

Worker completion order is never semantic order.

The baseline intentionally includes thread creation and teardown in the timed execution. The expensive deterministic fixture clone is performed before the timer so each candidate receives an identical independent starting image rather than charging copy cost differently by worker count.

## Useful work

Every domain owns a real 4096-cell direct block section populated with valid target 26.2 block-state identities. Each stage performs a deterministic irregular read trace and one real section mutation per sixteen operations, including exact section-summary maintenance. The stage also emits deterministic local authority mutation and one cross-domain staged effect.

This is therefore not an empty thread-pool or busy-loop benchmark.

The same prepared workload is executed under exactly:

- 1 worker;
- 2 workers;
- 4 workers.

The implementation test compares the topology-independent `SemanticDigest` after **every completed stage**, plus the useful-work checksum and operation count. Any divergence is a correctness failure independent of performance.

## Timing protocol

The release harness uses all six permutations of `[1, 2, 4]` in rotation so one candidate does not systematically receive first/last frequency or cache state.

The artifact retains every measured wall-clock sample and summarizes p50/p95/p99/max per worker count. It also records:

- speedup relative to 1 worker as integer millionths;
- parallel efficiency as integer millionths;
- p50 core-nanoseconds per useful operation;
- exact useful operation count;
- final and per-stage semantic identity through the reference evidence;
- shared compiler, commit, CPU, cache, affinity, SMT, frequency/governor, NUMA-policy and memory provenance from `crucible-benchmark-support`.

Integer millionths avoid floating-point formatting becoming part of the evidence format. A value of `2_000_000` means 2.0x.

Hosted GitHub Actions timing is diagnostic only and cannot admit a scheduler mechanism.

## Memory evidence

Memory is separated into two categories.

### Deterministic logical bytes

For each worker count the artifact records:

- section-cell backing bytes;
- shallow per-domain records;
- predeclared trace bytes;
- one stage's outcome bytes;
- shallow static worker-partition vectors;
- the accounted total.

These values intentionally exclude guessed allocator metadata, channel implementation internals and operating-system thread-stack commitment.

### Process observations

Each measured run samples Linux `/proc/self/status` immediately before and after execution and retains:

- `VmRSS` before;
- `VmRSS` after;
- `VmHWM` after.

Unsupported platforms emit JSON `null`; the logical model remains available. `VmHWM` is process-lifetime cumulative and must not be misread as isolated per-candidate peak allocation.

No custom allocator is introduced merely to count allocations.

## Commands

CI smoke:

```bash
cargo run --release --locked \
  --package crucible-executor-baseline \
  --bin executor_baseline_bench -- \
  --smoke \
  --output target/executor-baseline-smoke.json
```

Controlled target-hardware run:

```bash
cargo run --release --locked \
  --package crucible-executor-baseline \
  --bin executor_baseline_bench -- \
  --full \
  --output evidence/executor-baseline.json
```

A controlled run should additionally follow `PERFORMANCE_QUALIFICATION_STANDARD.md`: quiet host, recorded affinity/topology/frequency state, stable build identity and retained raw artifact.

## Decision rule

This baseline is allowed to scale poorly. Poor scaling is useful evidence.

It establishes the simple cost floor that later production executors must beat while preserving the exact ownership/migration/effect law. Tokio, work stealing, lock-free queues, custom allocation, NUMA placement, thread pinning and specialized runtime mechanisms are not admitted merely because they are fashionable or theoretically attractive.

A replacement requires a reproducible material whole-cost improvement on representative workloads and controlled hardware, with no loss of semantic equivalence or resource bounds.
