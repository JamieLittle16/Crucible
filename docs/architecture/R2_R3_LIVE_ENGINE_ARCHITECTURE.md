# R2/R3 Live Engine Architecture

**Status:** architectural direction for the first persistent live server  
**Parent laws:** `CRUCIBLE_MASTER_BLUEPRINT.md`, `OWNERSHIP_SIMULATION_CONTRACT.md`, `PROTOCOL_CLIENT_SPINE.md`, `WORLD_SECTION_IMPLEMENTATION_SLICE.md`  
**Performance search:** `R2_R3_PERFORMANCE_SEARCH_PLAN.md`, `R2_R3_PERFORMANCE_DECISION_REGISTER.md`  
**Target:** Minecraft: Java Edition 26.2 / protocol 776  
**Immediate predecessor:** `milestone-r1x-first-visible-world`

## Purpose

R1X established that Crucible's bounded Rust networking/session spine can carry an unmodified 26.2 client through Handshake -> Login -> Configuration -> Play and into a rendered world. R2 and R3 convert that black-box convergence scaffold into a persistent, replay-free live server.

The objective is not to reproduce Mojang's Java implementation in Rust. Crucible must preserve supported vanilla-observable semantics while deliberately using a cleaner and more efficient engine shape.

The governing rule is:

> **Vanilla defines the game. Crucible chooses the engine.**

R2/R3 therefore freezes semantic contracts first and allows optimized mechanisms to compete behind those contracts. Playability is not permission to introduce avoidable allocation, repeated lookup, global mutation locks, runtime registries, redundant client publication, or scheduler-defined gameplay.

`R2_R3_PERFORMANCE_SEARCH_PLAN.md` expands this architecture into a whole-engine cost-elimination and qualification programme. `R2_R3_PERFORMANCE_DECISION_REGISTER.md` names the high-risk choices that must remain mechanism candidates until representative evidence selects them. Those documents are part of this architecture contract rather than optional later optimization notes.

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

A dimension is not a polymorphic world object whose hot operations repeatedly ask dynamic questions.

Crucible should separate:

```text
DimensionTypeFacts
    immutable semantic/type facts

DimensionInstance
    loaded world state for one dimension

DimensionId
    compact runtime identity
```

The standard dimensions may have generated/resolved facts such as vertical lattice, skylight capability, coordinate scale and protocol identifiers. These facts should be resolved at load/composition boundaries and passed to hot code in compact form.

Strings/resource locations remain boundary identities rather than ordinary inner-loop keys.

The architecture must also preserve dimension-specific performance evidence. Existing section qualification already demonstrates why Overworld, Nether and End must not be silently collapsed into one workload score.

## Chunk and section topology

Crucible already owns the right foundational shape:

```text
chunk column
  |
  +-- contiguous logical section lattice
  +-- compact vertical summary masks
  +-- generation + semantic revision
```

The normal production direction is:

- section Y resolves by arithmetic against the dimension lattice, not by a per-section hash map;
- empty/uniform sections do not allocate 4096 cells;
- the selected production section representation comes from the existing correctness + representative population + hardware Pareto process;
- block-state facts use generated dense target data;
- repeated local spatial work resolves chunk windows once instead of repeatedly traversing a global world directory.

### Region cell -> chunk slot -> section

The intended hot lookup hierarchy is:

```text
already-owned region/local view
    -> coarse cell / dense chunk slot
    -> chunk
    -> section index
    -> local block cell
```

The global sparse world directory remains necessary for lifecycle/discovery. It is not the inner-loop block lookup API.

### Resolved local windows

Collision/pathfinding/streaming-shaped operations should be able to resolve a fixed chunk neighborhood once and reuse dense references. The existing `ResolvedChunkWindow` qualification is the first concrete experiment in this direction.

## Generated target data

Static target-version facts should be compiler output, not repeatedly interpreted runtime data.

Priority order for immutable hot lookup is:

```text
identity arithmetic
→ dense array
→ bitset/packed table
→ generated specialized lookup
→ generic hash map only when actually dynamic/sparse
```

The current target data already uses dense compact block-state identities. R2/R3 should extend that pattern to additional proven hot facts rather than introducing registries back into inner loops.

## Mutation, revisions and derived state

A semantic mutation should advance the smallest authoritative revision(s) necessary and update derived state incrementally.

Potential derived state includes:

- section/chunk activity summaries;
- heightmaps;
- light dirtiness/frontier;
- collision/occupancy summaries;
- block-entity presence;
- client projection dirtiness.

The source of truth and coherence law for every derived structure must be explicit.

A change that provably cannot affect a derived subsystem should not mark that subsystem dirty merely because "a block changed."

Generated old/new block-state facts should allow many of these decisions with direct table/bit operations.

## Client interest and minimal publication

The target client is a semantic endpoint, not a reason to resend the world repeatedly.

For each client maintain compact current interest/observation state.

When position/view settings change:

```text
entered = new_interest - old_interest
left    = old_interest - new_interest
kept    = intersection
```

Only the delta needs lifecycle work.

A stationary client in an unchanged area should require approximately zero chunk-interest CPU.

Dirty world state should reach only actual observers; do not scan every connected player to discover which ones care.

## Shared revision-keyed projection

Client projection should be cacheable by exact semantic identity.

Conceptually:

```text
(chunk identity, generation/revision, target protocol, projection variant)
       ↓
immutable encoded/projection artifact
       ↓
fan-out to interested clients
```

When multiple clients need identical chunk state, the expensive semantic-to-protocol work should not be repeated independently unless whole-cost evidence shows sharing costs more.

Caches must be bounded by bytes and must never weaken revision coherence.

After a client already observes a chunk, smaller target-supported deltas should normally replace full chunk retransmission for local changes.

## Network/projector boundary

The existing borrowed-decode/bounded-publication architecture remains.

Future production egress should be able to consume shared immutable projection segments while retaining per-client bounded backpressure/accounting.

Compression, encryption, buffer pools and vectored I/O belong at their true boundaries. Shared work should extend only as far as bytes remain semantically identical across clients.

## Chunk lifecycle and persistence

Pregenerated R2 world import should construct Crucible semantic chunks/sections without requiring a Mojang-shaped object graph.

Long-term chunk lifecycle should distinguish resident semantic state from active region-local runtime structures so inactive loaded chunks do not pay every active/ticking/watch structure cost.

Persistence and other background work use immutable/revision-bound preparation and install/commit rules. Stale asynchronous completion must fail or be superseded according to an explicit durability contract.

## World generation reserve

World generation is not an R2 prerequisite, but R2 storage must not obstruct a future optimized generator.

A future generator should be able to:

- generate directly toward selected Crucible section/chunk representations;
- precompute/cache dimension- and seed-wide immutable structures;
- compute derived state while generation data is already hot;
- parallelize true independent work with explicit dependencies;
- install through normal generation/revision authority;
- qualify SIMD/unsafe/specialized noise paths only while preserving exact vanilla output for the strict profile.

## Active-set scheduling

Inactive state should not be scanned to prove it is inactive.

Use explicit active/due/dirty structures where semantics permit:

- scheduled block/fluid work;
- random-tick-eligible sections;
- dirty chunks/sections;
- clients needing publication;
- deferred completions;
- chunks needing lifecycle transition;
- persistence flush work.

The container is a mechanism decision: bitset, dense list, timing wheel/bucket, heap or hybrid depending on exact ordering and density.

## Extensibility reserve

Future package/plugin semantics must attach at semantic boundaries rather than forcing unconditional dynamic dispatch/event allocation through every hot operation.

If no installed extension observes a class of event, the ordinary production path should be able to approach zero extension overhead.

Static composition remains preferred for trusted HOT engine capabilities.

## Qualification principle

A sophisticated mechanism is not automatically a production optimization.

Every candidate must answer:

```text
same semantics?
less total work?
less/reasonable memory?
better p50/p95/p99/max?
scales on representative topology?
benefit survives construction/invalidation/migration cost?
complexity justified?
```

The companion performance-search and decision-register documents define the mechanism tournament and red-team gates in detail.

## Implementation ordering rule

R2 must not become a disposable single-thread architecture.

A simple reference executor is permitted, but the persistent APIs and state layout introduced for R2 must already respect:

- explicit world/domain authority;
- region-compatible chunk identity/storage;
- connection/player-presence separation;
- revisioned world state;
- incremental interest/projection;
- bounded asynchronous work;
- future multi-worker schedule invariance.

This lets R2 be implemented quickly without making R3 a rewrite of every world/client subsystem.

## Closing target

The intended progression is:

```text
R1X visible replay-backed world
    ↓
R2A live Play liveness
    ↓
R2B replay-free bootstrap
    ↓
R2C Crucible world projection
    ↓
R2D persistent visible world
    ↓
R3A authoritative movement
    ↓
R3B collision/local world access
    ↓
R3C incremental chunk interest
    ↓
R3D regionized execution
    ↓
R3E qualified walkable server
```

At R3E the server is not merely playable. It should already embody the ownership, locality, generated-data, incremental-work and shared-projection laws that make later survival breadth and large-player scaling tractable.
