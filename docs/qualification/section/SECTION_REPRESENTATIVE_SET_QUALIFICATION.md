# Section representative-set qualification

Status: **M0.3D qualification workflow; no performance decision**  
Parent: #19  
Depends on: `vanilla-section-representative-v1`

This document defines how Crucible materializes the complete four-seed vanilla section population after the representative sampling law has been admitted.

The workflow is deliberately separate from target-hardware benchmarking. Its job is to prove that the workload population exists, is exactly the frozen population, and can be reconstructed by every qualified section mechanism. It does not use GitHub-hosted timing to choose a representation.

## Workflow

The manual workflow is:

```text
.github/workflows/section-representative-set-qualification.yml
```

It is `workflow_dispatch` only. Routine pull requests continue to use the single-member representative probe plus synthetic/full semantic qualification; generating four official worlds on every code change would add substantial cost without improving the semantic gate.

One qualification run performs:

```text
frozen representative-v1 plan
        ↓
fresh official 26.2 runtime state identities
        ↓
committed source-qualification binding
        ↓
exact frozen generated-state input digest
        ↓
┌──────── seed 0 ────────┐
├──────── seed 1 ────────┤
├──────── seed 2 ────────┤  sequential; same pinned runner/toolchain
└──────── seed 3 ────────┘
        ↓ per seed
pinned official server world
        ↓
bounded 8-ticket generation batches
        ↓
exact 64 × 3 selected chunk columns
        ↓
`minecraft:full` saved-status census
        ↓
representative-member extraction
        ↓
independent Python corpus validation
        ↓
all-five Rust reconstruction
        ↓
individual decision gate MUST reject
        ↓
four-member set firewall
        ↓
`decision_eligible = true`
`decision_scope = dimension-separated-only`
`cross_dimension_score_allowed = false`
```

## Why members are generated sequentially

The workflow intentionally runs the four official server worlds sequentially on one hosted runner.

This is not a throughput benchmark, so parallel wall-clock speed has no evidentiary value. Sequential generation has useful engineering properties:

- bounds Java heap and disk pressure to one active world at a time;
- avoids accidental shared-resource contention between four official servers;
- keeps one toolchain/runner identity for the complete population materialization;
- makes failure localization and artifacts straightforward;
- avoids introducing artifact-transfer machinery merely to join a four-job matrix.

The world generator itself remains bounded internally: eight same-dimension force-load tickets are admitted at a time, separated by explicit command markers and synchronous `save-all flush` barriers.

If qualification runtime later becomes operationally unreasonable, orchestration may be parallelized without changing the representative sampling policy. Such a change must retain independent per-member evidence and the same set firewall.

## Frozen workload identity

The set workflow does not choose seeds or coordinates. Those are already frozen by representative-v1:

- policy: `vanilla-section-representative-v1`;
- plan SHA-256: `fecb9c9bc77aa9689ceaf6d88fa9af96019a48d9533269f3bd15824f7dfc7191`;
- four algorithmically derived seeds;
- 64 chunk columns in each standard dimension per seed;
- 768 selected chunk columns total;
- equal seed weighting;
- dimension-separated decision evidence;
- natural vertical-section weighting inside selected generated chunks.

The generator's stable 192-command member-selection identity is:

```text
cb97b7490c28e38293251561749a87dbda2d0f78d78c7cf98471e5eff825a354
```

Changing generation mechanics does not silently change workload identity. Changing seeds, coordinates, dimensions or weighting creates a new representative policy.

## Independent member gates

Each seed is admitted independently before it may reach the set validator.

Required artifacts per seed:

```text
world-evidence.json
extraction-evidence.json
corpus-manifest.json
rust-import.json
decision-rejection.log
server.log
member.corpus
```

The member must prove:

- exact official server identity;
- exact representative plan and seed identity;
- complete bounded generation command coverage;
- exactly 192 selected chunks saved at `minecraft:full`;
- exact selected coordinate schedule;
- contiguous per-dimension section lattices;
- source inventory and normalized corpus SHA-256 identities;
- Python target/cardinality/dimension summaries;
- Rust target/cardinality/dimension summaries;
- exact reconstruction by direct-reference, direct, adaptive, fast-local and packed-local;
- per-dimension candidate evidence recomposes the member evidence;
- `purpose = representative-member`;
- `decision_eligible = false` for the member itself.

A missing or malformed seed fails the population. There is no substitute seed and no content-based retry rule.

## Population gate

`tools/section_corpus_set.py` is the only mechanism that turns the four member records into decision-eligible workload evidence.

It requires all four frozen seed identities and distinct member corpus SHA-256 values, a common target/server/plan identity, common per-dimension section lattices, and agreement between Python and Rust evidence.

The resulting record deliberately carries two conceptual identities:

- `population_sha256` identifies **which workload population** was materialized. It binds the frozen policy/plan/target/server/weighting/lattice and member seed/corpus identities. Candidate implementation diagnostics and runner timing do not alter this identity.
- `evidence_sha256` identifies the **validated set evidence record** produced by the current qualification tooling. Candidate diagnostics therefore change this digest even when the workload population is unchanged.

This separation prevents changes in benchmark implementation or runner speed from masquerading as a new vanilla workload population.

## Dimension-weighting firewall

The set may not collapse dimensions into a production decision score.

The only decision-bearing aggregate is per dimension across equal-weight seeds:

```text
per_dimension.minecraft:overworld
per_dimension.minecraft:the_nether
per_dimension.minecraft:the_end
```

The whole-set `aggregate` object is descriptive only and may contain section/cell counts, but not candidate totals or a global cardinality histogram suitable for ranking candidates.

This rule exists because summing section populations directly would silently weight dimensions according to their vertical lattice size rather than observed server usage. For the admitted 26.2 lattice that accidental ratio would be 3:2:2.

See [`SECTION_DIMENSION_WEIGHTING_GUARD.md`](SECTION_DIMENSION_WEIGHTING_GUARD.md).

## Artifact manifest

The workflow writes `artifact-manifest.json` containing the relative path, byte size and SHA-256 of every evidence file present before the manifest itself is written.

The GitHub artifact is transport/retention, not the canonical workload identity. The canonical workload identity is `population_sha256`; the set record and per-member hashes are the machine-readable evidence graph.

The artifact includes the normalized member corpora so the exact population can later be consumed by target-hardware CPU/RSS qualification without regenerating worlds on the benchmark machine.

## Failure policy

The workflow fails closed on any member or set inconsistency.

A failure may motivate a generator/parser/tooling fix, but must not silently:

- remove a seed;
- replace a coordinate;
- omit a dimension;
- ignore a non-`full` chunk;
- fill an unknown section lattice from a guessed height;
- accept a member that failed Python or Rust reconstruction;
- make a single member decision-eligible;
- introduce a cross-dimension score.

If an orchestration mechanism is replaced, the failed mechanism and reason remain in the experiment record.

## Relationship to performance qualification

A green four-member set is necessary but not sufficient for #19.

It supplies real vanilla **population evidence**:

- natural section cardinalities;
- spatial state images;
- representation distributions;
- deterministic logical storage totals;
- per-dimension prevalence of representation regimes.

It does not supply qualifying CPU latency or process RSS because GitHub-hosted hardware is noisy and uncontrolled.

The subsequent target-hardware layer consumes the exact admitted member corpora and must record CPU/core/frequency/governor/toolchain/build provenance, repetitions/noise, per-dimension CPU/tail measurements and isolated process/RSS evidence. Only that combined evidence may enter the Pareto decision.

## First-member sanity signal

The first completed seed-0 real-member probe is useful as admission evidence, not as a population verdict. It produced the expected 26.2 lattices:

```text
Overworld  -4..19  = 24 sections/chunk
Nether      0..15  = 16 sections/chunk
End         0..15  = 16 sections/chunk
```

Therefore one member contains:

```text
64 × (24 + 16 + 16) = 3,584 sections
```

The complete four-member population is expected to contain 14,336 sections if all four seeds reproduce the same target lattice. The set validator derives and checks the actual result rather than accepting that count as a hardcoded substitute for evidence.

## Exit condition for this layer

This layer is complete when a workflow artifact exists for a single exact commit in which:

1. all four frozen members PASS independently;
2. every member is individually decision-ineligible;
3. all member corpus SHA-256 values are distinct;
4. cross-seed dimension lattices agree;
5. `section_corpus_set.py` emits `decision_eligible = true`;
6. `decision_scope = dimension-separated-only`;
7. `cross_dimension_score_allowed = false`;
8. population and evidence identities are archived;
9. the result is recorded in the section experiment log and #19.

Only then should the target-hardware Pareto/RSS slice consume representative-v1 as production-decision workload evidence.
