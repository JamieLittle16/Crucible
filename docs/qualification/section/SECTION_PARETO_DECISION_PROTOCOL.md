# Section Pareto decision protocol — M0.3D

Parent: #19  
Status: **normative production-selection contract**

This document defines the final evidence-to-decision boundary for Crucible's first production block-section representation policy. It consumes already-qualified correctness and performance evidence. It does not run benchmark mechanisms itself and it does not invent workload weights to force a winner.

## Principle

> The decision layer reports the Pareto facts that the admitted evidence actually supports. It must refuse to choose when a unique production policy is not justified.

Correctness is a prerequisite, not a scored axis. A correctness-failing or unqualified mechanism is removed before performance comparison.

## Required inputs

A decision attempt consumes two evidence families from the **same repository source revision and frozen target**.

### 1. Full M0.3C correctness evidence

Required for each production candidate:
- `direct`;
- `adaptive`;
- `fast-local`;
- `packed-local`.

Every record must be full qualification, PASS, and bind to the same Minecraft/protocol/data version plus generated state-data identity. Candidate trace surfaces/fingerprints must agree exactly where the qualification format requires them to agree.

The direct-reference implementation remains a permanent oracle and benchmark baseline; it is not a production candidate.

### 2. Controlled combined M0.3D measurement evidence

The input must be a qualification-mode artifact produced by the admitted combined target-hardware controller:
- at least five balanced rounds and a multiple of five;
- `qualification_complete = true`;
- `population_evidence_eligible = true`;
- `synthetic_evidence_eligible = true`;
- `combined_measurement_evidence_eligible = true`;
- `decision_evidence_eligible = false` at the measurement layer;
- the only remaining blocker must be the absent Pareto selection record;
- `decision_scope = dimension-separated-only`;
- `cross_dimension_score_allowed = false`.

The decision tool reopens and rehashes the combined artifact rather than trusting a copied JSON summary. It also reopens lower population child records and synthetic child records needed for independent recomputation.

## Identity closure

Before comparing any metric, the decision assembler must prove:
- one repository commit across correctness and performance evidence;
- Minecraft 26.2 / protocol 776 / data version 4903;
- one generated state-data input/generation identity;
- one representative population/admission identity for all population children;
- one retained benchmark executable identity for the whole combined measurement session;
- exact candidate set and standard dimension set;
- exact artifact file hashes and sizes.

Any mismatch fails closed. The tool must never "mostly match" evidence from different revisions.

## Recompute, do not trust

The decision layer independently recomputes the values it uses:
- repeated population p50 aggregates from retained child records;
- repeated population RSS-delta aggregates;
- deterministic logical-owned-byte, maximum-owned-byte, transition and logical-allocation values from retained child records;
- repeated synthetic replacement-p50 aggregates;
- repeated synthetic promotion-p99 aggregates;
- combined/root evidence digests and artifact inventories.

Compact lower-layer aggregates are cross-checks, not sole authorities.

Deterministic representation/memory quantities must be identical across repeated children for a candidate/dimension. Drift in a quantity that is supposed to be deterministic is a qualification error, not benchmark noise.

## Comparison axes

All production candidates are compared on the following evidence surfaces.

### Dimension-local representative-population axes

For each of:
- `minecraft:overworld`;
- `minecraft:the_nether`;
- `minecraft:the_end`;

retain separately:
- random-read p50;
- sequential-full-section-read p50;
- small-volume-read p50;
- `maybe_contains` positive p50;
- `maybe_contains` negative p50;
- process RSS loaded delta median;
- logical owned bytes;
- maximum per-section owned bytes;
- construction transitions;
- logical backing allocation events.

CPU and resident/logical memory are first-class decision dimensions. Transition/allocation counts are retained as mechanism-cost diagnostics and may explain a result; they are not silently converted into synthetic nanoseconds or bytes.

### Dimensionless synthetic mechanism axes

Retain separately:
- same-state replacement p50;
- low-entropy replacement p50;
- high-entropy replacement p50;
- palette-churn p50;
- promotion p99 at 2, 3, 5, 9, 17, 33, 65, 129 and 257 live states.

Every synthetic scenario keeps its pattern, requested cardinality, observed cardinality and representation transition identity. Candidate-specific representation names are provenance, not a reason to compare mismatched semantic scenarios.

## No cross-dimension scalar

The selector may not assign weights to Overworld, Nether and End and may not calculate a single blended "gameplay score".

A candidate that is excellent in one dimension and worse in another remains an explicit trade-off unless another admitted workload model exists. This M0.3D decision does not create such a model.

Synthetic measurements are dimensionless and must not be copied three times to manufacture dimension-local evidence.

## Numeric Pareto relation

For metrics where lower is better, candidate A **numerically dominates** candidate B only when:
1. A is no worse than B on every scored numeric axis across every dimension-local and dimensionless scenario; and
2. A is strictly better on at least one axis.

This relation is reported, but small differences are not automatically sufficient to justify permanent mechanism complexity.

## Materiality relation

Issue #19's engineering threshold is made explicit rather than applied informally.

A change is **materially better** when the lower-is-better metric improves by at least:
- 5% for CPU/latency/tail metrics;
- 10% for resident or logical byte metrics.

A change is a **material regression** at the symmetric corresponding threshold.

Integer comparison is performed without floating-point rounding:

```text
A materially improves B by T ppm
iff A * 1_000_000 <= B * (1_000_000 - T)
```

with T = 50,000 ppm for latency/tail and T = 100,000 ppm for bytes.

The decision record stores exact integer ratios/ppm deltas used for every materiality statement.

## Practical/material dominance

Candidate A **materially dominates** candidate B only when:
1. A has no material regression versus B on any scored CPU/tail or memory axis;
2. A has at least one material improvement versus B; and
3. neither candidate has a correctness/protocol qualification failure.

This relation intentionally allows sub-threshold noise-sized differences in either direction to be treated as ties for the purpose of complexity justification.

The exact numeric relation remains recorded alongside it; material dominance never rewrites raw measurements.

## Complexity rule

The tool does **not** hard-code a total aesthetic ranking of `adaptive`, `fast-local` and `packed-local`. Their implementation complexity is qualitatively different, and pretending that one scalar complexity number is objective would violate the evidence philosophy.

Instead:
- `direct` is the simple production baseline;
- an optimized mechanism must demonstrate at least one material benefit over the simpler mechanism(s) it is proposed to replace without a disqualifying material regression;
- if multiple optimized candidates remain on the material Pareto frontier, the assembler reports all of them and requires an explicit profile/budget choice rather than manufacturing a winner;
- a candidate whose only advantages are below the materiality threshold cannot justify additional permanent mechanism complexity over an otherwise equivalent simpler candidate.

Any final human-selected profile choice among surviving Pareto candidates must be represented explicitly in the committed decision record, with the supporting budget/trade-off stated. It cannot be hidden inside the analysis code.

## Selection outcomes

The assembler may produce exactly one of three outcomes.

### `unique-default-supported`

Exactly one production candidate survives all correctness gates and the documented numeric/material Pareto analysis with no unresolved material trade-off relevant to the default strict-fidelity profile.

The record may nominate it as the first production default.

### `explicit-profile-frontier`

Two or more candidates remain genuinely non-dominated and represent meaningful CPU/memory/tail trade-offs.

The record must list the frontier. A later explicit profile decision may retain more than one mechanism only when the trade-off is material and operationally useful.

### `insufficient-or-unstable-evidence`

Any required evidence is missing, mismatched, noisy/ineligible, or the comparison surface is incomplete.

No production policy may be frozen.

## Rejection records

Every rejected candidate receives a permanent structured entry containing:
- stable candidate ID from `SECTION_CANDIDATE_REGISTRY.md`;
- correctness status;
- numeric dominators, if any;
- material dominators, if any;
- exact axes causing rejection;
- material improvements/regressions relative to the selected/frontier mechanisms;
- raw artifact/evidence digests;
- whether the mechanism is to be removed from production linkage after the decision;
- any experiment deliberately retained for future targets/profiles.

Deleting implementation code does not delete this record.

## Decision record

The machine-readable final record must include:
- schema/version;
- source commit and frozen target identities;
- correctness evidence identities;
- combined measurement evidence/artifact identities;
- exact hardware/toolchain identity;
- candidate set;
- per-dimension representative-population tables;
- dimensionless synthetic tables;
- deterministic memory/allocation tables;
- numeric pairwise dominance matrix;
- material pairwise dominance matrix;
- Pareto frontier;
- selected candidate/profile outcome, if justified;
- rejected candidates and exact reasons;
- unresolved trade-offs;
- materiality thresholds;
- `cross_dimension_score_allowed = false`;
- a content digest over the complete record.

A human-readable companion document may summarize this record, but it may not contradict or omit material trade-offs present in the machine-readable evidence.

## Post-decision code rule

After a production policy is frozen:
1. selected production mechanism(s) enter the normal engine path;
2. experimental losers are removed from production linkage and normal runtime dispatch;
3. the permanent direct reference remains as a qualification oracle;
4. loser history, evidence and rationale remain in committed documentation;
5. benchmark-only candidate implementations may remain only when deliberately isolated from production and their maintenance cost is justified by an active experiment. Otherwise they are deleted after their historical record is preserved.

## M0.3D exit

M0.3D is complete only after:
- full correctness evidence is bound;
- controlled combined target-hardware qualification is eligible;
- the Pareto assembler validates and emits the complete decision record;
- the selected policy is committed;
- candidate registry/experiment log are updated;
- losing mechanisms are removed from production linkage according to the decision;
- HOT-path architecture is re-audited for accidental dynamic dispatch, synchronization or global lookup.

At that point Crucible can move from section-representation laboratory work into the next World Kernel/server-feature slice with a frozen, evidence-backed storage primitive.
