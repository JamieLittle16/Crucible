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
→ resolve locality once and reuse it
→ batch boundaries
→ parallelize true independence
→ cache/precompute when total cost wins
→ share identical projection/fan-out work
→ specialize representations
→ SIMD/unsafe only when earned
```

The live engine should make wasted work difficult to express: unchanged or unobservable state should ordinarily generate no work; immutable target facts should be generated/resolved once; owner-local mutation should avoid locks; repeated local world access should use resolved locality rather than rediscovering world routing; semantically identical expensive client projections should be shareable when safe.

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

The 2026-08-25 R1X milestone proved that an unmodified Minecraft 26.2 client can traverse Crucible's Handshake -> Login -> Configuration -> Play path and render world data through the bounded Rust networking/session spine.

The current product frontier is now R2/R3:

```text
R2 persistent visible world
  live Play control plane
  replay-free bootstrap
  dimension/pregenerated-world ownership
  Crucible chunk/light projection

R3 walkable regionized server
  authoritative movement/collision
  incremental client interest
  region-local ownership and execution
  schedule-invariant multi-domain semantics
```

The detailed live-engine law is [`R2_R3_LIVE_ENGINE_ARCHITECTURE.md`](R2_R3_LIVE_ENGINE_ARCHITECTURE.md).

R2/R3 must preserve the high-performance world foundations already under qualification: dense/replaceable section representations, resolve-once local chunk access, generated direct target facts, explicit semantic revisions and dimension-separated evidence. Playability is not a reason to regress to global lookup/object-graph architecture.

This summary is intentionally concise. The full architecture bundle is the source for detailed subsystem policies during bootstrap.
