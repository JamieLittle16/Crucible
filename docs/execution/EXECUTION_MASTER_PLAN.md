# Crucible Execution Master Plan

Crucible advances by qualified vertical slices, not by accumulating partially implemented subsystems.

## Priority order

```text
1. repository/tooling foundation
2. component/profile composition
3. Vanilla Atlas/source index
4. generated target-version data
5. World Kernel M0
6. protocol/client spine
7. persistent visible world (R2)
8. walkable server (R3)
9. interactive world
10. survival spine
11. breadth and long-tail parity
```

## Persistent workstreams

- **Foundation / Composition** — repository, package resolver, guards, profiles, CI.
- **Vanilla Atlas** — official source index, VARs, dependency graph, generated data, version tracking.
- **Engine Kernel** — world/section/chunk, dimensions, regionization, ownership, causal execution, resource governance.
- **Client / Product Spine** — protocol, client integration, player lifecycle, interest/projection, persistence, playability.
- **Qualification** — parity, replay, property/fuzz tests, schedule perturbation, performance and memory.

## Scope control

Before starting work, ask whether it unblocks the next milestone, retires a major uncertainty, creates reusable evidence/infrastructure, prevents likely rework, or closes a high-risk performance decision in `R2_R3_PERFORMANCE_DECISION_REGISTER.md`. Otherwise defer it.

Broad redstone, villagers, advanced AI, complete worldgen, package registry UX, dynamic native loading, custom allocators, and speculative lock-free/unsafe work are explicitly not current priorities.

## Current boundary

`milestone-r1x-first-visible-world` established that an unmodified Minecraft Java 26.2 client can complete Handshake -> Login -> Configuration -> Play through Crucible and render world data using the explicitly experimental finite R1X Play replay.

The next work converts that convergence scaffold into a live engine. R2/R3 implementation is governed by:

- `docs/architecture/R2_R3_LIVE_ENGINE_ARCHITECTURE.md`;
- `docs/architecture/R2_R3_PERFORMANCE_SEARCH_PLAN.md`;
- `docs/architecture/R2_R3_PERFORMANCE_DECISION_REGISTER.md`;
- the existing ownership/world/section/protocol qualification laws.

R2 is not permitted to become a disposable global-main-thread architecture that R3 later has to replace.

## R2 — persistent visible world

### R2A — live Play control plane

Goal: the stock client remains connected using real Crucible Play liveness/control semantics.

Required:

- source-backed keepalive issue/ack/timeout law;
- bounded inbound Play processing;
- compact continuing session state;
- no replay dependency for liveness;
- repeated 30-minute idle stock-client qualification;
- bounded memory with no monotone session growth.

### R2B — replay-free Play bootstrap

Goal: synthesize initial Play from semantic state rather than captured bodies.

Required:

- source-backed initial Play packet/ordering contract;
- player bootstrap;
- teleport/position transaction;
- inventory/menu initialization closure;
- resumable bounded publication;
- zero captured Play bodies in the admitted path.

### R2C — Crucible-owned world projection

Goal: import a pregenerated world into Crucible state and produce the client's chunk/light world image.

Required:

- `DimensionRuntimeProfile`/`DimensionInstance` boundary;
- exact chunk/section ownership and generation/revision identity;
- pregenerated chunk import toward selected Crucible section representation;
- source-backed light/height/derived-state semantics required for client projection;
- revision-bound target-26.2 chunk/light projection;
- bounded projection cache/publication policy.

R2C should establish the permanent two-tier world access shape: sparse lifecycle directory at the boundary, dense region/local handles/windows in hot work.

### R2D — top-level R2 qualification

Goal: persistent visible world with no Play capture dependency.

Required:

- full Crucible-owned Handshake/Login/Configuration/Play path;
- zero captured Play replay;
- stock client renders the pregenerated world;
- indefinite ordinary liveness;
- deterministic disconnect/reconnect;
- repeated joins reuse immutable target/world work where possible;
- resource/backpressure qualification;
- performance red-team against the R2/R3 search plan.

## R3 — walkable regionized server

### R3A — authoritative player presence and movement

- split `ClientSession` from region-owned `PlayerPresence`;
- decode movement into typed semantic inputs;
- route to current authority;
- connection code never directly mutates world/player state.

### R3B — vanilla-faithful collision through local world access

- source-backed movement/collision semantics;
- resolved chunk/local neighborhood access;
- no per-block global world lookup in the production common path;
- generated block-state collision classes where evidence supports them.

### R3C — incremental interest and chunk demand

- stationary client interest cost approximately zero;
- common one-chunk movement updates only the frontier;
- dirty chunks address actual observers directly;
- chunk residency/activation demand maintained incrementally rather than whole-world rediscovery;
- client publication coalesces/reuses identical work where legal.

### R3D — regionized execution

- coarse power-of-two region-cell mechanism candidates;
- merge/split changes ownership metadata rather than copying chunk semantic state;
- region-local active structures;
- explicit halo/conflict-envelope law;
- typed cross-region effects;
- worker placement independent of semantic authority;
- scheduler APIs preserve future causal/dependency-frontier execution rather than requiring a permanent global barrier.

### R3E — top-level R3 qualification

Goal: the first qualified walkable server.

Required:

- movement/collision authoritative;
- movement drives load/send/unload state;
- teleport and reconnect deterministic;
- at least two independent loaded domains execute without shared mutable world authority;
- 1/N-worker and legal schedule perturbations produce the same semantic/client-observable result;
- no captured Play traffic;
- whole-engine benchmark matrix run across clustered, independent, convoy, exploration, mutation, boundary, teleport and slow-client cases;
- unresolved high-risk production mechanisms remain explicitly registered rather than silently frozen.

## Performance implementation order

Within each gate, optimize in this order:

```text
1. eliminate unnecessary work
2. make work incremental
3. resolve global/general state before hot loops
4. improve data layout/locality
5. share identical work
6. move cold work off hot state
7. parallelize true independence
8. cache/precompute when whole cost wins
9. specialize representation/algorithm
10. SIMD/unsafe/kernel-specific tuning only after profiling
```

This order is architectural policy, not a ban on early experiments. An experiment may test any layer; production admission must still demonstrate why lower-level complexity is necessary after higher-level waste has been removed.

## R2/R3 benchmark matrix

Production decisions must eventually include at least:

```text
idle connected
clustered spawn/fan-out
independent players/regions
convoy movement
independent exploration
mutation hotspot
region-boundary merge/split pressure
teleport/dimension migration
slow client/backpressure
storage pressure
projection fan-out
region fragmentation
```

Measure useful simulation CPU separately from routing/scheduling/projection/compression/I/O overhead, along with p50/p95/p99/max, memory, allocations/copies, queue residency and semantic equivalence.

## Playability route

```text
qualified world substrate
→ protocol/configuration
→ R1X visible-world convergence
→ live Play control
→ replay-free bootstrap
→ pregenerated Crucible world + chunk/light projection
→ persistent visible world (R2)
→ movement/collision
→ incremental chunk interest
→ regionized execution
→ walkable server (R3)
→ block interaction
→ persistence breadth
→ survival systems
```

The real target client remains an integration oracle. Source-backed SEM contracts remain the authority.
