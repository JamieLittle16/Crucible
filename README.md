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

## Source and provenance policy

Do not copy or commit Mojang source code, server JARs, worlds, or other proprietary game artifacts into this repository. The local official source/runtime corpus is pinned by digest and indexed by tooling; version-controlled evidence contains independent semantic records, provenance, fingerprints, generated Crucible data, and qualification results rather than redistributed Mojang source bodies.

## Licence and contributions

Unless a file or component clearly states otherwise, Crucible source code is licensed under the **Mozilla Public License 2.0 (MPL-2.0)**. See [`LICENSE`](LICENSE).

MPL-2.0 is file-level copyleft: covered Crucible source files and distributed modifications to them remain available under MPL-2.0, while separate files in a larger work may use other licences subject to the MPL's terms.

Contributors retain ownership of their Contributions. External contributions additionally require acceptance of the [`Crucible Contributor Licence Agreement`](CLA.md), which grants the Project Steward durable rights needed to maintain, sublicense, dual-license, and relicense Crucible while leaving contributors free to use their own work elsewhere.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the engineering and contribution contract.

## Independence and trademarks

Crucible is an independent project and is not affiliated with, sponsored by, or endorsed by Mojang Studios or Microsoft.

`Minecraft` and related names and assets are the property of their respective rights holders. The Crucible software licence does not grant rights to third-party trademarks or proprietary game content.

See `docs/README.md` and `docs/execution/EXECUTION_MASTER_PLAN.md` for the current plan.
