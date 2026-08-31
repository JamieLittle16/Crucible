# R2C World-State Admission Doctor

Status: **source-free diagnostic tooling; not semantic admission**  
Target: **Minecraft: Java Edition 26.2 / protocol 776 / DataVersion 4903**  
Parent: `R2C_WORLD_STATE_ADMISSION_PROMOTION.md`

## Purpose

The R2C world-state evidence chain intentionally contains two human judgment gates:

1. inspect the local official-source excerpts and decide which sources genuinely support each biome, heightmap and light law;
2. author the actual source-free SEM rules supported by that completed review.

`tools/r2c_world_state_admission_doctor.py` does **not** automate either judgment. It makes the remaining obligations explicit so reviewers do not need to discover them one fail-fast exception at a time.

The doctor emits source-free JSON only. It never prints `source_excerpt`, never invents semantic statements, and finally reuses the authoritative finalizer/materializer/promotion validators before declaring a phase ready.

## Phase 1 — source review

```bash
python3 tools/r2c_world_state_admission_doctor.py review \
  --review-pack /tmp/helve-r2c-world-state-review/review-pack.json \
  --worksheet /tmp/helve-r2c-world-state-review/worksheet.json
```

For each of `R2C-BIOMES`, `R2C-HEIGHTMAPS`, and `R2C-LIGHT`, the doctor checks and reports all of these obligations in one pass:

- source explicitly inspected;
- review explicitly complete;
- at least one selected source;
- every candidate exactly partitioned into selected or rejected;
- no selected/rejected overlap;
- no unresolved follow-up dependency;
- at least one semantic observation;
- every Atlas hazard on selected sources explicitly reviewed.

When no blockers remain, the doctor runs the same authoritative worksheet validator used by `r2c_world_state_source_review_finalize.py`.

## Phase 2 — semantic admission authoring

```bash
python3 tools/r2c_world_state_admission_doctor.py admission \
  --review-result /tmp/helve-r2c-world-state-review/review-result.json \
  --worksheet /tmp/helve-r2c-world-state-review/admission.json
```

The doctor checks:

- the worksheet is bound to the exact completed review result;
- every group is explicitly admission-complete;
- the selected-source set has not drifted from review;
- every group has at least one explicit SEM rule;
- SEM ids use the stable `SEM-NET-R2C-WORLD-*` namespace and are globally unique;
- statements are non-empty canonical text;
- every rule cites at least one source selected for that group;
- no rule cites a source selected only elsewhere;
- every selected source supports at least one semantic rule.

Automatic semantic inference remains forbidden. A blocker such as `semantic-rule-required` means a reviewer must author the rule from the inspected source evidence; it is not an invitation for tooling to guess one.

When no blockers remain, the doctor invokes the exact materializer validator before declaring the worksheet ready.

## Phase 3 — source gate and promotion readiness

Before the bound Atlas gate:

```bash
python3 tools/r2c_world_state_admission_doctor.py staging \
  --staging-dir /tmp/helve-r2c-world-state-admission
```

The expected blocker at this point is:

```text
manifest-bound-source-gate-required
```

The doctor deliberately names the required R2C wrapper:

```bash
python3 tools/r2c_world_state_source_gate.py \
  --db .crucible/vanilla/atlas.sqlite \
  --staging-dir /tmp/helve-r2c-world-state-admission \
  --output /tmp/helve-r2c-world-state-source-gate.json
```

After that gate:

```bash
python3 tools/r2c_world_state_admission_doctor.py staging \
  --staging-dir /tmp/helve-r2c-world-state-admission \
  --gate-report /tmp/helve-r2c-world-state-source-gate.json
```

Only the same manifest-bound report accepted by `r2c_world_state_admission_promote.py` produces phase `promotion-ready`.

This also prevents the older materializer prose that mentions the generic source gate from being mistaken for the final R2C.4 promotion path. R2C world-state promotion requires `r2c_world_state_source_gate.py`, because it binds the complete materialization manifest and therefore the staged SEM Markdown as well as the VAR/gate bytes.

## Exit codes

- `0`: the requested phase is ready for its next explicit step;
- `2`: artifacts are structurally readable but one or more review/admission/gate blockers remain;
- `1`: the doctor invocation or prerequisite artifact is malformed and cannot be diagnosed safely.

A non-zero result is fail-closed. The doctor never edits the review worksheet, admission worksheet, staging bundle, gate report, or repository evidence.

## Non-claims

The doctor does not establish that:

- any biome, heightmap or light semantic law is correct before human review;
- source review can be replaced with method-name or call-graph inference;
- an authored SEM rule is admitted before the manifest-bound Atlas gate;
- admitted evidence means runtime behavior is implemented;
- tooling-only diagnostics require or justify a runtime performance benchmark.

Its role is narrower: expose every remaining explicit obligation, reuse the canonical validators, and make it difficult to accidentally skip a stage.