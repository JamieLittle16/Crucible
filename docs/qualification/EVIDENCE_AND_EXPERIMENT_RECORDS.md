# Qualification, experiment, and decision records

Status: **normative engineering process**

Helve treats performance and parity work as an evidence-producing process. Source code is allowed to evolve aggressively; the reasoning that caused a mechanism to be introduced, retained, changed, or deleted must remain recoverable.

The governing principle is:

> **Code can disappear; experimental knowledge cannot.**

A rejected implementation should normally be removed from the production build, but its hypothesis, evidence, result, and rejection reason remain in committed documentation.

## Record layers

Helve uses five complementary record types.

### 1. Semantic specification

Defines the observable law that implementations must satisfy. It is derived from pinned official source/runtime evidence and is representation-independent.

Examples:
- `SEM-WORLD-SECTION-*` rules;
- target-data manifests;
- source/runtime qualification records.

A semantic rule may invalidate implementation evidence when the target changes.

### 2. Candidate registry

One durable entry per mechanism or materially distinct policy considered for a subsystem.

Each entry records:
- stable candidate ID and human name;
- status: `active`, `selected`, `rejected`, `superseded`, `deferred`, or `reference-only`;
- hypothesis being tested;
- representation/mechanism shape;
- expected benefit and expected cost;
- first implementation/PR or commit;
- correctness evidence IDs;
- performance artifact/decision references when available;
- known defects and important fixes;
- final disposition and rationale.

Deleting candidate code does **not** delete the registry entry.

### 3. Experiment log

Chronological laboratory notebook for material experiments and discoveries.

Each entry records:
- date and subsystem;
- question/hypothesis;
- exact candidate/configuration;
- commit SHA;
- workload/fixture/evidence identity;
- result;
- interpretation;
- follow-up decision.

Routine formatting/build failures do not belong here. Results that changed our understanding or architecture do.

### 4. Machine-readable evidence artifact

Qualification and benchmark runs emit artifacts tied to exact code and target identity. These are the primary numerical/test evidence, not prose summaries.

Artifacts must record enough provenance to reproduce or invalidate the result, including as applicable:
- commit SHA;
- Minecraft/protocol/data version;
- generated-data/input digests;
- candidate/profile identity;
- trace/workload/harness version and seed;
- build profile/toolchain/target flags;
- hardware/OS identity for performance runs;
- CPU affinity and relevant frequency/governor/turbo context;
- raw samples or sufficient underlying measurements.

Committed prose links to artifact identifiers/digests rather than copying only headline numbers.

### 5. Decision record

A freeze/delete decision is a synthesis over evidence, not a benchmark output.

A decision record states:
- scope and decision date;
- candidates considered;
- correctness prerequisites;
- workload definitions;
- raw artifact digests;
- Pareto/noise analysis;
- selected mechanism(s) and profile mapping;
- rejected mechanisms and exact reason;
- thresholds/configuration frozen by the decision;
- unresolved hypotheses explicitly deferred;
- requalification triggers.

A production mechanism is not considered frozen until this record exists.

## Performance evidence admission ladder

Performance evidence is not binary. It moves through explicit states, and a lower state never acquires the authority of a higher state merely because its numbers look convincing.

```text
hosted diagnostic artifact
        ↓ explicit controlled-run action
qualified target-hardware artifact
        ↓ independent repeated runs
cross-process consistency report
        ↓ human review
accepted baseline / decision record
```

### Hosted diagnostic artifact

Purpose:
- prove the harness builds and runs;
- continuously protect semantic/structural invariants;
- detect catastrophic regressions or suspicious direction changes;
- retain useful debugging samples.

It may **not** select a production mechanism or freeze a timing threshold unless a subsystem specification explicitly establishes a stronger hosted-run policy.

### Qualified target-hardware artifact

A subsystem that uses target-hardware timing must define a deliberate operator path rather than relying on an ordinary benchmark invocation that happens to run on a suitable machine. Where practical, the artifact should carry an explicit witness that required controls such as CPU affinity were enforced.

This prevents accidental promotion of:
- GitHub-hosted runs;
- ad-hoc local runs;
- reduced smoke workloads;
- runs with incomplete provenance.

### Cross-process consistency report

At least three independent processes are the default minimum for a new performance baseline unless the subsystem's qualification specification justifies another rule.

Mechanical combination should fail closed on mismatched:
- code/commit identity;
- workload/schema identity;
- stable hardware/toolchain identity;
- semantic witness;
- required controlled-run provenance.

The combiner may compute medians, MAD/variation, tails and direction stability. It must not silently promote those calculations into a mechanism selection.

### Accepted baseline / decision

Human review remains required for machine quietness, outliers, frequency/governor context, whole-cost relevance and complexity trade-offs. A production mechanism is selected only by the relevant decision record.

## Status semantics

`active`
: Implemented and admitted to the current laboratory.

`selected`
: Qualified and selected for at least one production profile by a committed decision record.

`rejected`
: Tested and intentionally removed/not admitted because evidence showed it was incorrect or dominated.

`superseded`
: Replaced before final performance selection because a corrected mechanism preserves its hypothesis while fixing a known design defect.

`deferred`
: A legitimate hypothesis intentionally postponed because current evidence does not justify its complexity or prerequisite infrastructure is absent.

`reference-only`
: Retained as an independent correctness oracle and explicitly not eligible for production performance selection.

## Evidence discipline

### Correctness and performance are separate gates

Performance cannot compensate for semantic failure. A correctness-failing candidate is ineligible for performance selection until corrected and requalified.

### Structural evidence is permanent when timing is not

If two semantically identical paths differ in counted allocations, lookups, copies, queue operations or other deterministic structural work, those counts may form permanent CI evidence even when hosted timing is too noisy to be authoritative.

This is preferred over turning a noisy timing ratio into a correctness threshold.

### Negative results are first-class results

A dominated or broken design is valuable project knowledge. Its record should explain why it looked plausible and what evidence falsified the hypothesis. Future contributors should not have to rediscover the same dead end.

### Do not canonize temporary numbers

GitHub-hosted benchmark timing is diagnostic only unless a qualification specification explicitly says otherwise. Production performance decisions require controlled target-hardware evidence.

### Raw evidence before narrative

Prefer retaining raw samples/evidence artifacts and deriving tables from them. Narrative summaries are useful indexes, not substitutes for evidence.

### Exact identity, not “latest”

Records use immutable commit/artifact/digest identities. Statements such as “the latest benchmark” are not adequate decision evidence.

### Never cherry-pick convenient processes

When a protocol requires multiple independent runs, preserve and combine the complete declared run set. Do not discard an inconvenient process because another run looks cleaner. If a run is invalid because a documented environmental condition failed, record that invalidation explicitly and rerun the complete required set where the protocol requires it.

### Requalification is explicit

A decision record names what invalidates it. Typical triggers include:
- Minecraft target change;
- semantic rule change;
- generated target-data digest change;
- candidate implementation change on a measured path;
- compiler/codegen policy change;
- benchmark workload/harness change;
- materially different target hardware/profile constraints.

## Pull requests and issues

Issues and PRs are the discussion/audit trail. Committed records are the canonical concise technical history.

A material experiment PR should update the relevant candidate registry and experiment log in the same change when practical. A production-selection PR must include the final decision record.

A PR description may summarize current hosted timing for review convenience, but it must label that timing with its evidence class and must not make the PR text the only retained copy of material evidence.

## Anti-patterns

Do not:
- leave multiple losing implementations linked into production “for history”;
- delete a failed mechanism without recording why;
- select a mechanism from one microbenchmark;
- paste unqualified hosted-runner timing into a production decision;
- call an ordinary local run “target-hardware evidence” without the subsystem's required provenance controls;
- cherry-pick the best process from a multi-process protocol;
- convert a diagnostic timing ratio into a correctness gate when deterministic structural evidence exists;
- silently change thresholds after a decision record;
- make prose the only copy of raw evidence;
- treat an implementation name as a semantic contract.

This process is intentionally strict. The objective is that, years later, a contributor can answer **what we tried, what happened, why the surviving design exists, what evidence class supports each claim, and exactly what would justify changing it**.
