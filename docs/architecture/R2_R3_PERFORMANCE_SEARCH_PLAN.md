# R2/R3 Performance Search Plan

**Status:** normative optimization search/qualification plan for the first persistent live engine  
**Parent:** `R2_R3_LIVE_ENGINE_ARCHITECTURE.md`  
**Target:** Minecraft: Java Edition 26.2 / protocol 776  
**Scope:** R2 persistent visible world through R3 qualified walkable server, while preserving the engine shape needed by later survival-scale workloads

## Purpose

This document answers a deliberately stronger question than "how do we make R2/R3 work?":

> **What engine shape lets Crucible reach R2/R3 without leaving obvious structural performance on the table or baking temporary single-thread assumptions into the permanent server?**

Crucible cannot prove an implementation is globally optimal before representative workloads exist. It can, however, make optimization opportunities explicit, remove avoidable work structurally, preserve multiple mechanism candidates where evidence is incomplete, and require qualification before a convenient implementation becomes production law.

The standard is therefore not aesthetic minimalism and not speculative cleverness. It is:

```text
freeze observable semantics
        ↓
eliminate unnecessary work
        ↓
resolve generality before hot loops
        ↓
make locality and ownership explicit
        ↓
share identical work
        ↓
make inactive state cost approximately nothing
        ↓
keep parallelism available at real independence boundaries
        ↓
measure whole cost
        ↓
only then add lower-level complexity
```

## Optimality standard

An R2/R3 production mechanism is not considered performance-complete merely because it is faster than the reference implementation.

For every material subsystem, the review must answer:

1. **Can the work be eliminated?**
2. **Can the work be made incremental instead of rediscovered?**
3. **Can general lookup/resolution happen once outside the inner loop?**
4. **Can data be represented more densely or with better locality?**
5. **Can identical results be shared across observers or operations?**
6. **Can cold state use a cheaper representation than active state?**
7. **Can independent work execute concurrently without changing semantics?**
8. **Can expensive work be prepared asynchronously and installed under revision/authority validation?**
9. **Can generated target facts replace dynamic runtime interpretation?**
10. **Does the proposed complexity win on representative whole-cost measurements, including memory and tail latency?**

If an optimization has not been measured yet, architecture should preserve the ability to test it without making it mandatory.

## Whole-engine cost model

For planning purposes, live-server cost is decomposed into independent attack surfaces:

```text
W_total =
    W_network_ingress
  + W_protocol_decode
  + W_input_routing
  + W_simulation
  + W_world_access
  + W_derived_state
  + W_interest_tracking
  + W_projection
  + W_serialization
  + W_compression
  + W_network_egress
  + W_chunk_lifecycle
  + W_persistence
  + W_scheduling
  + W_observability
```

The goal is not to optimize every term equally. The goal is to make each term attributable, measurable and structurally reducible.

A mechanism that improves one term by moving hidden cost into another has not demonstrated a win.

## Frozen high-level topology

The live world should be shaped as:

```text
DimensionInstance
  |
  +-- sparse lifecycle / address-space directory       COLD / BOUNDARY
  |
  +-- Regionizer
        |
        +-- RegionCell (coarse power-of-two chunk grid)
        |     |
        |     +-- dense chunk occupancy / chunk slots
        |     +-- cell-local activity metadata
        |     +-- cell-local observer/scheduling metadata where useful
        |
        +-- ActiveRegion
              |
              +-- owned RegionCell handles
              +-- resolved local chunk access
              +-- region-local player/entity/activity sets
              +-- due work / dirty work
              +-- typed outgoing effects
              +-- projection/observer work
```

The sparse global directory is a lifecycle tool. It is not the ordinary block-access mechanism during simulation.

The coarse cell is a regionizer/locality unit. It does not replace exact chunk identity.

The active region is an authority and locality boundary. It is not a permanent object graph that world storage must copy when regions merge, split or migrate.

## 1. Dimension abstraction

### Frozen law

A dimension is split into three identities:

```text
DimensionTypeFacts    immutable semantic facts
DimensionInstance     one loaded world/dimension state
DimensionId           compact runtime identity
```

Resource-location strings are boundary/configuration identities, not hot-loop lookup keys.

### Required hot facts

Once a dimension is loaded, ordinary world code should have direct access to compact resolved facts such as:

- minimum block Y;
- maximum/exclusive block Y;
- minimum section Y;
- section count;
- coordinate scale where semantically relevant;
- skylight capability;
- natural/ceiling/bed behavior facts where needed;
- protocol dimension type/runtime identifiers;
- precomputed masks/offsets derived from the vertical lattice.

These facts should normally live in one immutable compact `DimensionRuntimeProfile` or equivalent resolved structure.

### Specialization policy

Do not force every hot operation through a generic map of dimension properties.

Candidate mechanisms are:

1. one compact runtime profile passed/resolved once;
2. static specialization for standard built-in dimensions where a benchmark proves meaningful benefit;
3. a generic resolved profile for custom/data-driven dimensions.

The strict vanilla profile must support the target semantics; specialization is an implementation choice.

## 2. Two-tier world addressing

### Tier A: sparse lifecycle directory

The dimension needs a sparse structure for arbitrary chunk/cell discovery, load/unload and ownership transitions.

This structure may be a hash table or another sparse index because the coordinate space is enormous and mostly empty.

It must not be consulted repeatedly by local simulation after a region/window has been resolved.

### Tier B: dense region-local access

An active region should resolve chunk access into one of:

- direct chunk slots in coarse region cells;
- a `ResolvedChunkWindow` for a bounded spatial operation;
- a direct already-validated chunk handle/reference;
- a small fixed neighborhood view for common collision/update operations.

The current `ResolvedChunkWindow` experiment already establishes the architectural shape: pay the global/general routing cost once and use dense row-major references for repeated reads.

### Common-coordinate fast path

If profiling shows signed Euclidean coordinate conversion remains material after global lookup elimination, qualify a separate resolved-coordinate candidate:

```text
precomputed world-space origin/bounds
      +
non-negative relative coordinate
      +
>> 4  chunk/section selection
      +
& 15  local coordinate
```

This candidate must remain an optimization inside a resolved access object, not a new semantic API.

## 3. Region cells and regionization

### Why coarse cells

Region merge/split bookkeeping should not operate at one record per chunk when exact chunk granularity provides no semantic benefit.

Use a power-of-two coarse grid candidate:

```text
RegionCell coordinate = ChunkPos >> REGION_CELL_SHIFT
N = 1 << REGION_CELL_SHIFT chunks per axis
```

Candidate `N` values must be measured; no constant is frozen by this document.

### RegionCell layout

A production candidate should strongly prefer:

```text
RegionCell {
    coordinate,
    occupancy bitset,
    dense/fixed chunk-slot index,
    cell activity summary,
    optional local lists/indices,
}
```

For a fixed `N`, exact chunk slot inside the cell is shift/mask arithmetic.

The cell may contain empty slots. The design question is whether direct fixed slots, occupancy+packed handles, or an adaptive form gives the best whole cost at realistic loaded densities.

### Merge/split without moving world data

A critical requirement:

> Region merge/split should normally relink ownership metadata, not copy chunk contents.

Chunk/section semantic storage should have identity independent of the transient `ActiveRegion` object.

A merge should approximately be:

```text
ownership transition
+ RegionCell handle splice/relink
+ local index/effect structure merge
```

not:

```text
copy every chunk
rebuild every section
reallocate every entity
```

A split should partition region-cell ownership and region-local indices with work proportional to actual cell/activity structure rather than rescanning the entire dimension.

### Region halo/conflict envelope

A region may execute independently only while its mutable semantic conflict envelope cannot overlap another independently executing region.

The envelope is a semantic concept; the coarse region-cell radius is a mechanism.

If an operation would require foreign mutation or invalidate the independence proof, Crucible must use one of:

- a typed cross-region effect for a semantically deferred consequence;
- an explicit merge/ownership transition before direct mutation;
- a qualified read snapshot where vanilla semantics permit it.

The engine must not silently perform cross-region writes because two chunks happen to be physically close in memory.

## 4. Logical time without a scalability-killing global barrier

This is a critical design risk.

Folia demonstrates that independent regions can tick in parallel and maintain region-local schedules, but Crucible's strict profile must not let scheduler timing redefine vanilla-observable order, time or random behavior.

### Frozen law

Worker wall-clock order is not gameplay order.

A region tick/stage executes against an explicit logical epoch/stage image.

### Early R3 mechanism

A simple deterministic stage barrier is acceptable as a reference/qualification mechanism.

It must **not** become a permanent API assumption that every independent region must globally wait at every stage forever.

### Production search direction

The production scheduler should preserve room for a causal-frontier model:

```text
region/domain has logical epoch E
independent domains may execute E concurrently
only dependencies/effects constrain commit ordering
spatial independence provides lookahead
ownership changes close the relevant frontier
```

This resembles conservative parallel discrete-event execution: independent regions may make progress while dependency boundaries prevent causally impossible observation.

The exact mechanism is deliberately unresolved. Candidates include:

- global epoch with parallel stage execution;
- per-connected-component epoch barriers;
- dependency-frontier/causal commit;
- bounded lead with catch-up before merge/inter-region interaction.

The decision must be driven by semantic proof/evidence and scaling measurements.

### Global world facts

World time, daylight, weather, gamerules and similar state should be exposed to regions as immutable versioned stage snapshots where vanilla semantics require a common value.

A region must not repeatedly acquire a global lock to read these facts.

## 5. RNG and ordering firewall

Parallelism can silently break vanilla through random-number consumption order even when final data structures are thread-safe.

Every subsystem entering regionized execution must classify its RNG/order semantics:

```text
local independent stream
canonical ordered stream
position-derived deterministic source
shared/global sequence
unknown -> not parallel-admitted
```

Crucible must not invent per-region RNG merely because it is convenient.

Where official source shows positional/split random sources, generated or resolved positional mechanisms may be used directly.

Where behavior depends on an ordered shared stream, the semantic order must be reproduced or the subsystem must remain within one authority/order domain.

The same rule applies to:

- entity iteration order;
- scheduled tick priority/sub-order;
- block update ordering;
- game events;
- neighbor notification order;
- any observable tie-breaking.

RNG/order classification is a mandatory part of future VAR/SEM admission for regionized gameplay.

## 6. Scheduler architecture

### Do not copy Folia's scheduler mechanically

Folia's EDF-like region scheduler demonstrates useful independent scheduling but its documented scheduler is not CPU-core or NUMA aware.

Crucible should preserve a stronger production search space.

### Required properties

The scheduler should support:

- deadline awareness for 20 TPS work;
- sticky worker/core affinity for cache locality;
- NUMA-aware placement where hardware topology makes it relevant;
- bounded work queues;
- work stealing/load balancing only when it beats locality cost;
- explicit separation of simulation capacity from background chunk generation/lighting/persistence work;
- batching of tiny ready regions when scheduler overhead would dominate useful work;
- no allocation/lock round-trip per ordinary owner-local world operation;
- deterministic semantic results independent of worker assignment.

### Thread-pool separation

At minimum distinguish resource classes:

```text
network I/O
simulation/deadline-critical
background CPU (generation/light/encode as appropriate)
blocking/storage
```

They may share physical workers only after evidence shows starvation/tail behavior remains acceptable.

Background chunk work must be permit/budget bounded so exploration or join bursts cannot starve the simulation pool.

### Scheduler metadata placement

Cross-thread scheduler state and owner-local region hot state should be physically separated where practical to avoid cache-line bouncing.

Per-worker counters/queues are preferred to one heavily contended global structure when semantics do not require global serialization.

## 7. Chunk identity, storage and lifecycle

### Stable identity

Loaded chunks should have stable semantic identity independent of current worker or active-region membership.

A candidate representation is a compact generational handle:

```text
ChunkHandle {
    slot,
    generation,
}
```

Generation validation happens at ownership/lifecycle boundaries.

Inside a frozen owner-local stage, repeatedly checking a global generational table for every block read is undesirable; validated handles should resolve to local direct access.

### Lifecycle tiers

Do not force every loaded chunk to pay the memory cost of a fully active ticking chunk.

Preserve a tiered model such as:

```text
Unloaded
  ↓
Resident       semantic chunk/section state loaded
  ↓
Active         region-local ticking/observer/dirty runtime state attached
  ↓
Resident
  ↓
Unloaded
```

Potentially expensive ephemeral structures belong in an `ActiveChunkRuntime`/region-local sidecar rather than the permanent semantic chunk when they are unnecessary for inactive chunks.

The exact tier boundary must be benchmarked against transition frequency and memory savings.

## 8. Vertical chunk/section layout

The current contiguous logical section lattice is the correct default shape.

Required properties:

- one contiguous section-slot allocation per chunk rather than a map keyed by section Y;
- `min_section_y + index` arithmetic;
- compact section-count representation where target bounds permit it;
- vertical summary masks using machine words where the target section count fits;
- no allocation for uniform/empty section storage;
- representation transition cost isolated and measured.

The production section provider remains selected by the existing dimension-separated Pareto process. R2/R3 must not bypass that decision by introducing a second networking-specific section representation.

## 9. Generated target data as a hot-path compiler output

The generated `u16` block-state identity universe is an important architectural direction and should be expanded deliberately.

### Lookup priority

For immutable target facts, prefer in order:

```text
identity arithmetic / constant
→ direct dense array
→ bitset
→ packed dense table
→ generated perfect/specialized lookup
→ sparse map only when the semantic domain is genuinely sparse/dynamic
```

### Candidate generated facts

As R2/R3 grows, generation should consider direct compact facts for:

- block mutation summary flags;
- collision/shape class IDs;
- light opacity/emission classes;
- random-tick eligibility;
- fluid-state relationships;
- block-entity presence/type;
- heightmap predicate classes;
- protocol block-state identity;
- biome identity/facts;
- entity type hot classification later;
- packet IDs/field constants already owned by target generation.

### Avoid a state-transition matrix explosion

Do not generate `state_count²` mutation tables.

A block replacement can usually derive affected domains from compact facts about old/new states plus a few direct property comparisons:

```text
affected = facts(old) OR facts(new)
+ transition-specific comparisons
```

If profiling reveals a hot transition classification, generate compact equivalence/class IDs rather than a giant pair matrix.

## 10. Mutation delta and dirty propagation

Every authoritative semantic mutation should produce enough compact information to update derived state and observation incrementally.

A candidate shape:

```text
MutationDelta {
    position,
    old_state,
    new_state,
    affected_flags,
    semantic_revision,
}
```

`affected_flags` is not a public gameplay API; it is a generated/internal optimization contract tied to proven derived-state semantics.

### Coalescing

Within the legal publication window, repeated changes should coalesce where intermediate values are not independently observable.

Maintain dirty state as bitsets/small sets such as:

- changed section mask;
- changed block positions for client delta publication;
- dirty light sections/frontier;
- dirty heightmap columns;
- changed block entities;
- changed collision/occupancy summaries;
- projection-invalidated layers.

Do not append an unbounded packet/event object for every intermediate mutation.

## 11. Derived maps

Derived state must have an explicit source, revision/coherence rule and incremental update path where profitable.

### Heightmaps

Candidate hot representation questions include:

- direct `u16`/compact integer height per 16x16 column;
- vanilla-equivalent packed representation for cold/persistence/protocol use;
- incremental raise in O(1);
- lowering via bounded downward search;
- section-level/column occupancy summaries to skip impossible vertical ranges.

Do not assume the most memory-compact live heightmap is fastest overall. Benchmark direct hot arrays versus packed forms under mutation and query workloads.

### Occupancy/collision summaries

Generated state classes plus section summaries can support conservative fast rejection:

```text
section cannot contain relevant collision/fluid/random-tick state
    → skip section
```

More detailed per-column/per-subcube summaries are candidates only if representative collision/pathfinding profiles justify their memory.

### Lighting

Lighting should be treated as an incremental frontier problem rather than a whole-chunk recomputation default.

Preserve candidates for:

- uniform zero/full light-section special forms;
- compact nibble backing only when required;
- dirty frontier queues;
- section masks to avoid visiting irrelevant sections;
- asynchronous preparation/install where generation/revision stamps make it safe;
- independent-region parallelism.

Exact vanilla light semantics remain authoritative.

## 12. Client interest should be arithmetic, not a set rebuild

The common case is a player moving by one chunk or not moving at all.

A production interest tracker should not rebuild and hash an entire view-distance set every tick.

### Stationary client

If position, negotiated view settings and world visibility state are unchanged:

```text
interest-maintenance work ≈ zero
```

### One-chunk movement

For the exact vanilla-visible interest shape, precompute or derive the entering/leaving frontier.

For rectangular/square components this can reduce from O(r²) membership reconstruction to O(r) strips.

If the vanilla send shape/order is non-trivial, generate ordered offset templates for supported radius values and transition deltas.

### Teleport/view-distance change

Large discontinuities may use a full rebuild because the common incremental assumption no longer applies.

The full path remains bounded and deterministic.

## 13. Observer index and fan-out

Dirty chunks should find observers directly; the engine should not scan every client to ask whether each client can see the chunk.

Each active chunk/cell should maintain an observer index appropriate to actual density.

Candidate mechanisms:

- compact `Vec<ClientSlot>` with O(1) swap-remove and reverse membership index;
- small-inline list where very small observer counts dominate;
- dense bitset when many local client slots observe the same chunk;
- adaptive list/bitset policy selected by workload evidence.

The architecture must support O(changed observers), not O(all connected players), publication work.

## 14. Split connection state from region-owned player presence

Do not force the socket/session object to own mutable world/player authority.

Use two concepts:

```text
ClientSession
    network/protocol/liveness/transaction state

PlayerPresence
    region-owned semantic player state + world interest membership
```

Movement input is decoded by the session/network layer and routed as a typed input to the region owning `PlayerPresence`.

This separation allows network I/O to remain asynchronous without world locks and lets player authority migrate between regions/dimensions explicitly.

## 15. Projection pipeline

The client projection layer is a major performance subsystem, not packet-writing glue.

### Projection identity

An immutable projection artifact must be bound to exact semantic identity, for example:

```text
ChunkProjectionKey {
    dimension,
    chunk generation,
    relevant semantic revision(s),
    target protocol,
    projection variant,
}
```

If full chunk wire output depends on multiple independently revised sources, use a compound stamp or layer stamps rather than one heuristic "dirty" boolean.

### Initial chunk projection

For an unchanged chunk observed by many clients:

```text
semantic state
    ↓
26.2 packet body / projection        compute once
    ↓
optional framed/compressed artifact  share where settings match
    ↓
client-specific encryption/write     per connection only where required
```

Do not serialize or compress semantically identical chunk content independently per observer by default.

### Layered revisions

Where useful, track separate revisions for high-cost projection inputs such as:

- block/biome section state;
- light;
- heightmaps;
- block entities;
- other protocol-visible chunk metadata.

Whether the final 26.2 initial packet can reuse layers independently is a protocol/mechanism question, but layer stamps still improve invalidation and later delta selection.

### Cache governance

Projection caches must be byte-bounded, not merely entry-count bounded.

Slow clients must not retain arbitrary old artifacts indefinitely.

A cached artifact must own/record enough semantic identity that stale publication is impossible even if the underlying chunk mutates while encoding/sending is in flight.

## 16. Delta publication instead of full resend

After initial observation, ordinary block/world changes should use the smallest semantically valid update mechanism supported by the target protocol.

Candidates include:

- single block update;
- section/multi-block update;
- block-entity update;
- light update;
- full chunk replacement only when semantically required or proven cheaper for a sufficiently large change set.

The threshold between delta forms is a measurable mechanism decision.

Multiple changes within one legal publication interval should coalesce into final client-visible state where vanilla semantics do not require intermediate packets/events.

## 17. Minimize client-side work

An unmodified client defines the protocol decoder/renderer, so Crucible cannot optimize client internals. It can avoid causing unnecessary client work.

The strict profile should aim to avoid:

- sending unchanged chunks;
- redundant block/entity metadata;
- repeated full chunk data for small deltas;
- unnecessary unload/reload churn from unstable interest decisions;
- sending chunks outside exact admitted interest;
- large bursts that exceed the client's admitted chunk-batch pacing/acknowledgement semantics;
- duplicate configuration/world metadata when protocol semantics do not require it.

Initial world publication should prioritize the semantically appropriate nearest/most useful world data so a client reaches a stable visible state with minimal speculative work.

Any reordering/prioritization must remain within source-backed protocol semantics.

## 18. Network ingress

### Borrowed decode

Continue the current design:

- frame parsing on borrowed slices;
- exact bounded consumption;
- no packet object allocation merely to dispatch;
- fixed-size/Copy semantic inputs for common movement/control packets;
- owned allocation only when the semantic input must outlive the ingress buffer and cannot fit a bounded inline form.

### Route in batches

Network I/O may receive multiple packets before the simulation owner consumes them.

The routing boundary should support batching typed inputs to a destination region to amortize queue/cache traffic while preserving required packet order.

Do not silently drop or coalesce movement packets until source-backed semantics establish that doing so is equivalent.

## 19. Network egress

### One bounded queue boundary

Avoid queue layering such as:

```text
simulation queue
→ packet queue
→ compression queue
→ socket queue
```

unless each boundary has a demonstrated independent resource/scheduling role.

Prefer one explicit bounded publication contract with internal resumable stages.

### Shared immutable output

The egress abstraction should be able to reference immutable shared packet/projection bytes without copying them into a per-client heap object at admission.

Candidate representation:

```text
OutboundSegment {
    shared buffer handle,
    offset,
    length,
}
```

Per-client queued byte accounting remains mandatory.

Reference-count atomics, buffer-pool handles and copy-into-connection-ring alternatives should be benchmarked; sharing is not free when packets are tiny.

### Vectored/batched writes

When multiple already-encoded segments are ready, qualify `write_vectored`/equivalent batching before concatenating with another copy.

For encrypted online-mode connections, shared plaintext/compressed input may still require connection-local encryption output. The architecture should share up to the highest semantically identical stage and specialize only at the true per-connection boundary.

## 20. Compression

Minecraft packet compression is potentially expensive enough to deserve independent caching and scheduling.

For identical packet bytes under identical compression parameters, qualify sharing of compressed framed artifacts.

Cache identity must include every parameter that changes output, including target state/threshold/compression policy.

Small packets should not be routed through heavyweight shared-cache machinery if direct encode/write wins.

Compression work may execute off the simulation owner when:

- the semantic projection is immutable;
- output is revision-bound;
- completion publication validates the relevant identity;
- background work is bounded and cannot starve simulation.

## 21. Chunk loading and persistence

### Decode directly toward final live representation

Avoid:

```text
NBT object graph
→ generic palette object graph
→ temporary chunk model
→ Crucible section
```

where a bounded streaming/structured decoder can construct the selected Crucible section/chunk representation directly.

The reference decoder may remain simpler; production decode should minimize intermediate allocations/copies after qualification.

### Async save snapshots

Persistence should consume immutable/revision-bound snapshots prepared from authority.

Encoding/compression/I/O may happen off-owner.

Completion/install semantics must guarantee that stale save work cannot make newer semantic state appear persisted at a required durability barrier.

Shutdown/save-all barriers must identify the latest required revisions explicitly.

### Region-file I/O

Do not select mmap, io_uring, custom K/V storage, or another persistence mechanism by ideology. Preserve the semantic snapshot boundary and benchmark actual access patterns first.

## 22. World generation architecture reserve

World generation is not an R2 blocker, but R2/R3 storage must not make future high-performance generation awkward.

Future generation should be able to:

- generate directly into selected section representations;
- compute derived height/occupancy information while source data is already hot;
- cache immutable generator/noise structures per dimension/seed;
- precompute active/no-op noise octave data;
- parallelize independent chunk/stage work with explicit dependencies;
- use SIMD only when floating-point/order parity is maintained;
- install results through chunk generation/revision authority.

Do not require a Mojang-shaped intermediate chunk object model as the generation ABI.

## 23. Active sets: inactive state should be cheap

Do not scan every loaded chunk/entity/client each tick to discover whether work exists.

Maintain explicit active structures for:

- due block/fluid ticks;
- random-tick-eligible sections;
- dirty chunks/sections;
- clients with pending publication;
- regions with a due tick/deadline;
- chunks needing lifecycle transition;
- deferred work completions;
- persistence flushes.

The exact container depends on workload:

- bitset for dense bounded identities;
- intrusive/dense list for active sparse objects;
- bucket/timing wheel for deadlines;
- heap only when ordering/range requires it.

A dormant region/chunk/client should not consume recurring CPU simply to prove it remains dormant.

## 24. Scheduled ticks and deadline structures

Vanilla scheduled work has semantic ordering rules that must be source-backed.

Once the order key is frozen, candidate containers may include:

- hierarchical timing wheel/buckets for near-future due ticks;
- ordered overflow structure for distant deadlines;
- region/cell-local queues to avoid global contention;
- compact deterministic tie-break fields.

Do not use a general heap for every scheduled item if bounded tick horizons/buckets prove superior, but do not sacrifice exact priority/sub-order semantics for O(1) insertion.

## 25. Entity architecture reserve

R3 begins with players; later survival introduces many entities. Avoid making player implementation force a future object-per-entity inheritance model.

Preserve the option for:

- generational compact `EntityId`;
- region-local dense entity slots;
- hot/cold field separation;
- SoA or grouped hot fields for position/AABB/velocity when measured;
- generated entity type/class IDs;
- type-specific cold behavior data;
- static/jump-table dispatch where appropriate instead of mandatory trait objects;
- event-interest bitsets so absent plugin/observer hooks cost approximately nothing.

A general-purpose ECS is not automatically optimal. FerrumC demonstrates one possible ECS direction, but Crucible should select entity storage from its actual vanilla semantic/access patterns rather than ecosystem popularity.

## 26. Cross-region effects

Cross-region communication should carry compact typed semantic data, not arbitrary closures capturing world graphs.

Effects should be batchable by target domain/region.

For each stage:

```text
owner-local work
→ compact outgoing effects
→ group/sort by target + semantic order key
→ destination install at defined boundary
```

If cross-region effect volume becomes high, that is also evidence that the regionizer is failing to preserve locality and should consider merging/repartitioning.

Effect traffic is therefore both a semantic mechanism and a region-quality metric.

## 27. Region migration and NUMA

Worker placement is not authority, but placement affects cache and NUMA cost.

A production scheduler should prefer stable region-to-worker/NUMA affinity when load permits.

Migration should have an explicit cost model using at least:

- recent region CPU time;
- memory footprint/locality;
- expected future lifetime;
- current queue pressure;
- cross-region effect/interaction volume;
- cache/NUMA topology.

Do not migrate a large hot region to correct a tiny short-lived imbalance.

Conversely, long-lived imbalance must not be preserved forever in the name of affinity.

NUMA-aware allocation/pinning is a later mechanism candidate, but region/chunk identity and ownership must not preclude it.

## 28. Memory layout and cache lines

Review hot structs by access frequency, not conceptual purity.

Prefer:

- hot/cold split when large rarely-read fields evict hot state;
- narrow integer identities when the target universe is bounded;
- bitsets for dense predicates;
- contiguous row-major/section-major arrays for predictable spatial loops;
- per-worker mutable counters to avoid false sharing;
- cold strings/provenance/configuration off hot structs;
- explicit byte accounting for caches/queues.

Do not add cache-line padding blindly. Use hardware-counter/false-sharing evidence where practical.

## 29. Allocation policy

The default hot-path target is zero allocation.

Allowed/expected allocation boundaries include:

- chunk/section load/construction;
- representation promotion when the selected adaptive policy requires it;
- connection establishment;
- bounded cold protocol/configuration state;
- cache artifact creation on miss;
- explicit lifecycle transitions.

Repeated ordinary movement, block read, unchanged tick, watcher iteration and already-resolved world access should not allocate.

If allocator cost becomes material at legitimate lifecycle boundaries, benchmark slab/pool/arena candidates then. Do not preemptively build a global custom allocator.

## 30. Reference counting policy

Atomic reference counting is useful at cross-thread immutable fan-out boundaries, but should not leak into owner-local world access.

Use the principle:

```text
owner-local mutable state -> ordinary exclusive references/indices
cross-thread immutable artifact -> shared handle candidate
```

Benchmark `Arc`-style sharing against copying/pool handles for actual packet sizes and observer counts.

## 31. Backpressure and overload control

Every asynchronous boundary needs a bound in the unit that actually consumes resources:

- bytes for network egress;
- packet/input count or bytes for ingress;
- jobs + estimated bytes for generation/light/projection work;
- dirty snapshot bytes for persistence;
- cached bytes for projection artifacts.

A slow client must not cause unbounded retention of old chunk projections.

An exploration burst must not enqueue unbounded generation/light tasks.

A storage stall must not allow unbounded save snapshots.

The strict profile may disconnect/fail/admit less work according to source/product policy; it must not silently corrupt or reorder semantics merely to maintain throughput.

## 32. Priority without hidden semantic changes

Resource scheduling may prioritize:

- simulation deadlines over background generation;
- nearest/needed chunks over speculative farther chunks;
- currently blocking join/world-entry work over prefetch;
- dirty publication for connected clients over cache warming.

Priority may change *when equivalent work is prepared* but not observable semantic order where vanilla constrains it.

## 33. Instrumentation that does not become the workload

Performance counters should usually be per-worker/per-region and aggregated outside the hot loop.

Required attribution should include at least:

- region tick useful CPU;
- scheduler/queue overhead;
- world lookup/resolution;
- collision/movement;
- dirty/derived update;
- projection encode;
- compression;
- network bytes and queue residency;
- chunk load/generation/light/save;
- region merge/split/migration;
- cross-region effect counts/bytes.

Detailed tracing should be sampling/opt-in where continuous instrumentation would distort measurements.

## 34. Production mechanism decision table

The following choices are intentionally **not frozen** until representative evidence exists:

| Surface | Candidate search | Qualification question |
| --- | --- | --- |
| Region cell size | power-of-two N values | locality vs false merge vs bookkeeping |
| Region local chunk slots | fixed slots / occupancy+packed / adaptive | memory vs lookup |
| Scheduler | global ready / sharded / sticky-steal / deadline+affinity | tail TPS + overhead + locality |
| Logical frontier | global stage / component / causal frontier | semantic equivalence vs scaling |
| Section provider | existing direct/adaptive/fast-local/packed-local | existing Pareto process |
| Heightmap live form | direct / packed / adaptive | mutation/query CPU vs memory |
| Watchers | vec / inline / bitset / adaptive | fan-out density workloads |
| Interest update | full template / arithmetic frontier / cached transition | movement CPU + correctness |
| Chunk projection cache | body / compressed frame / layered | CPU saved vs memory/retention |
| Shared output handle | refcount / pool ID / copy | fan-out whole cost |
| Scheduled work | heap / buckets/wheel + overflow | exact order + scale |
| Chunk directory | std hash / alternative sparse index | lifecycle lookup + memory |
| Persistence I/O | conventional pread/write / other | real region-file workload |
| NUMA placement | none / sticky / explicit policy | large-machine throughput/tail |
| Unsafe/SIMD | safe scalar baseline / targeted candidate | measured hotspot only |

No candidate is selected because it sounds sophisticated.

## 35. Performance red-team checklist

Before R2D and again before R3E, perform an architectural red-team answering all of the following.

### World access

- Is any local block/collision loop performing a global world hash lookup per cell?
- Is chunk/section coordinate math repeated when a spatial window can resolve it once?
- Does a hot path use strings/resource locations?
- Is section Y stored in a map when the dimension lattice is contiguous?

### Simulation

- Are inactive chunks/entities/players scanned every tick?
- Is owner-local state behind a mutex/RwLock/atomic authority test?
- Does parallelism alter RNG/order/time semantics?
- Does one slow region force unrelated regions to wait because of an implementation barrier rather than a semantic dependency?

### Derived state

- Are heightmaps/light/summaries recomputed globally for local changes?
- Can generated facts decide that a subsystem is unaffected?
- Are repeated changes coalesced?

### Client path

- Is interest rebuilt from scratch while the player is stationary or moves one chunk?
- Are all clients scanned for one dirty chunk?
- Is the same chunk serialized/compressed separately for many clients?
- Are unchanged chunks or redundant metadata sent?
- Are slow clients retaining unbounded shared artifacts?

### Network

- Does ordinary decode allocate packet objects?
- Are there redundant queue boundaries or buffer copies?
- Could vectored/batched writes remove concatenation copies?
- Is compression occurring on the simulation authority unnecessarily?

### Lifecycle/persistence

- Does region merge/split copy world data?
- Does disk decode build disposable object graphs?
- Are save/generation/light tasks bounded and revision-validated?

### Hardware

- Is scheduler migration destroying cache locality?
- Is a global queue/cache line contended at scale?
- Are NUMA effects being ignored on machines where they matter?
- Has unsafe/SIMD been added without measured need?

Any "yes" requires either redesign or an explicit evidence-backed justification.

## 36. R2/R3 implementation sequence

The order should retire semantic uncertainty while establishing permanent high-performance boundaries early.

### Step 1 — freeze live Play semantic inputs/outputs

- keepalive/liveness SEM;
- Play bootstrap packet/ordering SEM;
- position/teleport transaction SEM;
- initial inventory/menu closure;
- exact client chunk batch/interest protocol semantics needed by R2.

### Step 2 — define compact live session + player-presence split

No world mutation from connection code.

### Step 3 — introduce `DimensionRuntimeProfile` and `DimensionInstance`

Use compact runtime IDs/facts from the start.

### Step 4 — introduce the sparse lifecycle directory + region-cell abstraction

Reference regionizer may be simple, but chunk ownership must not be global-main-thread-shaped.

### Step 5 — attach region-owned `PlayerPresence`

Typed inbound movement/control routing lands at current authority.

### Step 6 — import pregenerated chunks directly into Crucible chunk/section state

Do not couple import to packet representation.

### Step 7 — build revision-bound initial chunk/light projection

First correct reference encoder, then shared-artifact candidate.

### Step 8 — build incremental interest + observer indices

Stationary and one-chunk-move common paths should already avoid full rebuild/all-player scans.

### Step 9 — remove R1X Play replay completely

R2D qualification.

### Step 10 — movement/collision through resolved chunk locality

Benchmark direct world lookup vs resolved windows on the real movement workload.

### Step 11 — regionized multi-worker execution

Run the same walkable world under 1/N workers and schedule perturbations; semantic digest/client-observable traces must match.

### Step 12 — scheduler/locality/projection optimization tournament

Only after representative R3 workloads exist, select region-cell size, scheduler strategy, watcher representation, projection cache policy and lower-level optimizations.

## 37. Required benchmark scenarios

R2/R3 performance decisions should not rely on one happy-path benchmark.

Minimum scenarios should include:

1. **Idle connected:** many stationary clients, unchanged world.
2. **Clustered spawn:** many clients seeing heavily overlapping chunks.
3. **Independent players:** clients separated into independent regions.
4. **Convoy movement:** clients moving together across chunk boundaries.
5. **Independent exploration:** many players generating/loading distinct frontiers.
6. **Mutation hotspot:** many updates inside one shared observed area.
7. **Cross-region boundary:** activity repeatedly approaches merge/split envelope.
8. **Teleport churn:** player migration between distant regions/dimensions.
9. **Slow client:** constrained egress consumer under normal world activity.
10. **Storage pressure:** delayed reads/writes while simulation continues within bounds.
11. **Projection fan-out:** one unchanged/newly-dirty chunk observed by 1, 8, 32, 128+ clients.
12. **Region fragmentation:** many tiny loaded islands vs one dense loaded area.

Measure CPU attribution, p50/p95/p99/max tick latency, memory/RSS, allocations, queue residency, bytes encoded/compressed/sent, cache hit rates, merge/split/migration cost and semantic correctness.

## 38. Success criteria

R2/R3 architecture is considered ready for implementation when:

- every material hot path has a named ownership/locality model;
- every dynamic target fact has been reviewed for generation/dense resolution;
- region merge/split cannot require copying semantic chunk state by design;
- no client/world hot loop requires repeated global lookup by API shape;
- connection state and world/player authority are separated;
- client interest and dirty propagation are incremental by design;
- projection artifacts can be shared without weakening revision coherence;
- async preparation has explicit generation/revision installation rules;
- scheduling APIs do not require a permanent global tick barrier;
- RNG/order semantics are an explicit admission gate for parallel gameplay;
- all queues/caches have explicit resource bounds;
- mechanism choices that still need evidence remain swappable behind semantic contracts;
- the benchmark matrix exists before those mechanisms are declared production winners.

## Closing principle

Crucible should not optimize by adding cleverness to work that did not need to exist.

The permanent architecture should make the cheapest correct path the natural path:

```text
no change       -> no work
local change    -> local work
known identity  -> direct lookup
known locality  -> direct handle
repeated query  -> resolve once
same result     -> share once
cold state      -> cold representation
independent work-> parallel execution
foreign effect  -> explicit boundary
stale async work-> reject
slow consumer   -> bounded retention
```

That is the standard R2/R3 should preserve as Crucible grows from a visible-world milestone into a real high-performance Minecraft engine.
