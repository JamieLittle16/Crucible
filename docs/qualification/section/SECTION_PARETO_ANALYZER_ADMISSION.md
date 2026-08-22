# Section Pareto Analyzer Admission — 2026-08-22

Parent: #19  
Implementation checkpoint: `11817124928ac9b045aad748af10986cd8ee99e4`  
Status at checkpoint: **ordinary strict CI PASS**

## What was admitted

The M0.3D decision pipeline now has a code-level boundary between combined controlled measurement evidence and an eventual explicit production-policy selection.

The admitted analyzer:

- accepts only complete qualification-mode combined evidence;
- requires population, synthetic and combined measurement eligibility;
- enforces at least five balanced rounds and a multiple-of-five schedule;
- preserves `dimension-separated-only` interpretation and rejects cross-dimension scoring;
- distinguishes 40-hex Git source identity from 64-hex SHA-256 evidence identity;
- recursively re-hashes combined and nested population artifact files;
- recomputes population aggregates and noise qualification from retained child summaries;
- recomputes synthetic aggregates and noise qualification from retained child summaries;
- re-opens raw population child evidence to recover deterministic logical owned bytes, max section bytes, construction transitions, logical backing allocations and representation census;
- refuses deterministic memory/representation evidence that changes between repeated rounds of the same dimension/candidate;
- requires same-commit full M0.3C semantic evidence for `direct`, `adaptive`, `fast-local`, and `packed-local` with the frozen full trace operation counts/fingerprint/SEM surface;
- builds per-dimension lower-is-better metric vectors without scalar workload weighting;
- treats `direct-reference` as diagnostic-only and never as a production dominator;
- computes strict Pareto dominance separately in Overworld, Nether and End;
- allows global rejection only when the same production candidate dominates another in all three dimensions;
- records the approximate 5% CPU/tail and 10% memory complexity-justification thresholds using integer ppm arithmetic;
- emits a canonical analysis digest but deliberately leaves `decision_evidence_eligible=false` and `winner_selected=false`.

## Test pressure

The admission test does not rely on a tiny hand-authored metric table alone. It constructs a complete fabricated qualification tree containing:

- five repeated rounds;
- all three standard dimensions;
- all five benchmark mechanisms;
- 75 population child summaries plus 75 raw population child evidence files;
- 25 repeated synthetic mechanism summaries over the frozen qualification replacement/promotion surface;
- nested population and combined content-addressed manifests;
- four full semantic qualification records.

The analyzer must re-open and reconcile that tree successfully.

Additional permanent adversarial regressions cover:

- hosted/smoke or otherwise ineligible evidence rejection;
- artifact-byte corruption;
- correctness commit and trace fingerprint drift;
- Git-SHA/SHA-256 provenance type confusion;
- equality not being strict dominance;
- the reference oracle never eliminating a production candidate;
- one-dimension domination not becoming a global rejection;
- all-dimension domination requiring a valid production dominator;
- deterministic memory evidence drift across rounds;
- candidate-specific representation names not contaminating synthetic metric identity;
- exact integer 5%/10% materiality boundaries;
- deterministic canonical analysis hashing.

## Scope

No real mechanism was selected at this checkpoint. The fabricated evidence exists only to test the analyzer mathematics and evidence firewall. Real selection still requires a controlled target-hardware qualification artifact from the representative-v1 population plus the same-commit full correctness evidence.

The next boundary is the explicit `section-production-policy-decision` record, which validates a human-reviewed default choice against this analysis and requires a durable rationale for every nonselected Pareto survivor.
