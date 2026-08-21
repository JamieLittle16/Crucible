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

M0 qualifies the world substrate before broad gameplay. After M0, protocol/client integration moves early so an unmodified target client becomes an integration oracle. P0 loads pregenerated vanilla worlds, avoiding world generation as an early blocker.

This summary is intentionally concise. The full architecture bundle is the source for detailed subsystem policies during bootstrap.
