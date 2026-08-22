# Qualification, experiment, and decision records

Status: **normative engineering process**

Crucible treats performance and parity work as an evidence-producing process. Source code is allowed to evolve aggressively; the reasoning that caused a mechanism to be introduced, retained, changed, or deleted must remain recoverable.

The governing principle is:

> **Code can disappear; experimental knowledge cannot.**

A rejected implementation should normally be removed from the production build, but its hypothesis, evidence, result, and rejection reason remain in committed documentation.

## Record layers

Crucible uses five complementary record types.

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

### Negative results are first-class results

A dominated or broken design is valuable project knowledge. Its record should explain why it looked plausible and what evidence falsified the hypothesis. Future contributors should not have to rediscover the same dead end.

### Do not canonize temporary numbers

GitHub-hosted benchmark timing is diagnostic only unless a qualification specification explicitly says otherwise. Production performance decisions require controlled target-hardware evidence.

### Raw evidence before narrative

Prefer retaining raw samples/evidence artifacts and deriving tables from them. Narrative summaries are useful indexes, not substitutes for evidence.

### Exact identity, not “latest”

Records use immutable commit/artifact/digest identities. Statements such as “the latest benchmark” are not adequate decision evidence.

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

## Anti-patterns

Do not:
- leave multiple losing implementations linked into production “for history”;
- delete a failed mechanism without recording why;
- select a mechanism from one microbenchmark;
- paste unqualified hosted-runner timing into a production decision;
- silently change thresholds after a decision record;
- make prose the only copy of raw evidence;
- treat an implementation name as a semantic contract.

This process is intentionally strict. The objective is that, years later, a contributor can answer **what we tried, what happened, why the surviving design exists, and exactly what evidence would justify changing it**.