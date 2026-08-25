# Crucible Master Blueprint — Repository Summary

Crucible does not port Minecraft's server. It reconstructs supported Minecraft semantics on a replaceable high-performance engine.

## Architectural identity

- **Same game. Different engine.**
- **Freeze the laws. Benchmark the mechanisms.**
- **Everything is replaceable; indirection is optional.**

Official Mojang source is the primary white-box implementation oracle. Crucible-owned semantic contracts are architectural truth. Simple reference components establish correctness. Optimized components compete to implement the same contracts with less work, memory, latency, or better scaling.

## Engine shape

```text
CRUCIBLE SEMANTIC KERNEL
identity / authority / logical time / composition / resource governance
                │
                ▼
ENGINE COMPONENT GRAPH
static / boot / quiescent trusted mechanisms
                │
                ▼
RUNTIME EXTENSIONS
sandboxed semantic plugins / observers / commands
```

### Core laws

- supported vanilla semantics are authoritative for the default product;
- mutable semantic state has singular ordinary mutation authority;
- workers execute authority but do not define it;
- scheduler topology is not accidental gameplay input;
- foreign mutation and freshness-sensitive reads are explicit;
- deferred work prepares anywhere but installs through current authority with generation/revision validation;
- derived state has a declared source of truth and coherence rule;
- resources are governed and bounded;
- stable APIs expose semantics rather than engine internals;
- important correctness/performance claims require independent evidence.

## Efficiency doctrine

Optimize in this order:

```text
eliminate work
→ improve asymptotics
→ avoid inactive scans
→ co-design algorithms and layout
→ batch boundaries
→ parallelize true independence
→ cache/precompute when total cost wins
→ specialize representations
→ SIMD/unsafe only when earned
```

For the current R2/R3 frontier this doctrine is expanded by:

- [`R2_R3_LIVE_ENGINE_ARCHITECTURE.md`](R2_R3_LIVE_ENGINE_ARCHITECTURE.md) — permanent live-server ownership/locality/projection laws;
- [`R2_R3_PERFORMANCE_SEARCH_PLAN.md`](R2_R3_PERFORMANCE_SEARCH_PLAN.md) — whole-engine work-elimination and optimization search;
- [`R2_R3_PERFORMANCE_DECISION_REGISTER.md`](R2_R3_PERFORMANCE_DECISION_REGISTER.md) — high-risk mechanism choices that require representative evidence before they freeze.

## Vanilla reconstruction

```text
OFFICIAL SOURCE
→ VAR
→ SEM
→ REFERENCE
→ PARITY
→ PRODUCTION
→ EQUIV
→ PERF
```

Mojang implementation constrains behavior, not Crucible structure.

## Package/product model

Crucible installations are dynamically composed, while HOT engine code is statically specialized where performance benefits.

Official profile families include strict `balanced`, `performance`, and `memory`, plus explicit `experimental` and deliberately non-strict profiles. A strict composition rejects any package carrying semantic deviation records.

## Current frontier

The foundational world/section and protocol/client work has now crossed the first stock-client visible-world boundary (`milestone-r1x-first-visible-world`). The current frontier is R2/R3: replace the finite R1X Play replay with a persistent, replay-free, regionized live engine and then a qualified walkable server.

R2/R3 must use the permanent performance shape rather than a disposable single-thread implementation: explicit dimension/runtime identities, region-compatible chunk ownership, dense/local world access, generated HOT facts, incremental derived/client state, revision-keyed shared projections, bounded asynchronous work and schedule-invariant multi-worker semantics.

World generation remains deliberately outside the R2 critical path; pregenerated vanilla worlds let the client/world/simulation architecture mature before generation breadth is required.

This summary is intentionally concise. The full architecture bundle is the source for detailed subsystem policies during implementation.
