# M0.3D final section qualification driver

Parent: #19  
Depends on: representative-v1 admission, target-hardware population/synthetic qualification, Pareto analysis, and the same-commit full correctness bundle  
Status: **measurement-to-Pareto orchestration contract; never an automatic production-policy choice**

## Purpose

M0.3D already has independent tools for:

1. representative vanilla population admission;
2. content-addressed benchmark-pack generation;
3. controlled candidate-isolated population CPU/RSS measurement;
4. controlled candidate-isolated synthetic replacement/promotion measurement;
5. combined evidence and repeat-noise qualification;
6. full M0.3C correctness evidence;
7. dimension-separated Pareto analysis;
8. explicit human-reviewed production-policy selection.

The final qualification driver removes manual glue from steps 1–7 without collapsing their trust boundaries.

> One command may assemble the evidence chain. It may not choose the winner.

The separate `section_policy_decision.py` boundary remains the only place where a human-authored default-candidate specification can become a final production-policy decision record.

## Command

Run from a completely clean checkout of the exact revision being qualified:

```text
python3 tools/section_m03d_qualification.py \
  --repo-root . \
  --representative-root /absolute/path/to/admitted-representative-set \
  --correctness-bundle /absolute/path/to/crucible-section-full-bundle \
  --output-root /absolute/path/to/new/qualification-session \
  --cpu <isolated-or-selected-cpu> \
  --rounds 5
```

`--rounds` must be at least five and a multiple of five. The current normative default is five.

The representative artifact, correctness bundle and output root must live outside the repository. Both evidence inputs must be real non-symlink directories, must be disjoint from one another, and the output root must be a fresh nonexistent path disjoint from both inputs.

## Preflight

Before any benchmark pack is built, the driver requires:

- the requested CPU is inside the current process affinity mask;
- the checkout is the exact Git repository root and completely clean;
- the representative artifact root is a real non-symlink external directory;
- the correctness bundle root is a real non-symlink external directory;
- the representative and correctness evidence roots do not overlap;
- the sealed correctness bundle revalidates against the exact current Git commit;
- the output root is external, fresh and cannot mutate either input artifact.

A correctness bundle from a different source revision is rejected before benchmark work begins. An invalid CPU is rejected before pack generation.

## Step 1 — benchmark packs

The driver invokes the admitted benchmark-pack builder against the representative-v1 artifact.

The pack builder independently revalidates:

- the representative artifact manifest;
- population admission identity;
- all four seed corpora;
- target state-data identity;
- dimension-separated section counts;
- corpus hashes while streaming;
- the no-cross-dimension-score firewall.

The driver independently recomputes the returned pack-manifest digest and records the representative artifact-manifest, population and admission identities.

Output:

```text
packs/
  pack-manifest.json
  minecraft_overworld.section-pack
  minecraft_the_nether.section-pack
  minecraft_the_end.section-pack
```

## Step 2 — controlled combined measurement

The driver invokes `section_target_combined.orchestrate` in qualification mode, never smoke mode.

The lower controller retains its own full protocol:

- one pinned offline release build;
- exact retained executable SHA-256;
- CPU pinning through `taskset`;
- dimension/candidate-balanced population scheduling;
- candidate-isolated RSS evidence;
- candidate-position-balanced synthetic scheduling;
- population and synthetic control workloads;
- central MAD gates;
- isolated-excursion maximum-relative-deviation gates;
- exact repository/binary/pack rehashing;
- recursive content-addressed artifact manifests.

The final driver additionally requires:

- `mode = qualification`;
- `qualification_complete = true`;
- `decision_evidence_eligible = false` at the measurement layer;
- `decision_scope = dimension-separated-only`;
- `cross_dimension_score_allowed = false`;
- source revision still equals the preflight commit;
- the combined evidence digest recomputes exactly.

## Step 3 — noise firewall

A completed measurement session and an eligible measurement session are different facts.

If either population or synthetic evidence fails its protocol/noise gates:

- all generated raw evidence remains retained;
- the driver does **not** invoke the Pareto analyzer;
- `measurement_evidence_eligible = false`;
- `pareto_analysis_complete = false`;
- `decision_review_ready = false`;
- the completed noisy session is sealed with explicit blockers.

This is a valid experimental outcome, not permission to rerun until a preferred candidate wins. A rerun requires a fresh output root and remains a separately identified experiment.

## Step 4 — Pareto analysis

Only eligible combined measurement evidence reaches Pareto analysis.

The driver passes the exact four child `full.json` files from the already-revalidated sealed correctness bundle. The Pareto analyzer independently reopens and validates those records against the combined measurement source commit.

The existing Pareto rules remain unchanged:

- direct-reference is never selectable;
- all standard dimensions remain separate;
- no hidden dimension weights or cross-dimension scalar score exist;
- synthetic stress metrics remain dimensionless shared mechanism constraints;
- a candidate is globally dominated only under the admitted all-dimension rule;
- additional mechanism complexity requires material evidence (approximately >=5% CPU/tail or >=10% memory beyond noise under the frozen rule);
- the analyzer does not choose a winner.

The driver independently recomputes the returned Pareto-analysis digest and requires `winner_selected = false`.

## Step 5 — end-of-session source revalidation

Before any top-level session can be sealed, the driver checks again that:

- the repository is still at the exact original clean commit;
- the correctness-bundle root is still a real external input;
- the sealed correctness bundle still validates and its bundle SHA-256 is unchanged;
- the representative-artifact root is still a real external input;
- the representative population can be independently re-admitted;
- its population, admission, artifact-manifest and policy identities still equal the identities used to construct the benchmark packs;
- the combined artifact-manifest digest recomputes.

A repository mutation, correctness-artifact mutation, representative-population mutation or evidence-chain digest drift prevents the top-level session seal.

## Step 6 — final seal ordering

`qualification-session.json` is deliberately the **last file written**.

The driver first constructs the complete session object and its `session_sha256` in memory. It serializes those exact bytes in memory, then recursively hashes the existing output tree. The artifact manifest includes a virtual entry for the exact future session bytes (path, size and SHA-256), and rejects any symlink in the generated output tree.

The driver then writes:

1. `session-artifact-manifest.json`;
2. `qualification-session.json` as the final seal.

Therefore an interruption or manifest-construction failure cannot leave a `qualification-session.json` that falsely appears to represent a fully sealed experiment. An artifact manifest without the final session file is incomplete by inspection; the reverse state is never intentionally produced.

## Output states

A successful orchestration can finish in three materially different states.

### A. Measurement completed but noise-ineligible

```text
session_complete = true
measurement_evidence_eligible = false
pareto_analysis_complete = false
decision_review_ready = false
production_policy_selected = false
```

Raw evidence is useful diagnostically but cannot influence production selection.

### B. Measurement eligible, Pareto complete, no common selectable default

```text
session_complete = true
measurement_evidence_eligible = true
pareto_analysis_complete = true
decision_review_ready = false
production_policy_selected = false
```

This is a legitimate mathematical/engineering outcome. It may imply multiple profiles, a missing explicit workload model, or that the current candidate family does not yield one all-dimension default. The driver does not invent weights to force an answer.

### C. Decision review ready

```text
session_complete = true
measurement_evidence_eligible = true
pareto_analysis_complete = true
decision_review_ready = true
decision_evidence_eligible = false
production_policy_selected = false
```

This means the evidence is ready for explicit human review. It does **not** mean a default has been selected.

The next step is to author a closed `section-production-policy-spec` and run the admitted `section_policy_decision.py` validator.

## Session evidence

The top-level output is:

```text
qualification-session/
  packs/
  combined/
  pareto-analysis.json          # only when Pareto was permitted to run
  session-artifact-manifest.json
  qualification-session.json    # final seal; written last
```

`qualification-session.json` binds:

- exact repository commit;
- CPU and round count;
- sealed correctness-bundle SHA-256;
- representative policy/population/admission/artifact-manifest identities;
- benchmark-pack manifest SHA-256;
- combined evidence SHA-256;
- combined artifact-manifest SHA-256;
- Pareto analysis SHA-256 when present;
- decision-scope firewall;
- eligibility state and blockers.

It has its own `session_sha256`.

`session-artifact-manifest.json` recursively hashes every generated evidence file except itself and includes the exact future session-file bytes in its inventory. This gives final human review one content-addressed experiment directory rather than a collection of loosely related command outputs.

## Failure semantics

A lower-layer exception or cross-layer invariant failure does not intentionally produce `qualification-session.json`.

Partial files may remain for diagnosis, but the absence of the final session seal means the experiment is incomplete and unusable for selection.

The driver never cleans up, overwrites or automatically retries a failed/noisy run. A later run uses a new output directory so failed evidence remains inspectable and cannot be silently replaced.

## Permanent regression obligations

The driver must permanently test at least:

- valid measurement -> Pareto chain with no automatic policy selection;
- noise-ineligible combined evidence never reaches Pareto;
- Pareto-complete but selection-not-ready is representable;
- fresh-output requirement;
- input/output path disjointness;
- symlinked input-root rejection;
- minimum/multiple-of-five qualification rounds;
- invalid CPU rejection before expensive work;
- decision-scope and cross-dimension-score firewall;
- repository mutation after measurement prevents a session seal;
- correctness bundle is reopened after measurement;
- representative population is re-admitted after measurement;
- exactly the four sealed correctness records feed Pareto;
- every lower-layer content digest is independently recomputed;
- recursive top-level artifact inventory and manifest digest.

Any defect discovered while running the real qualification receives a permanent regression and an experiment-log entry before the final policy is frozen.

## What this deliberately does not do

The driver does not:

- select a default candidate;
- generate a hidden weighted score;
- combine dimensions into a scalar;
- weaken correctness because a candidate is faster;
- rerun automatically when noise gates fail;
- delete losing implementations;
- edit the candidate registry;
- emit a final production-policy decision.

Those last three actions occur only after the explicit production-policy record has been reviewed and sealed.
