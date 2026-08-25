# R2/R3 Performance Decision Register

**Status:** unresolved mechanism register / mandatory qualification gates  
**Parent:** `R2_R3_PERFORMANCE_SEARCH_PLAN.md`  
**Target:** Minecraft: Java Edition 26.2 / protocol 776

## Purpose

The architecture can be correct and still leave substantial performance on the table if one expensive mechanism is allowed to become permanent without a deliberate comparison.

This register names the highest-risk choices before R2/R3 implementation begins.

For each decision:

- **semantic constraint** is frozen;
- **reference baseline** is intentionally simple;
- **production candidates** remain open;
- **decision evidence** defines what must be measured before a winner is frozen;
- **failure mode** names the technical debt we are preventing.

A production choice may be made earlier when evidence becomes decisive. An unresolved choice must not leak into public semantic APIs.

## D1 — Region-cell granularity

**Semantic constraint:** exact chunk identity and region authority remain correct regardless of coarse regionizer cell size.

**Baseline:** power-of-two coarse cells with a simple sparse cell directory.

**Candidates:** multiple `REGION_CELL_SHIFT` values; potentially adaptive policy only if static choices fail representative workloads.

**Measure:** add/remove cost, merge/split frequency, false region merging, memory, cell occupancy, cache locality, cross-region effect volume, region CPU distribution, p99 tick latency.

**Failure mode prevented:** selecting a Folia-inspired constant by imitation rather than Crucible workload evidence.

## D2 — Region-cell chunk-slot representation

**Semantic constraint:** exact loaded chunk lookup inside an owned region cell.

**Baseline:** fixed direct `N*N` chunk slots plus occupancy mask.

**Candidates:** fixed slots; occupancy+packed handle array; adaptive sparse/dense cell.

**Measure:** loaded-density distributions, point lookup, iteration, add/remove, memory, split/merge cost.

**Failure mode prevented:** paying sparse-map hashing in hot regions or wasting excessive fixed-slot memory in sparse cells.

## D3 — Logical tick frontier

**Semantic constraint:** worker wall-clock order must not alter vanilla-observable state/order/time.

**Baseline:** deterministic global stage barrier used as correctness oracle.

**Candidates:** global epoch with parallel stage work; connected-component barrier; dependency/causal frontier; bounded independent lead with reconciliation before interaction.

**Measure:** exact semantic digest/client trace equivalence, scaling under one slow region, scheduler overhead, merge/interaction latency, throughput across independent regions.

**Failure mode prevented:** permanently coupling every region to the slowest region merely because the first parallel implementation used a global barrier.

## D4 — RNG and ordered-iteration model

**Semantic constraint:** reproduce source-backed vanilla RNG/order behavior.

**Baseline:** keep uncertain RNG/order consumers inside one canonical authority/order domain.

**Candidates:** source-proven positional streams; source-proven split streams; canonical sequence allocation; local streams only where semantic independence is demonstrated.

**Measure:** differential traces over random ticks, entities, scheduled updates and future worldgen/gameplay; schedule perturbation.

**Failure mode prevented:** a thread-safe server that is subtly non-vanilla because parallelism changes random-call or tie-breaking order.

## D5 — Immutable chunk publication/snapshot mechanism

**Semantic constraint:** background consumers observe one exact chunk generation/revision and stale work cannot publish as current.

**Reference baseline:** current `PublishedChunk`: one canonical full semantic state copy.

**Production candidates:**

1. owner-thread direct target projection into a pooled immutable buffer;
2. dirty-section incremental shadow publication;
3. immutable/structurally shared section snapshots;
4. page/section COW only at publication boundaries;
5. hybrid: cached wire projection for observed chunks + full semantic snapshot only for persistence/rare rebuild.

**Measure:** copy bytes, allocations, owner-thread stall, cache locality, mutation cost, join burst, dirty chunk rebuild, background parallelism, total memory and p99 tick latency.

**Important:** COW/refcounting must not add an atomic/refcount branch to every owner-local block access merely to make background readers convenient.

**Failure mode prevented:** copying ~whole-chunk semantic state on every projection/save opportunity or, conversely, infecting the mutation hot path with snapshot machinery.

## D6 — Chunk demand / residency mechanism

**Semantic constraint:** chunks are resident/active at exactly the times required by admitted player/simulation/loading semantics.

**Baseline:** explicit per-chunk demand records driven by the R2 player interest slice.

**Production search:** replace broad rescans/ticket-graph rediscovery with incremental cause counts/levels and precomputed spatial demand deltas where semantics allow.

Potential demand causes include:

- client observation;
- simulation distance;
- scheduled work;
- explicit teleport/load transaction;
- generation/light dependency;
- persistence/deferred completion hold.

**Measure:** stationary players, one-chunk movement, teleports, clustered players, independent exploration, load/unload churn, memory and lifecycle CPU.

**Failure mode prevented:** reproducing a complex vanilla-style ticket manager when the same laws can be maintained incrementally with less rediscovery.

## D7 — Client interest update mechanism

**Semantic constraint:** exact source-backed set/order of chunks eligible/required for observation and exact protocol pacing.

**Baseline:** deterministic full interest reconstruction used as an oracle.

**Candidates:** arithmetic frontier delta; generated radius templates; precomputed one-chunk transition deltas; hybrid fallback for teleport/view-distance changes.

**Measure:** stationary, slow walking, sprint/elytra-like fast traversal later, teleports, view-distance change; CPU and temporary allocations.

**Failure mode prevented:** O(r²) set rebuild/hash churn for the overwhelmingly common stationary/one-chunk movement cases.

## D8 — Chunk watcher/observer representation

**Semantic constraint:** direct identification of clients observing a dirty chunk with exact add/remove semantics.

**Baseline:** compact vector with explicit reverse membership metadata.

**Candidates:** vector; inline-small vector; dense client-slot bitset; adaptive list/bitset.

**Measure:** observer counts 1, 2, 8, 32, 128, 512+; churn; fan-out; removal; memory.

**Failure mode prevented:** scanning every connected client or forcing a dense bitset cost on small ordinary servers.

## D9 — Client projection cache level

**Semantic constraint:** cached output is bound to exact semantic/protocol identity and cannot be stale.

**Candidates:**

1. target packet body only;
2. framed uncompressed packet;
3. compressed frame by compression policy;
4. layered chunk projection inputs;
5. multi-level cache where evidence justifies it.

**Measure:** cache hit ratio, encoding CPU, compression CPU, retained bytes, invalidation frequency, join fan-out, mutation-heavy chunks and slow clients.

**Failure mode prevented:** either recompressing identical multi-observer chunks or retaining huge low-hit caches that merely shift cost into memory.

## D10 — Projection invalidation granularity

**Semantic constraint:** any protocol-visible change invalidates exactly the necessary artifact/update state.

**Baseline:** whole chunk semantic revision.

**Candidates:** compound/layer stamps for block+biome sections, light, heightmap, block entities and other packet-visible state.

**Measure:** mutation mixes and whether layer separation prevents meaningful rebuild work without excessive bookkeeping.

**Failure mode prevented:** one changed block entity forcing expensive unrelated recomputation, or overly fine revisions adding more bookkeeping than they save.

## D11 — Delta-vs-full client publication threshold

**Semantic constraint:** choose only packet forms/orderings allowed by the 26.2 protocol semantics.

**Baseline:** straightforward semantically correct update form.

**Candidates:** single update; multi-block/section update; light-specific update; full chunk rebuild above measured crossover.

**Measure:** 1, 2, 8, 32, 128, 512+ changes per observed chunk, observer fan-out, bytes, encode CPU, client acceptance/integration latency.

**Failure mode prevented:** sending whole chunks for tiny changes or constructing thousands of tiny packets when one aggregate update is cheaper.

## D12 — Shared outbound artifact handle

**Semantic constraint:** immutable output can be referenced by multiple client egress paths while every client retains independent bounded backpressure state.

**Candidates:** `Arc`-like shared bytes; slab/pool ID with explicit lifetime; copy for small payloads; adaptive size/fan-out threshold.

**Measure:** packet size and fan-out matrix, atomic contention, memory, copies, queue operations and network throughput.

**Failure mode prevented:** assuming reference counting is always cheaper than copying or vice versa.

## D13 — Compression implementation and sharing

**Semantic constraint:** exact Minecraft compression framing/threshold semantics.

**Baseline:** one correct ordinary compressor.

**Candidates:** shared compressed artifacts; compression-level tuning; high-performance DEFLATE implementation; worker-pool preparation; bypass for below-threshold/small packets.

**Measure:** CPU cycles/byte, compression ratio, p99 latency, fan-out, queue delay, memory.

**Failure mode prevented:** compression dominating join/chunk-send CPU or an overaggressive cache consuming more memory than CPU it saves.

## D14 — Encryption boundary

**Semantic constraint:** exact per-connection online-mode encryption semantics when admitted.

**Baseline:** correct per-connection encryption after shared plaintext/framed/compressed work.

**Candidates:** hardware-accelerated AES implementation, batched connection-local encryption, buffer reuse.

**Measure:** cycles/byte, copy count, fan-out under encrypted sessions, integration with vectored writes.

**Failure mode prevented:** designing a packet-sharing cache around bytes that are no longer identical after the true per-connection boundary.

## D15 — Network runtime / kernel I/O strategy

**Semantic constraint:** bounded ordered connection I/O and backpressure.

**Baseline:** current transparent bounded connection engine.

**Candidates only after profiling:** readiness event loop, alternate executor, vectored I/O, io_uring or zero-copy kernel features where compatible with encryption/buffer ownership.

**Measure:** clients/core, syscalls, context switches, CPU, latency, memory, slow-client behavior.

**Failure mode prevented:** replacing a simple sufficient network path with fashionable async/kernel machinery before socket I/O is actually the bottleneck.

## D16 — Simulation/background pool partition

**Semantic constraint:** background preparation cannot starve deadline-critical simulation.

**Baseline:** separately governed resource classes.

**Candidates:** fixed pool partition; shared pool with reserved permits; hierarchical budgets; workload-adaptive limits.

**Measure:** fresh exploration, join storms, heavy lighting/generation, ordinary steady play, tick p99/max.

**Failure mode prevented:** excellent aggregate throughput with catastrophic MSPT because chunk/background work consumes every core.

## D17 — Scheduler affinity and stealing

**Semantic constraint:** worker placement does not affect semantics.

**Baseline:** static/sticky region placement.

**Candidates:** sticky+bounded steal, hierarchical per-NUMA queues, deadline-aware affinity scoring.

**Measure:** cache misses, migrations, utilisation, p99/max tick duration, independent-region scaling.

**Failure mode prevented:** either pathological load imbalance or work stealing that constantly destroys locality.

## D18 — Heightmap live representation

**Semantic constraint:** exact admitted heightmap values and update behavior.

**Baseline:** simple direct values with independent recomputation oracle.

**Candidates:** direct `u16`; packed live form; hot-direct/cold-packed; occupancy-assisted downward search.

**Measure:** reads, raises, lowers, chunk memory and serialization cost.

**Failure mode prevented:** choosing memory compactness that makes every mutation/query slower or keeping a wasteful fast form for enormous cold worlds.

## D19 — Lighting live representation/frontier

**Semantic constraint:** exact vanilla light result/order where observable.

**Baseline:** transparent correct light mechanism.

**Candidates:** zero/full special sections, nibble arrays, incremental frontier, section masks, async preparation/install.

**Measure:** generated chunk, single block light change, bulk edits, skylight boundaries, memory, tail latency.

**Failure mode prevented:** whole-chunk relighting for local changes or always allocating light arrays for trivial sections.

## D20 — Section storage policy

**Semantic constraint:** existing `BlockSection` law and exact state image.

**Baseline/candidates:** use the existing direct/adaptive/fast-local/packed-local qualification programme.

**Decision:** R2/R3 must consume its result rather than silently selecting a networking-friendly second representation.

**Failure mode prevented:** duplicate world truth and conversion overhead between "simulation sections" and "network sections".

## D21 — Scheduled tick container

**Semantic constraint:** exact vanilla due-time/priority/sub-order semantics.

**Baseline:** obvious ordered container.

**Candidates:** timing wheel/buckets + ordered overflow; local heaps; hybrid structures.

**Measure:** insertion, cancellation/dedup if applicable, due extraction, sparse/large tick horizons, redstone/fluid workloads.

**Failure mode prevented:** O(log n) generality for every event when exact bounded buckets can do less work, or O(1) buckets that lose ordering correctness.

## D22 — Persistence snapshot and disk decode

**Semantic constraint:** exact saved semantic state and durability barriers.

**Baseline:** canonical semantic snapshot and ordinary correct region/NBT codec.

**Candidates:** direct-to-final section decode, reusable scratch arenas, revision-bound partial snapshots, optimized DEFLATE, alternate I/O mechanisms only after profiling.

**Measure:** startup/load, exploration, save storm, shutdown flush, memory/copy bytes, stale completion handling.

**Failure mode prevented:** multiple disposable object graphs and copies between disk bytes and final live state.

## D23 — Active/cold chunk runtime split

**Semantic constraint:** resident semantic state remains identical regardless of whether ticking/observer runtime sidecars are attached.

**Baseline:** explicit `Resident`/`Active` lifecycle.

**Candidates:** exact sidecar contents and transition policy.

**Measure:** large loaded but inactive worlds, active hubs, churn, transition cost, RSS.

**Failure mode prevented:** every resident chunk permanently carrying tick/watch/dirty structures used only by a small active subset.

## D24 — Entity/player storage direction

**Semantic constraint:** vanilla player/entity state and iteration/order semantics.

**Baseline:** compact region-local player storage for R3.

**Future candidates:** dense generational arena, hot/cold split, selected SoA, static enum/jump dispatch, specialized per-type storage.

**Measure:** movement/collision now; entity-heavy survival later.

**Failure mode prevented:** accidentally freezing a Java-style object hierarchy or a general-purpose ECS into the engine before actual access patterns are measured.

## D25 — Prefetch/speculation

**Semantic constraint:** speculative work cannot become gameplay state until normal authority/semantic conditions permit it.

**Baseline:** no speculative world work beyond directly demanded R2 chunks.

**Candidates:** bounded velocity-aware chunk load/generation/projection prefetch; cache warming.

**Measure:** hit rate, wasted CPU/IO, memory, visible chunk latency, high-speed travel.

**Failure mode prevented:** doing lots of "helpful" work the player never needs and losing the project's eliminate-work discipline.

## D26 — Plugin/observer hot-path reserve

**Semantic constraint:** future extensions observe/affect admitted semantic boundaries, not internal object layout.

**Baseline:** no plugin work in R2/R3 hot loops.

**Future requirement:** event interest must be pre-resolved so a semantic event with zero interested extensions costs approximately zero allocation/dispatch work.

**Candidates:** generated/static hook sets for trusted packages; compact observer bitsets; sandbox boundary batches.

**Failure mode prevented:** adding unconditional event object allocation/dynamic dispatch to every block/entity operation for hypothetical future plugins.

## D27 — Unsafe/SIMD/custom allocator admission

**Semantic constraint:** exact behavior and memory safety obligations.

**Baseline:** safe scalar Rust.

**Candidates:** only a measured hotspot with an independent equivalent baseline.

**Decision evidence:** material whole-cost improvement beyond noise, dedicated adversarial tests/fuzzing, architecture-specific coverage where needed.

**Failure mode prevented:** irreversible complexity and portability cost without meaningful system-level gain.

## Freeze order

The following should be settled **semantically** before R2 implementation:

1. Play control/bootstrap packet law;
2. dimension/world identity and authority boundaries;
3. chunk generation/revision semantics;
4. client interest and chunk-observation semantics;
5. projection revision/coherence law;
6. cross-region effect/migration law;
7. RNG/order classification rules.

The following should deliberately remain **mechanism tournaments** until representative R2/R3 benchmarks exist:

- region-cell size/layout;
- scheduler/frontier implementation;
- watcher representation;
- projection cache level;
- snapshot mechanism;
- heightmap/light hot representation;
- compression sharing/implementation;
- NUMA/stealing policy;
- low-level unsafe/SIMD/allocator work.

## Decision-record rule

When one of D1-D27 is resolved, the decision must name:

```text
semantic contracts preserved
candidate implementations compared
exact workload corpus
source commit / target / hardware identity
CPU + tail + memory + allocation/copy evidence
winner and rejected candidates
reason rejected complexity is not retained
```

A benchmark result without a semantic identity is not enough. A correct mechanism without whole-cost evidence is not enough to become the performance default.
