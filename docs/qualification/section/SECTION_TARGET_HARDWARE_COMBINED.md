# Section target-hardware combined evidence protocol — M0.3D

Parent: #19  
Status: **normative measurement-orchestration contract; not a representation decision**

This document defines the boundary at which Crucible combines representative vanilla-population CPU/RSS evidence with synthetic mutation/promotion-tail evidence. It does not define the final Pareto selection rule and cannot select a production representation by itself.

## Core rule

> Population evidence and synthetic mechanism evidence may be combined only when they come from the same source revision, the same retained benchmark executable, the same pinned target, the same controlled hardware CPU, and one content-addressed orchestration session.

Two unrelated benchmark runs are not retrospectively declared a single qualification experiment merely because both passed individually.

## Layering

The combined controller sits above two already-admitted lower layers:

1. `section_target_hardware.py`
   - validates the four-seed representative-v1 benchmark packs;
   - performs the pinned offline release build;
   - retains and hashes the exact `section_bench` executable;
   - runs dimension-separated candidate-isolated population CPU/RSS children;
   - applies population noise gates.
2. `section_target_synthetic_evidence.py`
   - validates candidate-isolated replacement/churn/promotion records;
   - independently recomputes timing summaries from raw samples;
   - aggregates repeated synthetic evidence;
   - applies synthetic control/replacement/promotion-tail noise gates.
3. `section_target_combined.py`
   - invokes the population controller exactly once;
   - reopens and rehashes its retained artifact;
   - reuses the exact retained executable for every synthetic child;
   - pins every synthetic child to the same CPU;
   - verifies source/binary/pack identity before and after the synthetic phase;
   - emits one content-addressed combined evidence object.

The lower layers remain independently meaningful and independently testable. The combined layer does not duplicate their parsers or noise mathematics.

## Evidence families remain distinct

Representative-population evidence measures naturally occurring vanilla section distributions, separated by dimension:
- random reads;
- sequential section reads;
- small volume reads;
- positive/negative conservative membership queries;
- construction/materialization diagnostics;
- logical owned bytes;
- process RSS delta.

Synthetic evidence measures mechanism behavior that natural terrain may visit too rarely for reliable tails:
- same-state replacement;
- low-entropy replacement;
- high-entropy replacement;
- palette churn;
- single-replacement promotion boundaries at 2, 3, 5, 9, 17, 33, 65, 129 and 257 live states.

Synthetic evidence is dimensionless. It must never be silently duplicated into each dimension or used to invent a cross-dimension gameplay score.

## One retained executable

The population controller owns the only build in a combined session. It performs the already-admitted clean-checkout, pinned Rust 1.97.1, offline Cargo, isolated Cargo home/target, release-profile and codegen validation.

The combined controller then reopens `population/benchmark-executable`, verifies its SHA-256 against the population orchestration record, and executes that exact file for every synthetic child. No second Cargo build occurs.

The executable SHA-256 is recorded in every synthetic child summary and in the combined identity block.

## Runtime environment

Before the synthetic phase, the controller rejects non-empty compiler/profile overrides using the same forbidden-environment policy as population qualification. Synthetic children receive explicit empty `RUSTFLAGS` and `CARGO_ENCODED_RUSTFLAGS` and run directly from the retained executable under `taskset -c <CPU>`.

The repository must remain clean and at the same commit before and after every synthetic child. The retained executable and every representative pack are rehashed after the combined session.

## Scheduling

Population scheduling remains the dimension/candidate-balanced schedule defined by the population orchestrator.

Synthetic scheduling is independently candidate-position balanced. In every five qualification rounds, each of the five candidates occupies every synthetic candidate position exactly once.

The two evidence families are not compared directly as if they were the same workload. Therefore no cross-family scalar normalization is introduced here. Candidate order is balanced within each evidence family and both retain candidate-independent control workloads for drift qualification.

## Eligibility fields

The combined record exposes:
- `qualification_complete` — both evidence phases completed and were structurally validated;
- `population_evidence_eligible` — copied from the admitted population noise/protocol result;
- `synthetic_evidence_eligible` — produced by the admitted synthetic noise/protocol classifier;
- `combined_measurement_evidence_eligible` — logical AND of those two evidence-family eligibility flags;
- `decision_evidence_eligible` — **always false in this layer**.

The final flag remains false even when both measurement families qualify because the separate dimension-separated Pareto/decision record has not yet been assembled.

## Decision firewall

The combined controller preserves:
- `decision_scope = dimension-separated-only`;
- `cross_dimension_score_allowed = false`.

No weighted Overworld/Nether/End score is introduced here. A later explicit workload model would require its own admitted hypothesis and evidence; it cannot appear accidentally in the section-selection step.

The combined controller also never turns GitHub-hosted smoke timings into qualification evidence. Smoke mode is structurally useful but both lower protocol classifiers remain non-eligible at one round.

## Content-addressed output

A combined evidence directory contains:

```text
combined-orchestration.json
artifact-manifest.json
population/
  benchmark-executable
  orchestration.json
  artifact-manifest.json
  children/...
  build.stdout
  build.stderr
synthetic/
  round-00/...
  ...
```

The combined artifact manifest recursively records path, size and SHA-256 for every retained file except itself and binds them to the combined evidence SHA-256.

The population artifact is independently reopened before synthetic work begins. Every file listed by the lower population artifact is rehashed and required to match. This prevents a stale or mutated population directory from being treated as trusted input to the combined layer.

## Regression obligations

The combined controller must permanently test at least:
- compiler/profile override rejection;
- exact retained-binary binding;
- population orchestration digest validation;
- population artifact recursive rehashing;
- decision-scope/cross-dimension firewall;
- independent decision blockers for population and synthetic evidence;
- unconditional Pareto-record blocker;
- content-addressed combined artifact construction.

Hosted integration smoke additionally proves the real executable path through all 15 population children and all five synthetic children.

Any material defect discovered in this controller receives a permanent regression and an entry in the section experiment/defect ledger.

## Qualification mode

A production-target run requires the lower protocols' qualification settings, currently at least five rounds and a multiple of five. Both population and synthetic noise classifiers must admit their respective evidence.

Even then, this layer only produces **combined measurement evidence**. The next independent layer must:
1. ingest correctness qualification identity;
2. ingest this combined evidence and revalidate its artifact identities;
3. construct per-dimension Pareto comparisons;
4. retain synthetic promotion/tail constraints without fabricating dimension weights;
5. identify strict domination and material trade-offs;
6. apply the documented complexity threshold;
7. emit the explicit candidate selection/rejection record.

Only that later record may freeze the first production section policy.
