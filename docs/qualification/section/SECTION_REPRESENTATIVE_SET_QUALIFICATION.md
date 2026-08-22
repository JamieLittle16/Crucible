# Section representative-set qualification

Status: **M0.3D population qualification; no performance decision**  
Parent: #19  
Depends on: `vanilla-section-representative-v1`

This document defines how Crucible materializes and admits the complete four-seed representative vanilla section population for later target-hardware performance qualification.

The central distinction is:

> A structurally valid four-member set is not yet a benchmark handoff. The complete population must also pass an independent admission firewall and receive a self-describing artifact manifest.

GitHub-hosted timings are never production-selection evidence.

## Evidence pipeline

```text
frozen representative-v1 plan
        ↓
fresh official 26.2 state identities
        ↓
source qualification / frozen state-data binding
        ↓
seed 0 ─┐
seed 1 ─┼─ exact official worlds, sequentially generated
seed 2 ─┼─ bounded 8-ticket batches
seed 3 ─┘
        ↓ per seed
exact 64 × 3 selected chunk columns
        ↓
`minecraft:full` census
        ↓
representative-member extraction
        ↓
independent Python corpus validation
        ↓
all-five Rust reconstruction
        ↓
individual decision gate MUST reject
        ↓
structural four-member set validator
        ↓
independent population-admission firewall
        ↓
self-describing artifact manifest
        ↓
`benchmark_handoff_eligible = true`
```

The manual workflow is:

```text
.github/workflows/section-representative-set-qualification.yml
```

It is `workflow_dispatch` only.

## Frozen workload identity

Representative-v1 remains unchanged by this qualification layer:

- policy: `vanilla-section-representative-v1`;
- plan SHA-256: `fecb9c9bc77aa9689ceaf6d88fa9af96019a48d9533269f3bd15824f7dfc7191`;
- four content-independent seeds;
- 64 chunk columns per standard dimension per seed;
- 768 selected chunk columns total;
- equal seed weighting;
- natural vertical-section weighting;
- dimensions reported separately.

The stable 192-command member-selection digest is:

```text
cb97b7490c28e38293251561749a87dbda2d0f78d78c7cf98471e5eff825a354
```

Changing seeds, coordinates, dimensions or weighting creates a new population policy. Qualification hardening does not alter the sampling law.

## Why four worlds are generated sequentially

This is population materialization, not a throughput benchmark. Sequential generation on one pinned runner:

- bounds Java heap/disk pressure;
- avoids four official servers contending for shared resources;
- keeps one runner/toolchain identity for the complete materialization;
- simplifies failure localization;
- avoids unnecessary inter-job artifact joining.

Each world generator remains internally bounded to eight same-dimension force-load tickets at a time with explicit command barriers and synchronous `save-all flush` barriers.

## Member gate

Every seed must independently produce:

```text
world-evidence.json
extraction-evidence.json
corpus-manifest.json
rust-import.json
decision-rejection.log
server.log
member.corpus
```

Each member proves at least:

- exact pinned official server identity;
- representative plan and exact derived seed;
- bounded generation command coverage;
- exact selected coordinates;
- all 192 selected chunks saved at `minecraft:full`;
- contiguous per-dimension section lattices;
- canonical source inventory/corpus identities;
- independent Python semantic/cardinality summaries;
- independent Rust reconstruction through direct-reference, direct, adaptive, fast-local and packed-local;
- per-dimension Rust candidate evidence recomposes global candidate evidence;
- `purpose = representative-member`;
- `decision_eligible = false` individually.

There is no substitute seed and no content-based retry/cherry-picking rule.

## Structural set firewall

`tools/section_corpus_set.py` validates the complete population structure. It requires:

- exactly the four frozen seed identities;
- distinct member corpus SHA-256 values;
- common target/server/plan identity;
- identical selected-coordinate schedule;
- common target dimension lattices across seeds;
- Python/Rust cardinality agreement;
- all-five candidate reconstruction diagnostics;
- exact per-dimension/global candidate recomposition;
- no individual member decision eligibility.

It emits:

```text
decision_eligible = true
decision_scope = dimension-separated-only
cross_dimension_score_allowed = false
```

This is **structural decision eligibility**. It is necessary but not sufficient for benchmark handoff.

## Independent population-admission firewall

`tools/section_population_admission.py` independently re-opens the four member evidence records and the structural set.

This second layer exists because redundant evidence can be internally wrong even when the normalized section images themselves are valid.

### Exact generation-property identity

For each frozen seed, admission recomputes the canonical `server.properties` text via the admitted official representative-world generator and requires the recorded line sequence to match exactly.

This prevents a member generated with the right server SHA, seed and coordinate plan but different terrain-affecting settings from entering the population—for example:

- `generate-structures=false`;
- a different level-generation mode;
- altered dimension-generation/server settings.

The exact seed-specific properties are retained by SHA-256 in the admission record.

### Semantic-summary coherence

For each member, global and per-dimension semantic summaries must contain exactly:

```text
cell_facts:
  non_air
  counted_fluid
  random_block
  random_fluid

section_classes:
  all_air
  contains_fluid
  random_block_present
  random_fluid_present
```

Admission enforces:

- no missing or invented keys;
- non-negative counts;
- cell counts bounded by total cells;
- class counts bounded by section count;
- `counted_fluid <= non_air`;
- `random_block <= non_air`;
- `random_fluid <= counted_fluid`;
- `random_fluid_present <= contains_fluid`;
- per-dimension section totals recompose the member;
- per-dimension fact/class counts recompose the member's global counts exactly.

The four-seed admission then preserves aggregate fact/class counts **per dimension**, never as a cross-dimension ranking score.

### Final admission record

A successful second firewall emits:

```text
kind = section-representative-set-admission
decision_eligible = true
benchmark_handoff_eligible = true
decision_scope = dimension-separated-only
cross_dimension_score_allowed = false
```

It binds:

- `population_sha256`;
- structural-set `evidence_sha256`;
- raw `corpus-set.json` file SHA-256;
- all four seed/corpus identities;
- seed-specific server-property digests;
- per-dimension semantic aggregates;
- its own canonical `admission_sha256`.

A later performance harness must consume an admitted population, not merely a raw member corpus or structural set file.

## Dimension-weighting firewall

No global candidate ranking is allowed across Overworld, Nether and End without a later explicitly justified workload model.

The decision-bearing strata remain:

```text
per_dimension.minecraft:overworld
per_dimension.minecraft:the_nether
per_dimension.minecraft:the_end
```

The cross-dimension `aggregate` object is descriptive only. It cannot contain candidate totals or a global cardinality histogram usable as an implicit score.

This prevents the target's 24:16:16 vertical lattice from silently creating a 3:2:2 gameplay weighting.

See [`SECTION_DIMENSION_WEIGHTING_GUARD.md`](SECTION_DIMENSION_WEIGHTING_GUARD.md).

## Population identity versus evidence identity

The structural set deliberately exposes two identities:

- `population_sha256` — which vanilla workload population was materialized;
- `evidence_sha256` — the validated structural evidence produced by the current mechanism/tooling diagnostics.

The independent admission layer adds:

- `set_file_sha256` — exact serialized structural-set record;
- `admission_sha256` — canonical digest of the hardened admission record.

Candidate implementation diagnostics may change `evidence_sha256` without changing `population_sha256`. Tooling/handoff changes may change the admission/artifact identity without pretending vanilla terrain changed.

## Self-describing workflow artifact

`tools/representative_set_artifact_manifest.py` runs with `if: always()` so failed qualifications still preserve diagnostics.

The resulting schema-2 artifact manifest explicitly records:

- `qualification_complete`;
- `decision_eligible`;
- `benchmark_handoff_eligible`;
- repository commit SHA;
- GitHub run ID and attempt;
- Python version;
- verbose Rust compiler identity;
- Java version;
- population/set/admission identities when present;
- relative path, size and SHA-256 for every evidence file.

Therefore an uploaded diagnostic ZIP from a failed run cannot masquerade as qualified population evidence merely because it has the expected artifact name.

For complete runs, the artifact generator re-verifies the canonical admission digest and proves `corpus-set.json` has not changed since admission.

## Regression obligations

Permanent tests cover both firewall layers and workflow orchestration. They must reject at least:

- missing/extra population members;
- duplicate member corpus identity;
- seed/plan/server mismatch;
- selected-coordinate drift;
- non-`full` selected chunks;
- section-lattice drift;
- Python/Rust cardinality disagreement;
- candidate-set/representation corruption;
- accidental individual decision eligibility;
- implicit cross-dimension scoring;
- missing/reordered/changed canonical server properties;
- semantic-summary missing/extra keys;
- semantic counts outside section/cell bounds;
- invalid semantic subset relationships;
- per-dimension/global semantic recomposition mismatch;
- structural set mutation after evidence digesting;
- corpus-set mutation after population admission;
- population-admission digest corruption;
- partial diagnostic artifact claiming qualification completion.

The manual workflow itself runs its own workflow-contract regression.

## First real member

The completed seed-0 official probe established the real 26.2 member path and produced 3,584 sections / 14,680,064 cells / 432 states.

Its exact evidence identities, semantic distributions and deterministic representation-byte sanity signals are preserved in [`SECTION_REPRESENTATIVE_SEED0_ADMISSION.md`](SECTION_REPRESENTATIVE_SEED0_ADMISSION.md).

That member remains individually decision-ineligible.

## Relationship to performance qualification

A green four-member admitted artifact supplies **workload population evidence**, not qualifying timing.

The next layer consumes the exact admitted corpora on controlled target hardware and must preserve dimensions separately while recording:

- exact population/admission identities;
- commit/toolchain/codegen identity;
- CPU/core affinity;
- governor/frequency/turbo context;
- repeated raw samples/noise;
- steady-state real-corpus read/query measurements;
- controlled synthetic mutation/promotion tail measurements;
- isolated process/RSS evidence per candidate/dimension.

Corpus parsing and construction setup must not be accidentally timed as steady-state section access. Process-RSS runs must isolate candidates so input/parser memory does not become candidate representation memory.

Only the combined correctness + real-population + target-hardware + RSS/tail evidence may enter the #19 Pareto decision.

## Exit condition

This population layer is complete only when one exact workflow run proves:

1. all four frozen members pass independently;
2. every member remains individually decision-ineligible;
3. member corpus identities are distinct;
4. target dimension lattices agree;
5. structural set validation passes;
6. independent population admission passes;
7. `benchmark_handoff_eligible = true`;
8. dimensions remain separate;
9. population/evidence/admission identities are archived;
10. the schema-2 artifact manifest says `qualification_complete = true` on the exact run/commit.

Only then may representative-v1 be used as production-decision workload evidence.
