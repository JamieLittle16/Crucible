# Section target-hardware orchestrator admission — M0.3D

Parent: #19  
Depends on: merged #43 (`d1c24e488caf922a0b1284bb9af66fd93add8877`)  
Implementation PR: #44  
Target: Minecraft Java 26.2

This record preserves the admission evidence for the controller that turns an already-qualified representative section population into repeated candidate-isolated target-hardware evidence.

It is an **orchestrator admission record**, not a representation-selection record. Hosted timing and RSS values remain diagnostic and are never production-decision evidence.

## Precondition admitted by #43

The candidate-isolated child and benchmark-pack boundary were squash-merged as:

`d1c24e488caf922a0b1284bb9af66fd93add8877`

On the exact #43 head before merge, all relevant gates passed:

- ordinary strict CI;
- Section Full Qualification;
- Section Benchmark Smoke;
- Section Target Benchmark Smoke;
- Section Corpus Probe;
- Section Representative Member Probe.

The merged child protocol provides one-candidate/one-dimension processes with exact semantic reconstruction, deterministic logical owned-byte accounting, signed process-RSS delta after explicit common-buffer prefaulting, construction evidence, steady-state read/query timings, candidate-independent control work, release/codegen identity, CPU affinity and NUMA/memory-affinity provenance.

## Clean #44 history checkpoint

#44 was originally developed stacked on the pre-merge #43 branch so CI could attack the controller while the expensive source-backed gate completed.

After #43 squash-merged, #44 was reconstructed directly on the new `main` rather than retaining duplicated dependency history.

Clean implementation checkpoint:

`a917160c02bf766840d2b100cf3813c258f5d433`

The clean PR delta contains only:

1. `.github/workflows/section-target-orchestrator-smoke.yml`;
2. `docs/qualification/section/SECTION_TARGET_HARDWARE_ORCHESTRATOR.md`;
3. `tools/section_target_hardware.py`;
4. `tools/tests/test_section_target_hardware.py`.

This history cleanup does not change the controller implementation from the already-tested stacked v2 design.

## Controller trust boundary

The controller fails closed unless it can independently bind:

```text
clean repository source
        ↓
pinned Rust 1.97.1 / exact rustc commit
        ↓
sanitized offline release build
        ↓
exact retained section_bench executable SHA-256
        ↓
independently revalidated representative-v1 benchmark packs
        ↓
exact per-pack SHA-256 before and after each child
        ↓
taskset-pinned one-candidate / one-dimension child
        ↓
independently validated child evidence
        ↓
repeated-run integer median/MAD noise qualification
        ↓
content-addressed orchestration + artifact manifests
```

Pack and output roots must be outside the repository. The checkout must be completely clean. Compiler/profile overrides and parent Cargo settings capable of changing binary generation are rejected.

The exact benchmark executable is retained in the evidence artifact; the controller does not treat a child-reported commit string as proof of binary identity.

## Scheduling law

Every round measures all five candidate identities across all three standard dimensions.

Dimension order rotates by round. Candidate order rotates independently within each dimension. Over every five rounds, every candidate occupies every candidate-order position exactly once in each dimension.

This is intended to prevent systematic first/last or cold/hot slot bias.

The qualification protocol requires at least five rounds and a round count divisible by five. One-round hosted CI is therefore structurally useful but intentionally ineligible for population qualification.

## Noise policy

Raw evidence is retained; samples are never deleted or rewritten to manufacture stability.

The v1 qualification gates are:

| Signal | Maximum relative MAD |
|---|---:|
| candidate-independent control | 5% |
| repeated production-candidate steady workload p50 | 10% |
| production-candidate RSS delta | 10% |

Production RSS evidence additionally requires a positive repeated-run median.

The permanent direct reference is retained and measured, but reference-only RSS/timing instability does not by itself invalidate otherwise-stable production-candidate population evidence. Its candidate-independent control work remains part of global drift detection.

All aggregates remain dimension-separated. Cross-dimension scoring is forbidden without a separately admitted workload model.

## Clean hosted smoke evidence

Workflow: `Section Target Orchestrator Smoke`  
Run: `32599748172`  
Head: `a917160c02bf766840d2b100cf3813c258f5d433`

The workflow performed a real controlled offline build, selected one actually allowed Linux CPU, constructed content-addressed smoke packs for all three dimensions, and launched all 15 dimension × candidate child processes through the real controller under `taskset`.

Evidence identities:

- Actions artifact ZIP SHA-256: `3eb6e3db466e97dcd8c0ef9fc3a9f9042c25406190b385e2a4ee9e24d3bb9be7`;
- orchestration evidence SHA-256: `b3a6331f653683ca06e555fa1694a849a3628816d5d8b767adb9e0df643c1560`;
- artifact-manifest SHA-256: `4bed0622bda68f26b3571fc2f3df199503ca456a4e53ba3ed99f6cb71fef2330`;
- retained benchmark executable SHA-256: `e6b99d801c4e028c73f753ee287a79829865312e044488a0f4f4200a3cb45255`;
- smoke pack-manifest SHA-256: `dbf2500f4c17e14bc136e0c02d56899cec9d00d669283fe266f91704005ba327`;
- child processes: `15`;
- rounds: `1`.

Observed controller state:

```text
qualification_complete = true
population_evidence_eligible = false
decision_evidence_eligible = false
```

The population gate was false for the correct reason: the hosted smoke used one round, while population qualification requires at least five rounds and a multiple of five. On this smoke, the control/workload/RSS noise checks themselves were structurally evaluable; none may be interpreted as target-hardware performance evidence.

The exact retained executable hash matched every child binding, every retained child JSON hash matched the orchestration record, and Cargo build scratch was excluded from the sealed artifact.

## Heavy regression classes

The controller tests permanently cover:

- representative-policy drift;
- pack byte corruption and manifest-digest drift;
- dimension-set drift and forbidden cross-dimension scoring;
- duplicate seed/corpus identities and member-count mismatch;
- unsafe pack paths;
- hidden Rust/compiler/release-profile overrides;
- binary-affecting parent Cargo configuration;
- deterministic dimension rotation;
- five-round candidate-position balance within every dimension;
- child commit/candidate/codegen/population/affinity tampering;
- missing memory-node provenance;
- signed RSS arithmetic;
- representation-count and construction-sample binding;
- workload-set/sample-count drift;
- stable five-round population admission;
- one-round smoke non-eligibility;
- high run-to-run drift downgrade;
- negative or unstable production RSS downgrade;
- preservation of otherwise-valid production evidence when only the reference candidate has unstable RSS.

## What this admits

Once #44 is merged, Crucible will have a reproducible mechanism for producing **population CPU/tail/RSS evidence** on controlled target hardware from the admitted four-seed representative-v1 population.

It still does not make the section decision complete.

The remaining evidence layers are intentionally explicit:

1. candidate-isolated synthetic replacement/churn/promotion-tail evidence through the same controlled source→binary→CPU/noise boundary;
2. actual repeated qualification runs on target hardware rather than GitHub-hosted timing;
3. dimension-separated Pareto assembly combining correctness, real-population CPU/tail/RSS, logical bytes and synthetic transition/tail evidence;
4. a committed final decision record naming selected mechanism/profile policy and preserving explicit rejection rationale for every losing candidate;
5. removal of losing mechanisms from the production engine path while retaining their candidate and experiment records.

Until those are complete, `decision_evidence_eligible` must remain false.
