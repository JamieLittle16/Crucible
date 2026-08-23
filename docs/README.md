# Crucible documentation

Crucible's documentation is organized around **authority, implementation, and evidence**. Higher-level contracts own project policy; lower-level documents implement, test or qualify those contracts and must not silently weaken them.

If you are new to the project, do not try to read everything in repository order. Use the paths below.

## Start here

### I want to understand what Crucible is

1. [`architecture/CRUCIBLE_MASTER_BLUEPRINT.md`](architecture/CRUCIBLE_MASTER_BLUEPRINT.md) — the concise architectural narrative: semantic fidelity, efficiency, concurrency, replaceability and the reference/production split.
2. [`execution/EXECUTION_MASTER_PLAN.md`](execution/EXECUTION_MASTER_PLAN.md) — how the project advances by qualified vertical slices rather than feature accumulation.
3. [`../README.md`](../README.md) — public project status, contributor quick-start and current milestone.

### I want to work on the current milestone

1. [`architecture/M0_FOUNDATION_IMPLEMENTATION_SPEC.md`](architecture/M0_FOUNDATION_IMPLEMENTATION_SPEC.md) — the M0 boundary and acceptance model.
2. [`architecture/WORLD_SECTION_IMPLEMENTATION_SLICE.md`](architecture/WORLD_SECTION_IMPLEMENTATION_SLICE.md) — the current world/section implementation slice.
3. [`qualification/SECTION_EQUIVALENCE_LAB.md`](qualification/SECTION_EQUIVALENCE_LAB.md) — how section implementations are checked against their reference semantics.
4. [`qualification/SECTION_REPRESENTATION_LAB.md`](qualification/SECTION_REPRESENTATION_LAB.md) — representation candidates and the performance questions they must answer.

### I want to understand how claims become evidence

1. [`qualification/EVIDENCE_AND_EXPERIMENT_RECORDS.md`](qualification/EVIDENCE_AND_EXPERIMENT_RECORDS.md) — durable evidence and experiment records.
2. [`qualification/PERFORMANCE_QUALIFICATION_STANDARD.md`](qualification/PERFORMANCE_QUALIFICATION_STANDARD.md) — project-wide rules for warm-up, machine state, cache/topology effects, counters, statistical discipline and low-level optimization admission.
3. [`execution/CI_QUALIFICATION_ROADMAP.md`](execution/CI_QUALIFICATION_ROADMAP.md) — how qualification becomes enforceable CI/repository policy.
4. [`qualification/SECTION_BENCHMARK_HARNESS.md`](qualification/SECTION_BENCHMARK_HARNESS.md) — benchmark methodology for the current foundational subsystem.
5. [`qualification/SECTION_SEMANTIC_FIXTURES.md`](qualification/SECTION_SEMANTIC_FIXTURES.md) — source-backed semantic fixtures.

## Architecture

| Document | Purpose |
| --- | --- |
| [`CRUCIBLE_MASTER_BLUEPRINT.md`](architecture/CRUCIBLE_MASTER_BLUEPRINT.md) | End-to-end architectural narrative and project identity. |
| [`M0_FOUNDATION_IMPLEMENTATION_SPEC.md`](architecture/M0_FOUNDATION_IMPLEMENTATION_SPEC.md) | Current milestone contract and implementation boundary. |
| [`WORLD_SECTION_IMPLEMENTATION_SLICE.md`](architecture/WORLD_SECTION_IMPLEMENTATION_SLICE.md) | Current world/section slice and its dependency/evidence shape. |

Architecture documents should separate **semantic laws** from **replaceable mechanisms**. A useful design can still be experimental; a convenient implementation detail does not become product semantics merely because it exists in code.

## Qualification

| Document | Purpose |
| --- | --- |
| [`EVIDENCE_AND_EXPERIMENT_RECORDS.md`](qualification/EVIDENCE_AND_EXPERIMENT_RECORDS.md) | Evidence identity, experiment records and decision provenance. |
| [`PERFORMANCE_QUALIFICATION_STANDARD.md`](qualification/PERFORMANCE_QUALIFICATION_STANDARD.md) | Normative machine-state, warm-up, topology, cache/counter, tail and whole-cost rules for performance claims. |
| [`SECTION_EQUIVALENCE_LAB.md`](qualification/SECTION_EQUIVALENCE_LAB.md) | Differential/equivalence qualification for section mechanisms. |
| [`SECTION_REPRESENTATION_LAB.md`](qualification/SECTION_REPRESENTATION_LAB.md) | Section representation experiments and selection criteria. |
| [`SECTION_BENCHMARK_HARNESS.md`](qualification/SECTION_BENCHMARK_HARNESS.md) | Controlled benchmark harness and measurement rules. |
| [`SECTION_SEMANTIC_FIXTURES.md`](qualification/SECTION_SEMANTIC_FIXTURES.md) | Source-backed semantic fixture contract. |
| [`SECTION_VANILLA_CORPUS.md`](qualification/SECTION_VANILLA_CORPUS.md) | Vanilla-derived section population/corpus evidence. |
| [`SECTION_VANILLA_EXTRACTOR.md`](qualification/SECTION_VANILLA_EXTRACTOR.md) | Extraction boundary for admitted vanilla evidence. |
| [`qualification/section/`](qualification/section/) | More focused M0.3 section qualification records/runbooks. |

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
- [`BRANDING.md`](BRANDING.md) — canonical motto, public description, claim boundaries and visual-asset conventions.
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
