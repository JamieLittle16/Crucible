# Contributing to Crucible

Crucible is currently in a strict private foundational phase. Changes are evaluated against semantic correctness, architectural consequence, measurable efficiency, and maintainability.

## Rules

1. **Do not guess Minecraft semantics.** Locate the official source/VAR, classify uncertainty, and construct evidence.
2. **Do not translate Mojang architecture.** Preserve semantic consequences; redesign mechanisms independently.
3. **Reference before optimization.** HOT optimizations require an independent reference or equivalent oracle where feasible.
4. **No silent semantic tradeoffs.** Deliberate deviations belong to explicit non-strict profiles and machine-readable deviation records.
5. **No speculative abstraction.** New component seams, global services, ownership concepts, unsafe boundaries, and HOT dynamic dispatch require architectural review.
6. **No unbounded resource growth.** Queues, caches, scratch, and retained work need explicit governance.
7. **Keep patches bounded.** A PR should answer one main engineering question.

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

The current execution plan lives under `docs/execution/`.
