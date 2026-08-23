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

## Licensing

Crucible is licensed under the **Mozilla Public License 2.0 (MPL-2.0)**. See `LICENSE`.

Contributors retain ownership of their contributions. Contributions are also subject to the project Contributor Licence Agreement in `CLA.md`, which grants the Crucible Project Steward the irrevocable rights needed to maintain, sublicense and relicense the project over its lifetime.

## Repository policy

Do not copy or commit Mojang source code, server JARs, game assets, worlds, or other proprietary Minecraft artifacts into this repository. The local official source/runtime corpus is used only as a semantic and qualification oracle and is pinned by digest where needed.

Crucible is an independent project and is not affiliated with, sponsored by, or endorsed by Mojang Studios or Microsoft. Minecraft is a trademark of Microsoft Corporation.

See `docs/README.md` and `docs/execution/EXECUTION_MASTER_PLAN.md` for the current plan.
