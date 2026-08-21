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
7. walkable server
8. interactive world
9. survival spine
10. breadth and long-tail parity
```

## Persistent workstreams

- **Foundation / Composition** — repository, package resolver, guards, profiles, CI.
- **Vanilla Atlas** — official source index, VARs, dependency graph, generated data, version tracking.
- **Engine Kernel** — world/section/chunk, ownership, causal execution, resource governance.
- **Client / Product Spine** — protocol, client integration, player lifecycle, persistence, playability.
- **Qualification** — parity, replay, property/fuzz tests, schedule perturbation, performance and memory.

## Scope control

Before starting work, ask whether it unblocks the next milestone, retires a major uncertainty, creates reusable evidence/infrastructure, or prevents likely rework. Otherwise defer it.

Broad redstone, villagers, advanced AI, complete worldgen, package registry UX, dynamic native loading, custom allocators, and lock-free/unsafe work are explicitly not current priorities.

## Playability route

```text
World Kernel
→ protocol/configuration
→ pregenerated vanilla world
→ chunks/light
→ movement/collision
→ block interaction
→ persistence
```

The real target client becomes an early integration oracle after M0.
