# Crucible

**Same game. Different engine.**

Crucible is a high-performance Minecraft: Java Edition server engine written in Rust. It reconstructs supported vanilla semantics on an independently designed, replaceable, data-oriented engine rather than translating Mojang's server architecture.

The default product contract is **strict supported vanilla fidelity + production-qualified performance**.

> **Freeze the laws. Benchmark the mechanisms.**

> **Everything is replaceable; indirection is optional.**

## Current phase

Crucible is at **M0 — Foundation and World Kernel Qualification**. The immediate work is intentionally foundational: repository guards, component/profile composition, the Vanilla Atlas source index, generated target-version data, section representations, `LiveChunkCore`, mutation invariants, bulk world views, and ownership/schedule qualification.

Broad gameplay is deliberately deferred until M0 proves the substrate.

## Architecture

The core implementation method is:

```text
Official Mojang source
        ↓
Vanilla Algorithm Record (VAR)
        ↓
Crucible semantic rules (SEM)
        ↓
Simple reference implementation
        ↓
Vanilla parity qualification
        ↓
Optimized Crucible component
        ↓
Equivalence evidence (EQUIV)
        ↓
Performance qualification
```

The official server is a white-box and black-box semantic oracle. Its class hierarchy is **not** Crucible's architecture.

## Repository policy

This repository is private during the foundational phase. No license is granted yet. Do not copy or commit Mojang source into this repository. The local official source corpus is pinned by digest and indexed by tooling.

See `docs/README.md` and `docs/execution/EXECUTION_MASTER_PLAN.md` for the current plan.
