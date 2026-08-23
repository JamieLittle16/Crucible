# Contributing to Crucible

Crucible uses a strict evidence-driven engineering process. Changes are evaluated against semantic correctness, architectural consequence, measurable efficiency, maintainability, and the quality of the evidence supporting them.

## Licence and contributor agreement

Crucible is licensed under the Mozilla Public License 2.0 unless a file or component clearly states otherwise. See [`LICENSE`](LICENSE).

**Contributors retain ownership of their Contributions.** Before an external contribution can be accepted, the contributor must also accept the [`Crucible Contributor Licence Agreement`](CLA.md). The CLA grants the Project Steward a perpetual, worldwide, transferable, sublicensable, royalty-free and irrevocable licence to use and relicense the Contribution, including under other open-source or proprietary/commercial terms, while leaving the contributor free to use and license their own work independently.

External pull requests must affirmatively check the CLA acceptance box in the pull request template. CI fails closed when that acceptance is absent.

Do not submit code or other material owned or controlled by an employer, client, institution, or another third party unless you have authority to grant the rights required by the CLA and MPL-2.0. The Project Steward may require separate corporate authorization before accepting such material.

## Rules

1. **Do not guess Minecraft semantics.** Locate the official source/VAR, classify uncertainty, and construct evidence.
2. **Do not translate Mojang architecture.** Preserve semantic consequences; redesign mechanisms independently.
3. **Do not commit Mojang source or proprietary game artifacts.** Official source/runtime material is a local qualification oracle, not repository content.
4. **Reference before optimization.** HOT optimizations require an independent reference or equivalent oracle where feasible.
5. **No silent semantic tradeoffs.** Deliberate deviations belong to explicit non-strict profiles and machine-readable deviation records.
6. **No speculative abstraction.** New component seams, global services, ownership concepts, unsafe boundaries, and HOT dynamic dispatch require architectural review.
7. **No unbounded resource growth.** Queues, caches, scratch, and retained work need explicit governance.
8. **Keep patches bounded.** A PR should answer one main engineering question.
9. **Evidence is part of the change.** Correctness, performance, provenance, and rejected hypotheses must remain reviewable after implementation details change.

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

The current execution plan lives under `docs/execution/`.
