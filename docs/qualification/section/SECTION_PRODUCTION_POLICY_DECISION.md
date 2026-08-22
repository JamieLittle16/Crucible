# Section Production Policy Decision — M0.3D

Parent: #19  
Depends on: dimension-separated Pareto analysis  
Status: **normative final-selection contract; no candidate selected by this document**

## Purpose

The Pareto analyzer answers which mechanisms are supported or rejected by the evidence. This layer is deliberately separate: it records the explicit engineering decision that freezes Crucible's first production block-section policy.

The separation is intentional. A mathematical frontier is not product policy, and a tool must never silently convert one into a winner.

## Inputs

A final decision requires:

1. one canonical `section-pareto-decision-analysis` record produced from controlled target-hardware qualification and full M0.3C correctness evidence;
2. one explicit human-reviewed policy specification naming the desired default production candidate and explaining every nonselected Pareto survivor.

The validator re-verifies the analysis digest before considering the policy specification.

## Default candidate rules

The default candidate must:

- be one of `direct`, `adaptive`, `fast-local`, or `packed-local`;
- never be the `direct-reference` oracle;
- remain in the analysis `pareto_survivors` set;
- lie on the Pareto frontier in **every** standard dimension;
- not appear in `strictly_dominated_candidates`;
- if it is not `direct`, carry `material=true` in the frozen material-benefit analysis.

This means a more complicated representation cannot become the default merely because it is technically non-dominated. It must also earn its complexity under the #19 materiality rule.

## No hidden workload weighting

The selection record inherits:

- `decision_scope=dimension-separated-only`;
- `cross_dimension_score_allowed=false`.

A policy specification cannot provide an Overworld/Nether/End weighting or scalar score. If the analysis has no common all-dimension frontier candidate, the current single-default contract refuses to freeze a default. The project must gather stronger evidence or explicitly design and admit a different profile/dimension policy in a later specification.

## Nonselected candidates

Every nonselected production candidate must end in exactly one durable class:

### Strictly dominated

The decision record inherits the analyzer's valid all-dimension production dominators. No free-text rationale is required to establish the mathematical rejection, although explanatory notes may be attached to the engineering record.

### Nonselected Pareto survivor

A surviving candidate cannot disappear merely because another survivor was preferred. The policy specification must provide a non-empty rationale explaining why that trade-off does not justify production complexity for the first default.

Typical legitimate reasons include:

- improvement fails the material complexity threshold;
- memory gain is real but not worth a latency/tail regression for the default profile;
- latency gain is real but memory cost exceeds the initial production budget;
- mechanism remains useful as a documented future profile experiment but is deliberately deferred.

The validator does not invent these reasons.

## Freeze readiness

A valid final record sets:

- `decision_evidence_eligible=true`;
- `production_policy_selected=true`;
- `production_pruning_ready=true` only when every nonselected production mechanism is classified and every required survivor rationale is present.

This does **not** delete code. It authorizes the next mechanical cleanup slice to:

1. move the selected mechanism into the normal engine path;
2. remove losing experimental mechanisms from production linkage;
3. retain reference/oracle code needed for qualification;
4. update the candidate registry and experiment ledger with exact decision evidence and rejection reasons.

## Profiles

M0.3's first final-selection schema freezes one default mechanism only.

Performance/memory/balanced profiles remain permitted future outcomes, but they require their own explicit profile semantics and budget contracts. They are not smuggled into this record as ad-hoc score weights.

## Output record

The validator emits canonical `section-production-policy-decision` evidence containing:

- exact Pareto analysis SHA-256 and source commit;
- target/population/executable identities inherited from the analysis;
- chosen default candidate;
- per-dimension proof that it belongs to every frontier;
- material-benefit evidence for the selected candidate;
- mathematically dominated candidates and their dominators;
- nonselected Pareto survivors and their supplied rationales;
- the unchanged dimension-separation firewall;
- pruning readiness;
- a canonical decision SHA-256.

## Loser preservation

Production pruning must never erase the experiment record. The candidate registry and chronological experiment log remain authoritative historical records for rejected mechanisms, including their designs, defects, benchmark strengths, benchmark weaknesses, evidence digests and rejection rationale.

**Code can disappear; experimental knowledge cannot.**

## Regression obligations

Permanent tests must prove at least:

- a valid common-frontier material candidate can be selected;
- `direct` can be selected without an artificial complexity-benefit requirement;
- `direct-reference` is rejected;
- a globally dominated candidate is rejected;
- a candidate absent from one dimension frontier is rejected;
- a complex candidate with `material=false` is rejected;
- a nonselected Pareto survivor without rationale blocks pruning readiness;
- dominated candidates cannot be relabelled as survivors by the policy file;
- cross-dimension scoring cannot be enabled by policy metadata;
- Pareto analysis digest corruption is rejected;
- final decision digest is deterministic.
