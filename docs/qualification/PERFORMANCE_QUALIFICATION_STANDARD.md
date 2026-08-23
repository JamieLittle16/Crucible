# Crucible Performance Qualification Standard

**Status:** normative engineering standard for performance claims and mechanism selection  
**Scope:** HOT engine mechanisms, subsystem laboratories, whole-server capacity/latency work, and comparative claims

Crucible is allowed to be extremely aggressive internally because semantic correctness is qualified independently. It is **not** allowed to call an idea an optimization merely because one benchmark invocation became faster.

The governing rule is:

> **Measure the machine state, separate execution regimes, and require a material whole-cost win beyond noise.**

This standard complements semantic/equivalence qualification. Performance evidence never compensates for a correctness failure.

## 1. Optimization order

Prefer improvements in this order unless evidence justifies a lower-level intervention earlier:

1. eliminate semantically unnecessary work;
2. improve asymptotic complexity;
3. stop scanning inactive state;
4. reduce indirection, copying, allocation, synchronization and repeated resolution;
5. improve data layout/locality and specialize hot loops;
6. batch/amortize boundaries;
7. exploit independent parallelism with explicit ownership;
8. maintain derived state when maintenance is cheaper than rediscovery;
9. specialize representations by measured workload class;
10. only then spend complexity on instruction-level tuning, SIMD, prefetching, unsafe code, cache-line padding, custom allocation or architecture-specific tricks.

A lower-level optimization is still welcome when profiling proves it is the actual limiting mechanism.

## 2. Evidence regimes

A qualifying experiment MUST identify which execution regime it measures. Do not mix these into one number.

### Cold start

Includes effects such as:
- first code/data page faults;
- allocator initialization;
- filesystem/page-cache coldness where relevant;
- first construction of caches/tables;
- first-touch NUMA placement;
- cold instruction/data caches and TLB state where the protocol deliberately targets them.

Cold-start cost matters for startup, chunk admission, rare transitions and bursty operations. It MUST NOT be mixed into a steady-state throughput claim.

### Warm-up / convergence

Warm-up is a measured regime, not ritual discarded iterations.

Potential effects include:
- CPU frequency/turbo ramp;
- branch predictor history;
- instruction/data cache population;
- TLB population;
- allocator/free-list state;
- working-set residency;
- representation promotion or lazy initialization.

The experiment MUST state either:
- a fixed, predeclared warm-up protocol; or
- a deterministic convergence criterion established before examining candidate winners.

Warm-up duration/sample counts remain in evidence even when steady-state statistics exclude them.

### Steady state

Steady-state measurements begin only after the declared warm-up boundary. Candidate ordering MUST NOT systematically give one implementation a hotter core/cache state than another.

### Transition and tail

Representation changes, migration, cache misses, refills, rebuilds, publication, queue saturation and other discontinuities are measured separately when meaningful. A rare 2 ms transition is not permitted to disappear inside an attractive average mutation throughput.

## 3. CPU placement and topology

Production-qualifying CPU measurements SHOULD run on controlled physical hardware rather than shared hosted runners.

The evidence MUST record, where available:
- CPU vendor/model/family/stepping and microcode;
- allowed CPU set and memory-node set;
- online CPUs;
- SMT state;
- relevant cache hierarchy, coherency-line size and sharing topology;
- CPU governor and min/current/max frequency information;
- turbo/boost policy;
- OS/kernel;
- target triple and codegen flags.

For latency-sensitive single-thread experiments:
- pin the benchmark to an explicit logical CPU where practical;
- identify its physical-core/SMT sibling relationship;
- avoid unrelated work on the sibling when measuring uncontended performance;
- reject or separately classify runs with observed CPU migration.

For multi-worker experiments, CPU placement is part of the experiment. Report one-, two-, four- and target-many-worker efficiency rather than hiding poor per-core behaviour behind more cores.

## 4. Frequency, thermal and temporal stability

Modern CPUs are dynamic systems. Wall time without machine-state context can lie.

A production decision MUST NOT rely on a run where candidates experienced materially different frequency/thermal conditions without accounting for that difference.

Recommended protocol:
- interleave or randomize candidate order across rounds;
- use balanced rounds rather than running all samples for A and then all samples for B;
- allow an explicit machine stabilization period;
- record frequency/governor/turbo state before/around measurement where available;
- retain round-level results so temporal drift is visible.

Do not disable turbo, SMT, frequency scaling, huge pages or other platform features merely to make a benchmark easier unless that configuration represents an intended deployment profile or the experiment explicitly isolates that variable.

## 5. Cache, TLB and predictor state

A benchmark MUST say whether it models a naturally warm working set, naturally cold/streaming access, or an explicit cache-state experiment.

Do not mechanically flush caches before every sample. That often benchmarks an artificial workload. Likewise, do not accidentally benchmark a tiny object that permanently fits in L1 when the production working set is much larger.

For HOT data-structure decisions, include workloads large enough to expose relevant:
- L1/L2/LLC capacity and sharing effects;
- cache-line traffic and false sharing;
- instruction-cache footprint;
- data/instruction TLB pressure;
- branch prediction behaviour;
- prefetcher-friendly versus irregular access.

Where Linux `perf`/PMU access is available, relevant experiments SHOULD capture supported counters such as:
- cycles and instructions;
- branches and branch misses;
- cache references/misses or more specific L1/LLC events when reliable on that CPU;
- dTLB/iTLB events where available;
- context switches;
- CPU migrations;
- page faults.

Hardware counters are diagnostic evidence, not universal semantic requirements. Missing or model-specific counters must be reported as unavailable rather than fabricated or compared across incompatible PMUs.

## 6. Memory and allocator state

Performance qualification considers:
- logical owned bytes;
- resident/working-set memory where practical;
- allocation count/lifetime when relevant;
- peak scratch/temporary memory;
- fragmentation/allocator effects when they become material;
- page faults and first-touch behaviour for large structures.

An optimization that removes 5 ns from an operation by creating unacceptable memory amplification has not automatically won. Conversely, a compact representation that causes pathological transition tails has not automatically won either.

Custom allocators, slabs, huge pages and object pools are separate hypotheses. They are not architecture requirements and must demonstrate their own gain.

## 7. NUMA and cache-line ownership

Single-socket results do not prove multi-socket behaviour.

When Crucible qualifies multi-socket/NUMA deployment:
- CPU and memory affinity are recorded;
- local and remote access cases are distinguished;
- first-touch placement is controlled or measured;
- ownership/migration policies are tested for cross-node traffic;
- large shared structures are examined for cache-line bouncing/false sharing.

Padding/alignment is not automatically good. Every permanent cache-line padding decision must justify the extra footprint with measured contention/locality evidence.

## 8. Candidate ordering and statistical discipline

For selection experiments:
- use multiple independent balanced rounds;
- rotate/randomize candidate order deterministically;
- retain raw samples and round identity;
- do not average percentiles from separate runs;
- disclose sample count and duration for extreme quantiles;
- use a predeclared noise/confidence gate;
- require a material improvement beyond measured noise before accepting permanent complexity.

As a default complexity filter, roughly >=5% CPU/latency or >=10% resident-memory improvement on an official target workload is a useful starting threshold, not a universal physical law.

Small wins may still be accepted when a mechanism is simpler, removes tail risk, composes across many HOT paths, or has compelling whole-server evidence. The decision record must say why.

## 9. Whole-cost accounting

An optimization cannot win by moving cost out of the timed region.

Account for relevant phases such as:
- construction/preparation;
- warm-up;
- ordinary steady-state operation;
- promotion/demotion/rebuild;
- allocation/freeing;
- publication/serialization consequences;
- cleanup/eviction;
- memory footprint;
- contention imposed on other workers.

Microbenchmarks answer mechanism questions. Representative traces and whole-server runs decide whether those mechanism wins survive integration.

## 10. Tail latency and capacity

For server-facing systems, throughput alone is insufficient.

Where applicable report:
- p50/p95/p99/max or justified alternatives;
- open-loop offered-load capacity;
- queue growth/backpressure;
- player-visible service latency;
- tick/epoch completion time;
- core-seconds per unit of useful semantic work;
- memory at the tested load.

Closed-loop tests that silently reduce offered work when the server stalls cannot establish capacity.

## 11. Low-level optimization admission

The following are **experiments**, never defaults by reputation:
- unsafe indexing/pointer code;
- SIMD/vector intrinsics;
- manual software prefetch;
- branchless rewrites;
- cache-line padding/alignment;
- custom allocators/slabs;
- lock-free structures;
- architecture-specific code paths;
- huge-page policy;
- non-temporal loads/stores.

Before admission:
1. profiling identifies the targeted cost;
2. an independent correctness/equivalence oracle exists;
3. the optimized implementation passes the same semantic traces;
4. benchmark protocol controls relevant machine state;
5. the gain is material beyond noise on target workloads;
6. tail/memory/startup/other-worker regressions are reported;
7. a fallback/reference path remains available where strategically valuable.

Inspect generated assembly and PMU counters when they answer a concrete hypothesis. Do not optimize assembly aesthetics instead of measured service cost.

## 12. Build/codegen qualification

Performance belongs to an exact binary identity.

Record:
- commit/composition lock;
- Rust compiler/version;
- target triple and CPU features;
- LTO/codegen-units/panic policy;
- `RUSTFLAGS` / encoded Rust flags;
- profile/features;
- relevant PGO/BOLT or architecture-specific build inputs if introduced later.

If a mechanism only wins under `-C target-cpu=native`, that is a deployment-profile result, not automatically the portable default.

## 13. Hosted CI rule

GitHub-hosted timing is diagnostic only.

Hosted CI is excellent for:
- correctness/equivalence;
- benchmark compilation;
- schema/invariant checks;
- smoke measurements;
- detecting catastrophic regressions.

It is not authoritative for close production performance decisions because VM placement, host load, frequency policy and topology are not controlled sufficiently.

## 14. Required performance decision record

A production mechanism decision records:
- exact semantic/equivalence evidence identity;
- exact benchmark binary/composition identity;
- workload/corpus identity;
- machine/topology/frequency provenance;
- cold/warm/steady/transition regime definitions;
- raw samples and round order;
- relevant PMU/counter evidence when available;
- CPU, tail, memory and allocation effects;
- rejected candidates;
- known regressions/trade-offs;
- noise/confidence assessment;
- the human decision and reopen trigger.

## 15. Reopen triggers

Performance qualification is reopened when a relevant variable materially changes, including:
- target Minecraft semantics/workload distribution;
- selected composition;
- representation/algorithm;
- compiler/codegen policy;
- target CPU class;
- ownership/concurrency topology;
- allocator/page policy;
- benchmark construction;
- evidence that the old workload missed a production bottleneck.

## 16. Definition of success

The objective is not to accumulate clever tricks.

A successful Crucible optimization makes an exact supported semantic workload cheaper on the machines we care about, under a protocol strong enough that we would expect the win to survive repetition and integration.

> **No benchmark theatre. No performance by assumption. No cost hidden outside the timer.**
