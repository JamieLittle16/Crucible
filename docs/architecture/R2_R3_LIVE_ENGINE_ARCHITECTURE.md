# R2/R3 Live Engine Architecture

**Status:** architectural direction for the first persistent live server  
**Parent laws:** `CRUCIBLE_MASTER_BLUEPRINT.md`, `OWNERSHIP_SIMULATION_CONTRACT.md`, `PROTOCOL_CLIENT_SPINE.md`, `WORLD_SECTION_IMPLEMENTATION_SLICE.md`  
**Target:** Minecraft: Java Edition 26.2 / protocol 776  
**Immediate predecessor:** `milestone-r1x-first-visible-world`

## Purpose

R1X established that Crucible's bounded Rust networking/session spine can carry an unmodified 26.2 client through Handshake -> Login -> Configuration -> Play and into a rendered world. R2 and R3 convert that black-box convergence scaffold into a persistent, replay-free live server.

The objective is not to reproduce Mojang's Java implementation in Rust. Crucible must preserve supported vanilla-observable semantics while deliberately using a cleaner and more efficient engine shape.

The governing rule is:

> **Vanilla defines the game. Crucible chooses the engine.**

R2/R3 therefore freezes semantic contracts first and allows optimized mechanisms to compete behind those contracts. Playability is not permission to introduce avoidable allocation, repeated lookup, global mutation locks, runtime registries, redundant client publication, or scheduler-defined gameplay.

## Non-negotiable live-engine laws

For production HOT paths, the default expectation is:

- no per-operation heap allocation where state can be owned/reused ahead of time;
- no mandatory `dyn Trait`, runtime service registry, string capability lookup, reflection, or packet registry;
- no world-global lock for ordinary owner-local gameplay;
- no lock/atomic authority check in an owner-local mutation path;
- no repeated global chunk lookup once a local region/window/handle is resolved;
- no registry/hash-map lookup for target-version facts that can be generated into dense tables;
- no whole-world/whole-player scans for work that can be represented as active/dirty/deadline sets;
- no rebuilding semantically identical client payloads independently for every observer when safe immutable fan-out is possible;
- no unbounded per-client queue and no hidden queue behind another queue;
- no cross-region direct mutation;
- no scheduler interleaving becoming accidental vanilla semantics;
- no semantic compromise in the default strict profile merely because a faster mechanism exists.

A reference implementation may intentionally be simpler. A production mechanism earns permanent complexity only through semantic equivalence plus whole-cost evidence.

## Milestone decomposition

The existing top-level protocol plan retains R2 = visible pregenerated world and R3 = walkable server. The following sub-gates define how those milestones are reached.

### R2A — live Play control plane

An unmodified 26.2 client enters Play and remains connected through real Crucible liveness/control semantics.

Required:

- source-backed keepalive issue/acknowledgement/timeout semantics;
- bounded inbound Play decode and fail-closed malformed-input behavior;
- compact continuing connection state;
- no replay dependency for liveness;
- at least 30 minutes of idle stock-client qualification over repeated keepalive cycles;
- bounded memory with no monotone per-session growth.

R1X may remain temporarily available as a visual bootstrap oracle, but R2A mechanisms must not depend on captured Play data.

### R2B — replay-free Play bootstrap

Initial Play state is synthesized from Crucible semantic state and source-backed 26.2 projection rules.

Required:

- source-backed Play entry;
- player bootstrap state;
- position/teleport transaction establishment;
- inventory/menu initialization closure;
- required world/session metadata;
- small resumable publication state rather than a per-join `Vec<Packet>` or giant packet object graph;
- zero captured Play bodies for the admitted bootstrap path.

### R2C — Crucible-owned world projection

A pregenerated world is imported into Crucible world state and projected to 26.2 chunk/light protocol data.

Required:

- dimension-aware world loading;
- Crucible-owned chunk/section state;
- Crucible-owned light/height/derived state needed by the protocol;
- revision-bound chunk projection;
- bounded publication through the normal connection engine;
- client-visible chunk data no longer sourced from a capture.

### R2D — qualified persistent visible world (top-level R2)

Required:

- Handshake/Login/Configuration/Play all Crucible-owned;
- zero captured Play replay;
- stock client renders the pregenerated world;
- client remains connected indefinitely under ordinary liveness;
- disconnect/reconnect repeats deterministically;
- repeated joins do not rebuild immutable target/world artifacts unnecessarily;
- resource/backpressure qualification passes.

### R3A — authoritative movement

Network movement packets become typed semantic inputs delivered to current simulation authority. The connection never directly mutates gameplay state.

### R3B — vanilla-faithful collision and movement resolution

Movement/collision outcomes match the admitted vanilla semantics while using Crucible-local world-access mechanisms.

### R3C — incremental client interest and live chunk tracking

Player movement changes an interest set. Only entered/left/dirty observable state produces publication work.

### R3D — regionized live execution

The walkable slice executes under the same ownership/domain model intended for later scale, rather than being designed around a temporary global main-thread assumption.

### R3E — qualified walkable server (top-level R3)

Required:

- movement/collision authoritative;
- movement drives chunk load/send/unload tracking;
- teleport and reconnect are deterministic;
- at least two independent loaded simulation domains can execute without shared mutable world authority;
- vanilla-observable results remain invariant under legal worker-count/placement/interleaving perturbations;
- no captured Play traffic.

## Engine topology

```text
NETWORK I/O
sockets / borrowed framing / bounded buffers
        |
        v
TARGET PROTOCOL 26.2
bytes <-> target packet law <-> semantic inputs/projections
        |
        v
CLIENT SESSION
identity / liveness / protocol transactions / interest cursor
        |
        v
SIMULATION AUTHORITY
region/domain owner-local mutation + canonical stage semantics
        |
        +------------------------+
        |                        |
        v                        v
WORLD STORAGE               TYPED EFFECTS
chunks/sections             cross-domain consequences
        |
        v
CLIENT PROJECTION
interest / dirty revisions / shared encoded artifacts
        |
        v
NETWORK I/O
```

The layers are intentionally asymmetric:

- networking owns bytes and backpressure, not world state;
- target protocol owns 26.2 representation, not gameplay authority;
- client session owns connection-local transactions, not chunk storage;
- simulation authority owns semantic mutation;
- world storage owns world representation, not packet IDs;
- client projection turns semantic state/revisions into target-specific observable output.

## Regionized simulation: Folia inspiration, Crucible semantics

Folia is valuable evidence that groups of nearby chunks can form independently ticking regions with region-local data and explicit merge/split behavior. Crucible should borrow the useful locality and ownership invariants without inheriting implementation or semantic compromises automatically.

Useful Folia-inspired ideas:

- loaded world space is partitioned into independently mutable regions;
- a ticking/executing region has exclusive ordinary write authority over its local world/entity data;
- nearby chunk ownership can be approximated through coarse power-of-two region cells to reduce regionizer bookkeeping;
- executing regions do not expand ownership while executing;
- buffer/halo ownership prevents adjacent independently executing regions from racing over near-boundary state;
- nearby regions merge when independence disappears;
- disconnected areas split when independence reappears;
- region-local active structures avoid global concurrent collections for ordinary gameplay.

Crucible-specific constraints:

1. **Authority is not the thread.** `DomainId + ownership generation + placement` defines authority; a worker merely executes it.
2. **Region shape is a mechanism, not API semantics.** Region-cell size, merge radius, split threshold, worker placement and queue implementation remain replaceable/qualifiable.
3. **Logical time remains vanilla-faithful.** Crucible must not expose divergent per-region game time simply because regions execute independently. A stage/time snapshot is frozen where vanilla semantics require a common observable value.
4. **Cross-region consequences are typed effects.** Arbitrary foreign mutation/task closures are not the default communication model.
5. **Migration occurs at a closed semantic boundary.** The existing ownership generation/revision contract remains authoritative.
6. **Schedule invariance is tested.** Worker count and legal independent interleavings must not alter stage semantic digests.

### Region cells

A candidate production regionizer should operate on coarse `N x N` chunk cells, with `N` a power of two, rather than paying full merge/split bookkeeping for every individual chunk when that precision has no semantic value.

This is a performance mechanism, not a frozen constant. Candidate values must be measured against:

- regionizer add/remove cost;
- loaded-world fragmentation;
- false merging of otherwise independent players;
- cache locality;
- migration frequency;
- cross-region effect volume;
- memory overhead;
- tail latency.

The regionizer may use coarse cells for ownership bookkeeping while chunk identity remains exact.

### Region-local data

An active region should directly own or resolve compact local structures for at least:

- chunk slots/handles;
- entities;
- block/fluid scheduled work;
- dirty sections/chunks;
- local client watchers/interest members;
- pending typed effects;
- local deferred-work install points.

Inactive structures should not be scanned merely because a region exists.

## Dimension abstraction

A dimension is a semantic/runtime namespace, not a subclass hierarchy.

Crucible should separate three identities:

```text
DimensionTypeFacts   immutable target/datapack semantics
DimensionInstance    one loaded runtime dimension/world namespace
DimensionId          compact runtime identity used after cold resolution
```

### Dimension type facts

Cold/generated/data-driven facts include, where applicable:

- minimum Y and height/section lattice;
- coordinate scale;
- fixed time / skylight / ceiling / ultra-warm / natural-style flags;
- bed/respawn and other dimension-type semantics;
- protocol registry identity;
- target-specific constants required by projection.

Namespaced/string identities are resolved at cold boundaries. HOT code should consume compact IDs/handles and direct facts.

### Dimension instance

A loaded dimension instance owns:

- immutable `DimensionTypeFacts` reference/handle;
- frozen `SectionLattice` mapping world Y -> dense section index;
- chunk directory / regionizer root;
- dimension-global semantic state that truly belongs to that dimension;
- region/domain ownership graph;
- persistence/world-source identity;
- projection/cache namespaces.

Standard Overworld/Nether/End assumptions must not leak into generic HOT world access. The current qualification firewall already treats dimensions separately because their vertical lattices and representative section distributions differ; the runtime architecture should preserve that explicitness.

### No virtual dimension dispatch in inner loops

Dimension differences should be resolved before inner block/collision loops where possible. Examples:

- resolve `SectionLattice` once;
- resolve dimension-local chunk window once;
- carry compact generated facts;
- specialize target/version static facts at build/composition time where appropriate.

A generic dimension abstraction must not mean repeated trait-object calls or string map lookups per block access.

## Chunk and section layout

Chunk/section identity remains semantic. Physical layout is replaceable.

### Vertical lattice

For a loaded dimension, establish one immutable dense section lattice:

```text
min_section_y
section_count
max_section_y
```

A chunk should normally address vertical sections by direct dense index after a single bounds transform, not by a hash map keyed by section Y.

Uniform/empty sections should not require a 4096-cell backing allocation. The selected production `world.section-store/1` provider may use `direct`, `adaptive`, `fast-local`, `packed-local`, or a later qualified mechanism; correctness and per-dimension Pareto evidence decide.

### Chunk core

The live chunk core should keep frequently accessed semantic metadata compact and locality-friendly. Candidate organization:

```text
LiveChunkCore
  header / revision identities
  contiguous section descriptors
  compact occupancy/summary masks
  derived-state revision stamps
  light/heightmap handles or compact state
  owner-local active metadata
```

Do not insert per-section boxed polymorphism merely to make representation replaceable. Replaceability belongs at the selected concrete component boundary.

### Joining chunks for hot local work

Do not repeatedly route every block query through a world-global directory.

The already-qualified `ResolvedChunkWindow` direction is the model:

```text
resolve exact local chunk set once
        -> dense row-major chunk handles
        -> repeated block/collision/path/streaming access
```

For collision, pathfinding, explosions, local AI and other dense local work, boundary/generalization cost should be paid once and amortized. If profiling proves coordinate transforms remain material, candidates may precompute window origin/bounds so inner reads use non-negative offsets, shifts and masks instead of repeated signed division/remainder.

### Region-local chunk joining

Regions may additionally maintain a dense/slab-like local chunk-slot view or other locality-optimized index so owner-local operations resolve world coordinates into local slots without a process-global hash lookup. Exact representation remains a measured mechanism.

The key law is:

> **Global/general lookup may discover locality. It must not remain inside a loop after locality is known.**

## Generated data and lookup policy

Minecraft target data is unusually suitable for code generation because the target version is pinned and many semantic universes are finite.

Crucible should aggressively move resolution from runtime HOT paths into generation/cold startup where correctness permits.

### Existing direction

The generated target database already demonstrates the intended shape:

- dense vanilla block-state identity;
- `u16` `BlockStateId` for the 26.2 state universe;
- direct array indexing;
- generated mutation flags/facts;
- source/generation SHA-256 identity.

### R2/R3 generated-data rules

Prefer, in order where applicable:

1. identity/no mapping at all;
2. direct dense array indexed by compact numeric ID;
3. bitset/packed flag table for boolean/small facts;
4. generated sorted/static table with cold binary search;
5. generated perfect/minimal hash or match tree for cold string/name resolution;
6. generic `HashMap` only where the key universe is truly runtime/dynamic and the whole-cost evidence justifies it.

HOT gameplay should not repeatedly resolve namespaced IDs, packet names, block-state properties, dimension names, registry keys, shape flags, or protocol IDs from strings.

### Pre-generated protocol artifacts

Target-global immutable protocol material should be generated or encoded once where legal. Per-connection bootstrap should reference shared immutable artifacts and only synthesize genuinely connection-specific fields.

## Derived maps and world-generated state

Crucible should avoid treating derived world structures as throwaway values that are repeatedly rediscovered.

Examples include:

- heightmaps;
- occupancy/non-air summaries;
- collision/shape summaries;
- light dirtiness/frontiers;
- block-entity presence;
- tickable/active-state summaries;
- section/chunk protocol projection dirtiness.

Rules:

- every derived structure declares its semantic source of truth;
- every cached/derived structure declares a revision/coherence rule;
- mutations should update a compact summary incrementally when cheaper than later full rescans;
- a mutation that cannot affect a derived map should not dirty it;
- expensive recomputation should be deferred/lazy when the result is not currently observable;
- deferred preparation may run anywhere but installation obeys generation/revision freshness and current authority.

### World generation future direction

World generation is not an R2 prerequisite, but its future architecture should exploit the same data-oriented model:

- generate directly into Crucible section/chunk representations where practical;
- avoid transient object graphs mirroring Mojang internals;
- pre-resolve generated target facts;
- construct derived maps during generation when the producer already has the necessary information and the whole cost is lower;
- parallelize genuinely independent generation stages/chunks while installing through explicit ownership/freshness boundaries;
- cache reusable generator structures at world/dimension scope rather than reconstructing them per chunk;
- benchmark intermediate-representation elimination as a first-class optimization.

## Minimize work required from the stock client

Crucible cannot change the vanilla client, but it can avoid making that client parse, decompress, integrate, render, or discard redundant server output.

The target is **minimal semantically sufficient publication**, not maximum packet throughput.

### Publication law

For every client-visible semantic fact, ask:

```text
Is it observable by this client now?
Has the client already been told an equivalent revision?
Can multiple clients share the expensive projection work?
Does vanilla require ordering/batching that constrains this optimization?
```

If the first answer is no or the second is yes, ordinary publication work should approach zero.

### Incremental interest tracking

Maintain each client's observable chunk/entity/world interest incrementally.

When a player crosses an interest boundary:

```text
entered = new_interest - old_interest
left    = old_interest - new_interest
kept    = intersection
```

Only `entered`, `left`, and dirty observable members create work. A stationary player in an unchanged world should not cause a per-tick rescan/resend of its entire view.

The mechanism should avoid a large independent `HashMap` per client where a region-local watcher/index or compact generation-stamped structure can represent the same semantics more cheaply.

### Revision-keyed shared projection

For expensive world payloads, use a semantic identity such as:

```text
(chunk identity, chunk semantic revision, target protocol, projection variant)
```

A projection cache may then produce one immutable encoded artifact and fan it out to every interested client that needs that exact revision.

Mutation advances semantic revision and makes old projections ineligible. Cache coherence is therefore explicit rather than heuristic.

The same principle may apply to other sufficiently large/shared state, but not every packet deserves caching. Whole-cost evidence must include cache memory, invalidation, construction, compression and fan-out savings.

### Encode/compress once where bytes are identical

If the exact framed/compressed body is valid for multiple clients under the same negotiated target/compression state, expensive serialization/compression should be shareable. If connection-specific framing or compression state prevents exact byte sharing, share the highest immutable common intermediate that remains correct.

### Dirty propagation instead of polling

World/entity mutations should mark or enqueue only affected observer/projection work. Do not scan every loaded chunk/entity/player each tick merely to discover nothing changed.

### Prioritize useful initial world data

Within vanilla-permitted ordering, initial chunk publication should prioritize data that makes the client become useful/visible quickly rather than spending bandwidth and client CPU on distant work first. Any ordering optimization must be source/differential qualified so it does not alter required vanilla semantics.

### No redundant semantic updates

Suppress no-op/redundant publication where vanilla semantics permit. A server-side no-op should not become a packet merely because an implementation callback ran.

## Fan-out architecture

Many server costs scale with `observers x changed facts`. Crucible should aggressively separate:

```text
semantic change
    -> projection construction
    -> immutable publication artifact
    -> observer fan-out
```

from a naïve shape:

```text
for each observer:
    rediscover state
    reserialize state
    recompress state
    send
```

Fan-out must remain bounded and backpressure-aware. One slow client must not retain unbounded shared artifacts or block unrelated observers indefinitely.

## Active-set scheduling: do not scan inactivity

R2/R3 should introduce the permanent pattern used later by ticks/entities/chunks:

- liveness deadlines live in an active deadline structure;
- dirty projections live in dirty sets/queues;
- scheduled block/fluid work lives in deadline/bucket structures;
- active/tickable entities live in active region-local sets;
- changed client interest is triggered by movement/settings/world events;
- unloaded/inactive chunks consume no ordinary tick scanning.

Reference implementations may scan to prove semantics. Production mechanisms must justify any broad scan with evidence that it wins total cost.

## Network and publication mechanics

The existing network laws remain:

- borrowed decode;
- bounded ingress/egress;
- transactional publication admission;
- explicit backpressure;
- compact connection state;
- no second hidden outbound queue.

Potential later optimizations include:

- vectored I/O;
- syscall batching;
- shared packet bodies;
- buffer pools;
- preframed immutable artifacts;
- compression caching;
- alternate executors.

None is admitted by aesthetics. Each must preserve exact bytes/semantics and demonstrate a whole-cost win including memory and tail behavior.

## Modularity rules

Create a durable component/module boundary when at least one is true:

- it owns a semantic law;
- it owns mutation authority/state lifetime;
- it is target-version specific;
- it has an independent qualification surface;
- multiple mechanisms genuinely need to compete behind one contract.

Do not create micro-components for every packet, field, coordinate or helper merely for directory symmetry.

Likely durable boundaries:

```text
wire/framing
connection/session
26.2 target protocol
client projection/interest
simulation authority/regionizer
world/chunk/section storage
persistence/world import
lighting/derived world state
```

HOT replacement should normally be installation-time/static composition, not runtime virtual dispatch.

## Qualification obligations

Every R2/R3 mechanism should carry four evidence classes where applicable.

### 1. Vanilla semantic evidence

- source-backed VAR/SEM contract;
- golden bytes/state transitions;
- differential capture where observable;
- stock-client black-box probe.

### 2. Reference equivalence

- deterministic reference implementation;
- randomized/property/fuzz traces;
- schedule/worker perturbation for concurrency-sensitive state;
- exact semantic digests.

### 3. Resource evidence

- bounded queues/buffers;
- allocation counts;
- logical owned bytes / RSS where meaningful;
- no monotone growth over long sessions;
- slow-client/backpressure adversaries.

### 4. Performance evidence

- representative workloads;
- synthetic stress/tails;
- target-hardware runs;
- p50/p95/p99/max and raw samples;
- whole-cost accounting including setup, cache memory and invalidation;
- explicit complexity threshold before optimized machinery becomes permanent.

## R2/R3 implementation order

The intended sequence is:

```text
R2A live control plane
  -> keepalive + bounded continuing Play

R2B source-backed replay-free bootstrap
  -> semantic Play plan + player/inventory/position state

R2C dimension/world import and projection
  -> DimensionInstance + SectionLattice
  -> pregenerated chunk/section import
  -> chunk/light projection
  -> revision-keyed publication candidate

R2D persistent visible world
  -> zero replay + reconnect + resource qualification

R3A movement semantic input
  -> connection -> authority handoff

R3B collision
  -> resolved local windows + vanilla differential/reference

R3C interest tracking
  -> incremental chunk/entity observable sets

R3D regionized production executor
  -> region cells + local data + merge/split/migration

R3E walkable server
  -> multi-domain schedule-invariant qualification
```

Do not postpone the ownership/region shape until after a single-thread-shaped gameplay implementation has spread through the codebase. Reference execution may remain simple, but interfaces and state ownership from R2 onward must be compatible with the intended regionized engine.

## Final principle

The target is not merely a fast Minecraft server.

The target is a server where the architecture makes wasted work difficult to express:

- locality is resolved once and reused;
- immutable facts are generated once and directly indexed;
- unchanged state produces no work;
- unobservable state produces no client work;
- shared output is computed once when safe;
- owner-local state mutates without locks;
- independent regions execute in parallel without sharing gameplay authority;
- the stock client sees vanilla-faithful results regardless of execution topology.

That is the standard R2/R3 should establish before Crucible expands into broader gameplay.