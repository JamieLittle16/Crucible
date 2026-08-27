<div align="center">

# Helve

### Same game. Different engine.

A high-performance, parity-focused Minecraft: Java Edition server engine written in Rust.

[![CI](https://github.com/JamieLittle16/Crucible/actions/workflows/ci.yml/badge.svg)](https://github.com/JamieLittle16/Crucible/actions/workflows/ci.yml)
[![License: MPL-2.0](https://img.shields.io/badge/license-MPL--2.0-blue.svg)](LICENSE)
[![Rust 1.97.1](https://img.shields.io/badge/Rust-1.97.1-orange.svg)](rust-toolchain.toml)

**Strict supported vanilla fidelity. Independently designed internals. Measured performance.**

[Documentation](docs/README.md) · [Architecture](docs/architecture/CRUCIBLE_MASTER_BLUEPRINT.md) · [Execution plan](docs/execution/EXECUTION_MASTER_PLAN.md) · [Contributing](CONTRIBUTING.md)

</div>

---

> [!NOTE]
> Helve is the new public name of the project previously called Crucible. The GitHub repository and
> some internal `crucible-*` crate/path identifiers are intentionally being migrated separately so
> the product rename does not create unrelated build-graph churn. Approved Helve visual assets will
> be added separately; the previous Crucible lockup is intentionally not shown here.

## What is Helve?

Helve is a from-scratch Minecraft: Java Edition server engine whose target is **the supported game, not Mojang's implementation architecture**.

The official server is used as a white-box and black-box semantic oracle. Helve reconstructs those semantics behind independently designed, replaceable, data-oriented mechanisms intended to eliminate unnecessary work, scale cleanly across cores, and remain efficient on ordinary hardware.

The project is intentionally strict about the distinction between **correctness evidence** and **performance evidence**. An optimization does not become production-worthy merely because it is fast, and a plausible reimplementation does not become "vanilla" merely because common cases appear to work.

> **Freeze the laws. Benchmark the mechanisms.**

> **Everything is replaceable; indirection is optional.**

## Project status

> [!IMPORTANT]
> **Helve remains experimental and does not yet provide a playable production server release.**
>
> On **27 August 2026**, an unmodified Minecraft: Java Edition **26.2** client completed Handshake → Login → Configuration → replay-free Play through Helve's source-backed networking/bootstrap path with **zero captured Play publication**. The client crossed the `WorldProjection` boundary and eventually entered the expected pre-R2C void. Native world/chunk/light projection is the next product milestone.

This closes the R2B bootstrap boundary: the stock client can enter Play through Helve-owned semantic bootstrap, continue on the same bounded connection driver, acknowledge the initial teleport and remain under live keep-alive control. Terrain is intentionally absent until R2C owns world projection.

Current work includes:

- target-version state data and source-backed Vanilla Atlas infrastructure;
- section representation candidates and semantic equivalence qualification;
- world/section contracts and reference implementations;
- bounded Handshake/Login/Configuration networking accepted by the stock 26.2 client;
- replay-free R2B Play bootstrap, teleport acknowledgement and continuing liveness;
- R2C native chunk/light/world projection;
- deterministic qualification, replay and evidence machinery;
- repository architecture, dependency and provenance guards;
- controlled performance qualification including tail latency and arrival smoothness.

The next product-facing visual slice is intentionally narrow: replace **Loading Terrain → void** with a source-backed Helve world projection containing valid chunk/section/light state and visible terrain. World generation is not a prerequisite for that first native-world slice.

### Current client boundary: replay-free R2B

The current stock-client development path uses the real source-admitted Configuration route and Helve's replay-free semantic R2B bootstrap. Captured Play traffic is structurally excluded from that path. Until R2C publishes world state, the client may remain on Loading Terrain before entering an empty void.

The earlier [R1X First Visible World milestone](docs/milestones/R1X_FIRST_VISIBLE_WORLD.md) remains historical evidence for the first end-to-end visible-world smoke test, but it is no longer the current implementation boundary.

## Why Helve exists

Helve is built around four simultaneous requirements.

### 1. Semantic fidelity

Supported Minecraft behaviour is reconstructed because it has been understood and qualified, not because an implementation happens to look plausible.

### 2. Structural efficiency

Performance work starts by removing work: unnecessary scans, object creation, synchronization, copying, interpretation and repeated resolution. Instruction-level tuning comes later.

### 3. Scalable concurrency

Independent semantic work should execute in parallel without allowing worker scheduling, ownership topology or timing accidents to become gameplay.

### 4. Replaceable architecture

Important mechanisms should be independently testable and replaceable without forcing plugin-style dynamic dispatch through every hot operation.

## How Helve establishes parity

The development pipeline is explicit:

```text
Official Mojang source
        ↓
Vanilla Algorithm Record (VAR)
        ↓
Helve semantic rules (SEM)
        ↓
Simple reference implementation
        ↓
Vanilla parity qualification
        ↓
Optimized Helve component
        ↓
Equivalence evidence (EQUIV)
        ↓
Performance qualification
        ↓
Integrated product slice
```

Reference implementations are permanent infrastructure, not throwaway prototypes. They provide independent oracles for differential testing, replay debugging, component substitution and optimization validation.

## Performance philosophy

Helve does not define success as "Rust is faster than Java" or as winning isolated microbenchmarks.

For hot systems, the preferred optimization order is broadly:

1. eliminate semantically unnecessary work;
2. improve complexity;
3. stop scanning inactive state;
4. co-design algorithms and memory layout;
5. resolve generality once, then specialize inner loops;
6. batch and amortize boundaries;
7. parallelize real independence;
8. maintain coherent derived state when it is cheaper than rediscovery;
9. specialize hot representations;
10. apply SIMD, unsafe code or low-level tuning only when evidence earns it.

Performance claims are expected to carry workload identity, semantic coverage, memory effects, tail latency/variance and reproducible evidence rather than a single flattering throughput number.

## Repository map

```text
crates/                 Rust implementation and semantic/reference components
vanilla/                project-owned target-version records, fixtures and provenance metadata
docs/architecture/      product and architecture contracts
docs/qualification/     parity, equivalence and performance qualification design
docs/execution/         milestone, CI and release operating plans
docs/milestones/        durable black-box/product milestone records
profiles/               composition/profile policy
tools/                  qualification, source-indexing and repository tooling
benchmark-results/      checked-in benchmark/evidence outputs where policy permits
```

The internal `crucible-*` crate/path namespace is temporarily retained as a migration-stable implementation namespace. It is not the current public product name.

Start with the [documentation index](docs/README.md) rather than browsing the tree at random.

## Working on Helve

The pinned toolchain is declared in [`rust-toolchain.toml`](rust-toolchain.toml). Until the GitHub repository rename lands, an existing Rust installation can use:

```bash
git clone https://github.com/JamieLittle16/Crucible.git
cd Crucible

cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --locked
cargo xtask guard
```

The runnable product binary is named **`helve`**; the Cargo package remains internally named `crucible-server` during the first rebrand step.

The ordinary CI lane additionally runs formatting, Clippy, source-backed section qualification, Python tooling tests and rustdoc with warnings denied.

Before proposing a change, read [`CONTRIBUTING.md`](CONTRIBUTING.md). Helve intentionally uses a stricter engineering process than most early-stage projects: meaningful changes should explain their semantic effect, evidence, performance consequence, concurrency impact and architectural cost.

## Documentation

Good entry points are:

- [R2B replay-free vanilla playtest gate](docs/qualification/R2B_VANILLA_PLAYTEST.md) — the current stock-client Handshake → Play boundary and exact claim limits.
- [R1X First Visible World milestone](docs/milestones/R1X_FIRST_VISIBLE_WORLD.md) — historical first visible-world black-box evidence.
- [Master architecture blueprint](docs/architecture/CRUCIBLE_MASTER_BLUEPRINT.md) — what the engine is trying to build and why; filename retained during the staged rename.
- [M0 foundation implementation spec](docs/architecture/M0_FOUNDATION_IMPLEMENTATION_SPEC.md) — the foundational implementation boundary.
- [World/section implementation slice](docs/architecture/WORLD_SECTION_IMPLEMENTATION_SLICE.md) — foundational world subsystem work.
- [Execution master plan](docs/execution/EXECUTION_MASTER_PLAN.md) — milestone sequencing and operating model.
- [CI qualification roadmap](docs/execution/CI_QUALIFICATION_ROADMAP.md) — how evidence becomes enforceable repository law.
- [Evidence and experiment records](docs/qualification/EVIDENCE_AND_EXPERIMENT_RECORDS.md) — how performance and correctness decisions remain reproducible.

See [`docs/README.md`](docs/README.md) for the full curated index.

## Contributing

Contributions are welcome, but Helve deliberately optimizes for **high-confidence engineering rather than low-friction merging**.

A good contribution is narrow, source/provenance-aware, independently testable, and explicit about what would falsify its assumptions. Large speculative abstractions, silent semantic compromises and performance claims without evidence are intentionally difficult to merge.

Contributors retain ownership of their work. External contributions must also accept the project [`Contributor Licence Agreement`](CLA.md); the pull-request workflow records that acceptance and fails closed when it is absent.

Read the full [contribution guide](CONTRIBUTING.md) before opening substantial work.

## Licence and project independence

Helve is licensed under the **Mozilla Public License 2.0 (MPL-2.0)**. See [`LICENSE`](LICENSE).

Do not copy or commit Mojang source code, server JARs, game assets, worlds, credentials or other proprietary Minecraft artifacts into this repository. Official source/runtime material is a local semantic and qualification oracle and is represented in the repository only through project-owned records, fingerprints, fixtures and derived evidence where permitted.

Helve is an independent project and is not affiliated with, sponsored by, or endorsed by Mojang Studios or Microsoft. Minecraft is a trademark of Microsoft Corporation.
