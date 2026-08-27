# Helve documentation

Helve's documentation is organized around **authority, implementation, and evidence**. Higher-level contracts own project policy; lower-level documents implement, test or qualify those contracts and must not silently weaken them.

The project was previously called Crucible. Historical evidence and a small number of staged-migration filenames/internal crate names retain that name where rewriting them would add churn or obscure provenance; current product-facing documentation uses **Helve**.

If you are new to the project, do not try to read everything in repository order. Use the paths below.

## Start here

### I want to understand what Helve is

1. [`architecture/CRUCIBLE_MASTER_BLUEPRINT.md`](architecture/CRUCIBLE_MASTER_BLUEPRINT.md) — the master architectural narrative; filename retained during the staged rename.
2. [`qualification/R2B_VANILLA_PLAYTEST.md`](qualification/R2B_VANILLA_PLAYTEST.md) — the current stock-client replay-free Handshake → Play boundary and exact claim limits.
3. [`architecture/R2_R3_LIVE_ENGINE_ARCHITECTURE.md`](architecture/R2_R3_LIVE_ENGINE_ARCHITECTURE.md) — the persistent live-server architecture: regionized ownership, dimension/chunk/section locality, generated HOT data, shared client projection, interest deltas and R2/R3 gates.
4. [`architecture/R2_R3_PERFORMANCE_SEARCH_PLAN.md`](architecture/R2_R3_PERFORMANCE_SEARCH_PLAN.md) — the whole-engine optimization search: every major R2/R3 cost surface, mechanism candidate, red-team question and benchmark needed before production choices freeze.
5. [`architecture/R2_R3_PERFORMANCE_DECISION_REGISTER.md`](architecture/R2_R3_PERFORMANCE_DECISION_REGISTER.md) — the high-risk mechanism register: snapshotting, chunk demand, logical frontier, region cells, watchers, projection caches, scheduler/NUMA, compression, persistence and future entity storage.
6. [`execution/EXECUTION_MASTER_PLAN.md`](execution/EXECUTION_MASTER_PLAN.md) — how the project advances by qualified vertical slices rather than feature accumulation.
7. [`../README.md`](../README.md) — public project status, contributor quick-start and current milestone.

### I want to work on the current milestone

1. [`qualification/R2B_VANILLA_PLAYTEST.md`](qualification/R2B_VANILLA_PLAYTEST.md) — the replay-free client boundary immediately before R2C world projection.
2. [`architecture/R2_R3_LIVE_ENGINE_ARCHITECTURE.md`](architecture/R2_R3_LIVE_ENGINE_ARCHITECTURE.md) — architectural law and implementation order for R2 persistent world and R3 walkability.
3. [`architecture/R2_R3_PERFORMANCE_SEARCH_PLAN.md`](architecture/R2_R3_PERFORMANCE_SEARCH_PLAN.md) — performance completeness review for regionization, scheduling, world layout, generated facts, dirty state, client projection, networking, persistence and hardware locality.
4. [`architecture/R2_R3_PERFORMANCE_DECISION_REGISTER.md`](architecture/R2_R3_PERFORMANCE_DECISION_REGISTER.md) — unresolved choices that must not become permanent without representative semantic/performance evidence.
5. [`architecture/OWNERSHIP_SIMULATION_CONTRACT.md`](architecture/OWNERSHIP_SIMULATION_CONTRACT.md) — authority, migration, staged effects and schedule-invariance law used by the regionized engine.
6. [`architecture/WORLD_SECTION_IMPLEMENTATION_SLICE.md`](architecture/WORLD_SECTION_IMPLEMENTATION_SLICE.md) — world/section implementation and HOT-path constraints.
7. [`qualification/WORLD_ACCESS_BENCHMARK_HARNESS.md`](qualification/WORLD_ACCESS_BENCHMARK_HARNESS.md) — resolve-once dense chunk-window access versus repeated world routing.
8. [`qualification/SECTION_REPRESENTATION_LAB.md`](qualification/SECTION_REPRESENTATION_LAB.md) — section representation candidates and the performance questions they must answer.

### I want to understand how claims become evidence

1. [`qualification/EVIDENCE_AND_EXPERIMENT_RECORDS.md`](qualification/EVIDENCE_AND_EXPERIMENT_RECORDS.md) — durable evidence and experiment records.
2. [`qualification/PERFORMANCE_QUALIFICATION_STANDARD.md`](qualification/PERFORMANCE_QUALIFICATION_STANDARD.md) — project-wide rules for warm-up, machine state, cache/topology effects, counters, statistical discipline and low-level optimization admission.
3. [`execution/CI_QUALIFICATION_ROADMAP.md`](execution/CI_QUALIFICATION_ROADMAP.md) — how qualification becomes enforceable CI/repository policy.
4. [`qualification/SECTION_BENCHMARK_HARNESS.md`](qualification/SECTION_BENCHMARK_HARNESS.md) — benchmark methodology for the foundational section subsystem.
5. [`qualification/SECTION_SEMANTIC_FIXTURES.md`](qualification/SECTION_SEMANTIC_FIXTURES.md) — source-backed semantic fixtures.

## Milestones

| Document | Purpose |
| --- | --- |
| [`R2B_VANILLA_PLAYTEST.md`](qualification/R2B_VANILLA_PLAYTEST.md) | 2026-08-27 stock Minecraft 26.2 client reaches replay-free Play/WorldProjection with zero captured Play publication; R2C world/chunk/light is next. |
| [`R1X_FIRST_VISIBLE_WORLD.md`](milestones/R1X_FIRST_VISIBLE_WORLD.md) | 2026-08-25 historical first visible-world stock-client evidence; records the finite replay boundary and exact claim limits. |

Milestone/qualification records are durable black-box/product evidence. They do not silently promote experimental mechanisms to production semantics.

## Architecture

| Document | Purpose |
| --- | --- |
| [`CRUCIBLE_MASTER_BLUEPRINT.md`](architecture/CRUCIBLE_MASTER_BLUEPRINT.md) | End-to-end architectural narrative; legacy filename retained during staged rename. |
| [`R2_R3_LIVE_ENGINE_ARCHITECTURE.md`](architecture/R2_R3_LIVE_ENGINE_ARCHITECTURE.md) | Persistent live-server design: R2/R3 gates, region cells, dimensions, chunk/section locality, generated lookup policy, revision-keyed projection/fan-out and client-work minimization. |
| [`R2_R3_PERFORMANCE_SEARCH_PLAN.md`](architecture/R2_R3_PERFORMANCE_SEARCH_PLAN.md) | R2/R3 optimization search and red-team plan: two-tier world addressing, region/scheduler design, logical time/RNG firewall, active sets, generated facts, incremental derived state, client-interest/projection sharing, network/persistence cost and qualification matrix. |
| [`R2_R3_PERFORMANCE_DECISION_REGISTER.md`](architecture/R2_R3_PERFORMANCE_DECISION_REGISTER.md) | Mandatory unresolved-mechanism register and evidence gates for high-risk performance choices. |
| [`OWNERSHIP_SIMULATION_CONTRACT.md`](architecture/OWNERSHIP_SIMULATION_CONTRACT.md) | Singular authority, migration generations, staged effects and schedule invariance. |
| [`PROTOCOL_CLIENT_SPINE.md`](architecture/PROTOCOL_CLIENT_SPINE.md) | Version-pinned client route and protocol layering. |
| [`PREPLAY_TARGET_BINDING.md`](architecture/PREPLAY_TARGET_BINDING.md) | Static target binding across the pre-Play connection spine. |
| [`COMPONENT_RESOLUTION.md`](architecture/COMPONENT_RESOLUTION.md) | Installation-time package choice with statically specialized HOT wiring. |
| [`M0_FOUNDATION_IMPLEMENTATION_SPEC.md`](architecture/M0_FOUNDATION_IMPLEMENTATION_SPEC.md) | Foundation milestone contract and implementation boundary. |
| [`WORLD_SECTION_IMPLEMENTATION_SLICE.md`](architecture/WORLD_SECTION_IMPLEMENTATION_SLICE.md) | World/section slice and its dependency/evidence shape. |

Architecture documents should separate **semantic laws** from **replaceable mechanisms**. A useful design can still be experimental; a convenient implementation detail does not become product semantics merely because it exists in code.

## Qualification

| Document | Purpose |
| --- | --- |
| [`R2B_VANILLA_PLAYTEST.md`](qualification/R2B_VANILLA_PLAYTEST.md) | Current stock-client replay-free Play-entry gate. |
| [`EVIDENCE_AND_EXPERIMENT_RECORDS.md`](qualification/EVIDENCE_AND_EXPERIMENT_RECORDS.md) | Evidence identity, experiment records and decision provenance. |
| [`PERFORMANCE_QUALIFICATION_STANDARD.md`](qualification/PERFORMANCE_QUALIFICATION_STANDARD.md) | Normative machine-state, warm-up, topology/cache/counter, tail and whole-cost rules for performance claims. |
| [`WORLD_ACCESS_BENCHMARK_HARNESS.md`](qualification/WORLD_ACCESS_BENCHMARK_HARNESS.md) | Whole-cost qualification for resolved local chunk windows versus repeated general world lookup. |
| [`SECTION_EQUIVALENCE_LAB.md`](qualification/SECTION_EQUIVALENCE_LAB.md) | Differential/equivalence qualification for section mechanisms. |
| [`SECTION_REPRESENTATION_LAB.md`](qualification/SECTION_REPRESENTATION_LAB.md) | Section representation experiments and selection criteria. |
| [`SECTION_BENCHMARK_HARNESS.md`](qualification/SECTION_BENCHMARK_HARNESS.md) | Controlled benchmark harness and measurement rules. |
| [`SECTION_SEMANTIC_FIXTURES.md`](qualification/SECTION_SEMANTIC_FIXTURES.md) | Source-backed semantic fixture contract. |
| [`SECTION_VANILLA_CORPUS.md`](qualification/SECTION_VANILLA_CORPUS.md) | Vanilla-derived section population/corpus evidence. |
| [`SECTION_VANILLA_EXTRACTOR.md`](qualification/SECTION_VANILLA_EXTRACTOR.md) | Extraction boundary for admitted vanilla evidence. |
| [`qualification/section/`](qualification/section/) | More focused section qualification records/runbooks. |

The qualification tree is intentionally detailed. Correctness and performance evidence should remain reviewable after the implementation that produced it changes.

## Execution and repository operation

| Document | Purpose |
| --- | --- |
| [`EXECUTION_MASTER_PLAN.md`](execution/EXECUTION_MASTER_PLAN.md) | Milestones, workstreams and scope-control rules. |
| [`FIRST_30_DAYS.md`](execution/FIRST_30_DAYS.md) | Initial execution plan retained as project-history context. |
| [`CI_QUALIFICATION_ROADMAP.md`](execution/CI_QUALIFICATION_ROADMAP.md) | Evidence rings, CI activation and repository governance. |
| [`PUBLIC_RELEASE_GATE.md`](execution/PUBLIC_RELEASE_GATE.md) | The audited process used to make the repository public safely. |

## Contribution, branding and security policy

These files are part of the public documentation surface even when they live outside the architecture/qualification hierarchy:

- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) — engineering rules, evidence expectations and contribution workflow.
- [`BRANDING.md`](BRANDING.md) — canonical Helve identity, motto, public description, claim boundaries and visual-asset conventions.
- [`../CLA.md`](../CLA.md) — contributor licence agreement.
- [`../SECURITY.md`](../SECURITY.md) — vulnerability reporting policy.
- [`../LICENSE`](../LICENSE) — Mozilla Public License 2.0.

## Documentation discipline

When adding or changing a document:

- identify whether it is **normative**, **implementation guidance**, **experiment design**, **evidence**, or **historical context**;
- link to the higher-level contract it implements rather than restating a competing version of the same rule;
- mark unresolved mechanisms as provisional/experimental instead of writing them as settled architecture;
- keep performance conclusions attached to exact workload, target, toolchain and evidence identity;
- never use copied Mojang source as repository documentation.

A document is useful when it makes the next engineering decision more precise, more reproducible, or harder to accidentally violate.
