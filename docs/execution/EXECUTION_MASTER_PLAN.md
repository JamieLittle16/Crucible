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
7. R2 persistent visible world
8. R3 walkable regionized server
9. interactive world
10. survival spine
11. breadth and long-tail parity
```

R1X (`milestone-r1x-first-visible-world`) established the black-box Handshake -> Login -> Configuration -> Play -> rendered-world boundary. It is historical evidence and an executable differential scaffold, not a production Play architecture.

## Persistent workstreams

- **Foundation / Composition** — repository, package resolver, guards, profiles, CI.
- **Vanilla Atlas** — official source index, VARs, dependency graph, generated data, version tracking.
- **Engine Kernel** — dimension/world/section/chunk, ownership, regionization, causal execution, resource governance.
- **Client / Product Spine** — protocol, client integration, player lifecycle, client interest/projection, persistence, playability.
- **Qualification** — parity, replay, property/fuzz tests, schedule perturbation, performance and memory.

## Scope control

Before starting work, ask whether it unblocks the next milestone, retires a major uncertainty, creates reusable evidence/infrastructure, or prevents likely rework. Otherwise defer it.

Broad redstone, villagers, advanced AI, complete worldgen, package registry UX, dynamic native loading, custom allocators, and lock-free/unsafe work are explicitly not current priorities.

This does **not** mean R2/R3 should be architected as a disposable single-thread prototype. The first live gameplay state must already obey the permanent ownership/layering boundaries needed by the regionized engine. Reference mechanisms may be simple; state ownership must not be temporary.

## R2 — persistent visible world

Detailed architecture: [`../architecture/R2_R3_LIVE_ENGINE_ARCHITECTURE.md`](../architecture/R2_R3_LIVE_ENGINE_ARCHITECTURE.md).

```text
R2A live Play control plane
    keepalive / continuing bounded Play / long-session resource stability

R2B replay-free Play bootstrap
    source-backed initial Play / player / inventory / position

R2C Crucible-owned world projection
    dimension instance / pregenerated world / chunk + light / revision projection

R2D qualified persistent visible world
    zero captured Play / stock-client render / reconnect / bounded resources
```

R2 must preserve the following engine directions rather than merely making the client stay connected:

- dimension-local world namespaces and dense vertical section lattices;
- selected high-performance section storage behind the qualified section contract;
- resolve-once dense local chunk windows for repeated HOT world access;
- generated/dense target facts rather than HOT registry/hash lookups;
- derived-state revisions and incremental dirty summaries;
- revision-keyed shared client projections where whole-cost evidence supports them;
- bounded fan-out and explicit backpressure;
- minimal semantically sufficient output to the stock client.

## R3 — walkable regionized server

```text
R3A authoritative movement
    network input -> typed semantic input -> current authority

R3B vanilla-faithful collision
    resolved local world access / reference+differential qualification

R3C incremental interest tracking
    entered/left/dirty observable sets, not full per-tick rescans

R3D regionized live execution
    coarse region cells / region-local data / merge-split / typed effects

R3E qualified walkable server
    movement + chunk tracking + teleport + reconnect + schedule invariance
```

R3 is not complete if it only works on one global main-thread-shaped executor. The product slice must demonstrate at least two independent loaded simulation domains and preserve vanilla-observable semantics under legal worker-count/placement/interleaving perturbations.

## Regionization direction

Folia is useful comparative evidence for independent region ownership, region-local data and coarse chunk grouping. Crucible deliberately separates those mechanisms from semantics:

```text
semantic authority != worker identity
region shape         != public gameplay API
logical game time    != local scheduler counter
foreign consequence  != direct foreign mutation
```

Crucible may use coarse power-of-two region cells, ownership halos, merge/split logic and region-local active structures, but candidate parameters/mechanisms remain evidence-selected.

## Playability route

```text
World Kernel
→ protocol/configuration
→ R1X visible-world proof
→ live Play control plane
→ replay-free bootstrap
→ dimension/pregenerated world
→ chunks/light
→ persistent visible world
→ movement/collision
→ incremental chunk interest
→ regionized walkable server
→ block interaction
→ persistence
```

The real target client is an integration oracle throughout, while source-backed semantic contracts remain the authority.
