# Contributing to Crucible

Crucible uses a strict evidence-driven engineering process. Changes are evaluated against semantic correctness, architectural consequence, measurable efficiency, maintainability, and the quality of the evidence supporting them.

Crucible is currently in **M0 — Foundation and World Kernel Qualification**. There is not yet a playable server release, so the most useful contributions are usually focused on the active milestone, qualification infrastructure, documentation correctness, tests, or tightly scoped implementation work rather than broad gameplay feature additions.

## Before you start

For substantial work, read these first:

1. [`README.md`](README.md) — project status and public overview.
2. [`docs/architecture/CRUCIBLE_MASTER_BLUEPRINT.md`](docs/architecture/CRUCIBLE_MASTER_BLUEPRINT.md) — architectural intent.
3. [`docs/architecture/M0_FOUNDATION_IMPLEMENTATION_SPEC.md`](docs/architecture/M0_FOUNDATION_IMPLEMENTATION_SPEC.md) — current milestone boundary.
4. [`docs/execution/EXECUTION_MASTER_PLAN.md`](docs/execution/EXECUTION_MASTER_PLAN.md) — sequencing and scope-control rules.

If your proposal changes a semantic contract, ownership model, component boundary, HOT-path abstraction, persistence rule, or qualification model, raise the design question before writing a large implementation. Crucible intentionally prefers small falsifiable changes over speculative architecture stacks.

## Licence and contributor agreement

Crucible is licensed under the Mozilla Public License 2.0 unless a file or component clearly states otherwise. See [`LICENSE`](LICENSE).

**Contributors retain ownership of their Contributions.** Before an external contribution can be accepted, the contributor must also accept the [`Crucible Contributor Licence Agreement`](CLA.md). The CLA grants the Project Steward a perpetual, worldwide, transferable, sublicensable, royalty-free and irrevocable licence to use and relicense the Contribution, including under other open-source or proprietary/commercial terms, while leaving the contributor free to use and license their own work independently.

External pull requests must affirmatively check the CLA acceptance box in the pull request template. CI fails closed when that acceptance is absent.

Do not submit code or other material owned or controlled by an employer, client, institution, or another third party unless you have authority to grant the rights required by the CLA and MPL-2.0. The Project Steward may require separate corporate authorization before accepting such material.

## Engineering rules

1. **Do not guess Minecraft semantics.** Locate the official source/VAR, classify uncertainty, and construct evidence.
2. **Do not translate Mojang architecture.** Preserve semantic consequences; redesign mechanisms independently.
3. **Do not commit Mojang source or proprietary game artifacts.** Official source/runtime material is a local qualification oracle, not repository content.
4. **Reference before optimization.** HOT optimizations require an independent reference or equivalent oracle where feasible.
5. **No silent semantic tradeoffs.** Deliberate deviations belong to explicit non-strict profiles and machine-readable deviation records.
6. **No speculative abstraction.** New component seams, global services, ownership concepts, unsafe boundaries, and HOT dynamic dispatch require architectural review.
7. **No unbounded resource growth.** Queues, caches, scratch, and retained work need explicit governance.
8. **Keep patches bounded.** A PR should answer one main engineering question.
9. **Evidence is part of the change.** Correctness, performance, provenance, and rejected hypotheses must remain reviewable after implementation details change.

## Local validation

The pinned Rust toolchain is declared in [`rust-toolchain.toml`](rust-toolchain.toml). A useful baseline before opening a PR is:

```bash
cargo fmt \
  --package crucible-types \
  --package crucible-component \
  --package crucible-section-qualification \
  --package crucible-world-contract \
  --package crucible-world-reference \
  --package crucible-world-section \
  --package xtask \
  -- --check

cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
python3 -m unittest discover -s tools/tests -p 'test_*.py' -v
cargo xtask guard
```

Subsystem work may require additional qualification commands. Follow the relevant document under [`docs/qualification/`](docs/qualification/) and do not substitute a lightweight smoke test for evidence explicitly required by that subsystem's gate.

## PR evidence

Meaningful PRs should state:

```text
Semantic effect:
Source / VAR:
Tests:
Equivalence:
Performance:
Memory / allocations:
Concurrency / schedule:
Unsafe:
Dependencies / architecture:
Reopen trigger:
```

Use `N/A` only where a category genuinely does not apply.

A useful PR should make it possible for a future reviewer to understand **what claim changed, what evidence supports it, and what observation would force the decision to be reopened**.

## Where to look next

- [`docs/README.md`](docs/README.md) — curated documentation index.
- [`docs/qualification/EVIDENCE_AND_EXPERIMENT_RECORDS.md`](docs/qualification/EVIDENCE_AND_EXPERIMENT_RECORDS.md) — evidence/experiment model.
- [`docs/execution/CI_QUALIFICATION_ROADMAP.md`](docs/execution/CI_QUALIFICATION_ROADMAP.md) — CI and qualification governance.

Thank you for helping build Crucible carefully. The strictness is intentional: the project is trying to make ambitious performance changes without making correctness, provenance or resource behaviour implicit.
