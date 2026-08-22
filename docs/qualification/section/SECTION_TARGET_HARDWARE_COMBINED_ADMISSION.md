# Section target-hardware combined evidence admission — M0.3D

Parent: #19  
Protocol: [`SECTION_TARGET_HARDWARE_COMBINED.md`](SECTION_TARGET_HARDWARE_COMBINED.md)  
Status: **hosted structural smoke admitted; production target-hardware qualification still required**

This record admits the combined population + synthetic orchestration boundary as functioning qualification infrastructure. It does **not** admit GitHub-hosted timings as production performance evidence and does not select a section representation.

## Admitted checkpoints

The combined controller first completed its full 20-child hosted path on implementation head:

`391d58e1a24af6d9661736df89859cde86925af5`

That run discovered and then verified the nested-manifest provenance fix described below.

A later statistical-hardening head:

`2024b61d80f878144cb87248cb23a3755514a72a`

closed a second qualification-tool defect: MAD alone could ignore one extreme result among five repeated rounds. Normal CI and the full combined 20-child smoke both PASSed on that strengthened head.

GitHub pull-request workflows execute a synthetic merge ref rather than the branch head itself. The final strengthened smoke records repository source identity:

`5ac6516e3487c9d7a789c8cfc533ea11e89f054d`

That distinction is expected and is recorded explicitly rather than silently relabeling the evidence as the branch head.

## Latest strengthened hosted artifact

Workflow: `Section Target Combined Smoke`  
Artifact: `section-target-combined-smoke`

Latest strengthened smoke identities:
- Actions artifact ZIP SHA-256: `eee58c5ba6b6c8a27058051930b1687a0f9a7c7c0eecc6792a83ae5e645d56be`;
- combined evidence SHA-256: `61c8fbab9a1bfc5ea4309bc2f503f7cdf5927c0d0be44460814305bc5a7f84bd`;
- combined artifact-manifest SHA-256: `be4004a232eb25443d97bb62379a6e38ffa21a2b09ef15cf2ab7f2510e053d57`;
- retained benchmark executable SHA-256: `3fdee28c67f6ba047b808de7850d30fcd075dc05fcee86b6d46b3f8558293199`;
- lower population orchestration evidence SHA-256: `e589cd227684f0c81f6ac3fc64342f446430e2f696cbec695460c49bfe2fb3a3`;
- lower population artifact-manifest SHA-256: `a8cb5a90b5f58d98b58a796f07ff71318e33e9e62126b6c0f1a059e10cc7a47a`.

Retention class: diagnostic / short-lived.  
Hosted timing status: **non-qualifying**.

The smoke fixture intentionally uses synthetic placeholder population/admission identities, because its purpose is orchestration structure rather than workload qualification. The real representative-v1 target-hardware run must use the admitted four-seed pack set and its actual provenance chain.

## Executed topology

One combined smoke session performs exactly one controlled build and then runs 20 child benchmark processes from the retained executable:
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

Every synthetic child records the same retained executable SHA-256 as the combined and lower population records.

## Artifact closure

The corrected combined artifact contains 66 recursively hashed retained files:
- 50 under `population/`;
- 15 under `synthetic/`;
- `combined-orchestration.json`.

The root `artifact-manifest.json` is intentionally excluded from its own file inventory. The nested lower-layer `population/artifact-manifest.json` is intentionally **included** and hashed.

This makes the lower population evidence manifest itself part of the upper combined chain of custody.

## Defect 1 — nested population manifest omitted by basename filter

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
- recomputes the combined manifest digest from emitted content.

## Defect 2 — MAD alone did not detect an isolated catastrophic repeat

Normal CI later exercised the synthetic noise classifier with five-round examples in which exactly one round was multiplied by 10.

The intended contract said these cases must downgrade evidence. The implementation used only median absolute deviation (MAD).

For a sequence such as:

```text
x, x, x, x, 10x
```

median = `x`, and four of five absolute deviations are zero. Therefore MAD = `0`, so a pure MAD gate calls the repeated measurement centrally stable despite one catastrophic excursion.

This is mathematically expected behavior for a robust statistic, but it was too permissive for Crucible's qualification boundary.

**Decision:** retain MAD for central repeatability and add a separate maximum-relative-deviation guard across repeated summary values.

Frozen synthetic repeat-noise ceilings:

| Surface | relative MAD | maximum relative deviation |
|---|---:|---:|
| candidate-independent control p50 | 5% | 15% |
| production replacement p50 | 10% | 30% |
| production promotion p99 | 15% | 45% |

The maximum-deviation ceiling is defined as exactly three times the corresponding MAD ceiling, rather than as an unrelated magic number.

This is **not** the raw timing sample `max_ns`. It is maximum run-to-run deviation of the repeated summary metric from its median.

Permanent regressions now prove:
- a 10× isolated control excursion fails even when MAD is zero;
- a 10× isolated replacement excursion fails even when MAD is zero;
- a 10× isolated promotion excursion fails even when MAD is zero;
- modest isolated variance below the looser excursion ceiling remains admissible;
- excursion thresholds remain exactly `3 ×` their corresponding MAD thresholds.

## Latest hosted-smoke eligibility behavior

The strengthened one-round hosted smoke produced:
- `qualification_complete = true`;
- `population_evidence_eligible = false`;
- `synthetic_evidence_eligible = false`;
- `combined_measurement_evidence_eligible = false`;
- `decision_evidence_eligible = false`.

The synthetic sub-gates on that noisy hosted runner were:
- replacement repeat-noise: PASS;
- promotion-tail repeat-noise: PASS;
- candidate-independent control repeat-noise: **FAIL isolated-excursion guard**.

The control failure is useful evidence that the new guard is active. It does not make the structural smoke workflow fail, because hosted smoke is specifically an execution/integrity test and is never performance-eligible. Production qualification additionally requires at least five balanced rounds and a multiple of five on controlled target hardware.

The combined decision blockers therefore remain conservative:
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

Admit the combined orchestration **mechanism** once the final documented PR head passes normal CI and the combined 20-child smoke.

Do not use hosted smoke timings to choose a production representation.

The next independent evidence/decision step is:
1. run this combined controller on controlled target hardware with the real representative-v1 pack set for at least five balanced rounds;
2. require both population and synthetic eligibility gates to pass, including MAD and isolated-excursion checks;
3. retain the complete content-addressed artifact;
4. combine it with full M0.3C correctness evidence in a separate Pareto assembler;
5. produce the dimension-separated selection/rejection record required by #19.
