# Section target-hardware combined evidence admission — M0.3D

Parent: #19  
Protocol: [`SECTION_TARGET_HARDWARE_COMBINED.md`](SECTION_TARGET_HARDWARE_COMBINED.md)  
Status: **hosted structural smoke admitted; production target-hardware qualification still required**

This record admits the combined population + synthetic orchestration boundary as functioning qualification infrastructure. It does **not** admit GitHub-hosted timings as production performance evidence and does not select a section representation.

## Admitted checkpoint

Implementation branch head exercised by the corrected hosted smoke:

`391d58e1a24af6d9661736df89859cde86925af5`

GitHub pull-request workflows execute a synthetic merge ref rather than the branch head itself. The emitted benchmark records therefore bind their repository source identity to the workflow checkout commit:

`881f706bb96ba57a783d0a346523057e9c883204`

That distinction is expected and is recorded explicitly rather than silently relabeling the evidence as the branch head.

Hosted artifact:
- workflow: `Section Target Combined Smoke`;
- artifact name: `section-target-combined-smoke`;
- artifact ZIP SHA-256: `c5932897b505de701ae38a8f1f974305527b20609f53cd7a329239886fd08c8f`;
- retention class: diagnostic / short-lived;
- hosted timing status: **non-qualifying**.

## Exact evidence identities

Corrected combined smoke emitted:
- combined evidence SHA-256: `b8e2d5f082a092695dcc1b17460f77c405715793350c50d196f57f46860864dc`;
- combined artifact-manifest SHA-256: `3c52cdd1b2040100b948028b864efdeff4cf35d799e03f493b80e1f0298e14de`;
- retained benchmark executable SHA-256: `3fdee28c67f6ba047b808de7850d30fcd075dc05fcee86b6d46b3f8558293199`;
- lower population orchestration evidence SHA-256: `b754f0f39473b46bae9746b15ed6dbacff12913fcdbdb2d23efc0a26a193aa31`;
- lower population artifact-manifest SHA-256: `9792308c231d37acc1e6eb783c0aa9748ad283f29339aa14377d383e8e5435e5`.

The smoke fixture intentionally uses synthetic placeholder population/admission identities, because its purpose is orchestration structure rather than workload qualification. The real representative-v1 target-hardware run must use the admitted four-seed pack set and its actual provenance chain.

## Executed topology

One combined smoke session performed exactly one controlled build and then ran 20 child benchmark processes from the retained executable:
- 15 population children = 3 standard dimensions × 5 candidates;
- 5 synthetic children = one per candidate.

Candidates:
- direct-reference;
- direct;
- adaptive;
- fast-local;
- packed-local.

Population dimensions:
- `minecraft:overworld`;
- `minecraft:the_nether`;
- `minecraft:the_end`.

Every synthetic child recorded the same retained executable SHA-256 as the combined and lower population records.

## Artifact closure

The corrected combined artifact contained 66 recursively hashed retained files:
- 50 under `population/`;
- 15 under `synthetic/`;
- `combined-orchestration.json`.

The root `artifact-manifest.json` is intentionally excluded from its own file inventory. The nested lower-layer `population/artifact-manifest.json` is intentionally **included** and hashed.

This makes the lower population evidence manifest itself part of the upper combined chain of custody.

## Defect discovered by first integration smoke

The first full 20-child combined execution itself completed successfully, but the post-run artifact validation failed.

Defective shape:

```python
for path in sorted(output_dir.rglob("*")):
    if not path.is_file() or path.name == "artifact-manifest.json":
        continue
```

The intent was to prevent the combined root manifest from listing itself. The basename test instead excluded **every nested file** named `artifact-manifest.json`, including the already-validated population artifact manifest.

Impact:
- benchmark execution and measurements were not corrupted;
- the upper evidence inventory had an incomplete chain of custody;
- the combined evidence boundary therefore correctly failed admission.

Fix:

```python
root_manifest = output_dir / "artifact-manifest.json"
for path in sorted(output_dir.rglob("*")):
    if not path.is_file() or path == root_manifest:
        continue
```

Permanent regression:
- creates both a root `artifact-manifest.json` and nested `population/artifact-manifest.json`;
- requires the root manifest to be excluded;
- requires the nested population manifest to be retained;
- recomputes the combined manifest digest from the emitted content.

This defect is another example of why evidence tooling itself is treated as qualification-critical software.

## Eligibility behavior observed

The one-round hosted smoke produced:
- `qualification_complete = true`;
- `population_evidence_eligible = false`;
- `synthetic_evidence_eligible = false`;
- `combined_measurement_evidence_eligible = false`;
- `decision_evidence_eligible = false`.

Both lower noise surfaces were numerically stable during the smoke:
- population control/workload/RSS noise checks: PASS;
- synthetic control/replacement/promotion-tail noise checks: PASS.

Both evidence families remained correctly ineligible solely because production qualification requires at least five rounds and a multiple of five. Hosted smoke must never bypass that protocol rule.

The combined decision blockers were exactly:
1. population evidence did not pass protocol/noise eligibility;
2. synthetic mechanism evidence did not pass protocol/noise eligibility;
3. dimension-separated Pareto selection record not assembled.

## Decision firewall

The admitted combined layer preserves:
- `decision_scope = dimension-separated-only`;
- `cross_dimension_score_allowed = false`;
- `decision_evidence_eligible = false` unconditionally at this layer.

Synthetic mechanism evidence remains dimensionless. No Overworld/Nether/End weighting has been invented.

## Admission decision

Admit the combined orchestration **mechanism** once the exact documented PR head passes normal CI and the corrected combined integration smoke.

Do not use hosted smoke timings to choose a production representation.

The next independent evidence/decision step is:
1. run this combined controller on controlled target hardware with the real representative-v1 pack set for at least five balanced rounds;
2. require both population and synthetic eligibility gates to pass;
3. retain the complete content-addressed artifact;
4. combine it with full M0.3C correctness evidence in a separate Pareto assembler;
5. produce the dimension-separated selection/rejection record required by #19.
