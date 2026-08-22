# Section Production-Policy Decision Validator Admission — 2026-08-22

Parent: #19  
Implementation checkpoint: `18f8f189be54c07d3c3c3f1db16eb5d0d0635090`  
Status at checkpoint: **ordinary strict CI PASS**

## What this checkpoint proves

The final M0.3D selection boundary now has a separate explicit policy validator above the dimension-separated Pareto analyzer.

The validator does not discover or rank candidates. It validates a human-reviewed default choice against admitted Pareto evidence and refuses any choice that the evidence does not support.

A valid default must:

- be one of the four production candidates;
- never be the permanent `direct-reference` oracle;
- remain a global Pareto survivor;
- be present on every standard-dimension Pareto frontier;
- not be globally strictly dominated;
- if more complex than `direct`, satisfy the frozen material-complexity gate.

The policy input schema is closed. It cannot introduce dimension weights, an overall scalar score, or other unreviewed policy fields. This preserves the M0.3D `dimension-separated-only` rule through the final selection layer.

Every nonselected Pareto survivor requires an explicit non-empty rationale. Strictly dominated mechanisms inherit their mathematically validated all-dimension production dominators from the analysis. This means later code pruning cannot silently erase a surviving trade-off merely because another candidate was preferred.

A valid decision record is content-addressed and sets:

- `decision_evidence_eligible=true`;
- `production_policy_selected=true`;
- `production_pruning_ready=true`;

only after all production mechanisms have a durable classification. It also requires later candidate-registry and experiment-log updates, preserving the project rule that code may disappear but experimental knowledge may not.

## Permanent regressions

Tests admitted at this checkpoint cover:

- valid common-frontier material selection;
- selecting the simple `direct` baseline without inventing a complexity-benefit requirement;
- `direct-reference` rejection;
- globally dominated default rejection;
- rejection when the chosen candidate is absent from even one dimension frontier;
- rejection of a complex candidate with `material=false`;
- exact rationale coverage for every nonselected Pareto survivor;
- cross-dimension scoring rejection;
- Pareto analysis digest tampering;
- survivor/dominated partition corruption;
- closed policy schema rejecting hidden weighting fields;
- deterministic final decision hashing.

## Scope

No real section candidate is selected by this admission record. Real selection still requires the controlled target-hardware qualification artifact and the corresponding real Pareto analysis. This checkpoint proves that, once those measurements exist, the final policy cannot bypass the evidence firewall.
