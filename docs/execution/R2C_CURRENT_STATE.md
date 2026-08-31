# R2C Current State — Native World Projection

Status: **live engineering ledger**  
Target: **Minecraft: Java Edition 26.2 / protocol 776**  
Canonical plan: `R2C_EXECUTION_PLAN.md`  
Architecture: `../architecture/R2C_WORLD_PROJECTION_IMPLEMENTATION.md`  
Milestone qualification: `../qualification/R2C_WORLD_PROJECTION_QUALIFICATION.md`

This file records the current implementation/evidence boundary so contributors do not infer milestone
status from whichever branch or benchmark they happen to encounter. The canonical architecture and
exit criteria remain in the documents above; this ledger answers **what is actually established now,
what is only a qualified candidate, and what remains blocked**.

## Current boundary

R2B is complete: the stock 26.2 client can reach replay-free Play and the same bounded connection
driver remains live through the explicit `WorldProjection` handoff. R2C now owns the transition from
that seam to Helve-owned visible terrain.

The active R2C engineering stack deliberately advances two mostly independent fronts:

```text
source-backed 26.2 world-wire discovery/admission
                    +
resident world ownership / access substrate
                    ↓
pregenerated-world semantic import
                    ↓
biome / height / light semantic ownership
                    ↓
reference projection
                    ↓
production projection + same-driver publication
```

Packet/wire facts are not guessed merely because world-runtime work can proceed independently.

## Slice ledger

| Slice | State | Evidence / boundary |
| --- | --- | --- |
| R2C.0 frontier/contracts | **ESTABLISHED** | R2C architecture, execution and qualification contracts exist; selected profile is pregenerated-world-first and worldgen is not a prerequisite. |
| R2C.1 source discovery | **TOOLING QUALIFIED; SEMANTIC ADMISSION PENDING** | Source-review/discovery tooling is green, but the exact 26.2 chunk/light/world publication law still requires the pinned local Mojang source/runtime review and independent admission. No production packet fact may be inferred from captures. |
| R2C.2 resident-world substrate | **IMPLEMENTATION CANDIDATE QUALIFIED; MERGE PENDING** | `DimensionInstance`, compact resident identity/generation handling, direct resolved access and stale-handle rejection are implemented on the R2C stack. The substrate is independent of networking and worker placement. |
| R2C.2 resident qualification | **HOSTED QUALIFICATION ACTIVE; TARGET BASELINE PENDING** | Dedicated lifecycle/HOT-path qualification, generated-state integration tests, full 1/9/25/81-chunk structural matrix, retained hosted diagnostics and fail-closed target-hardware evidence tooling exist. Hosted timings are not production decision evidence. |
| R2C.2S section production policy | **PENDING CONTROLLED SELECTION** | Existing section candidates remain correctness-qualified, but the production storage winner still requires controlled target-hardware/Pareto decision evidence. R2C must not select a representation by convenience. |
| R2C.3 pregenerated-world import | **NEXT IMPLEMENTATION FRONTIER** | May build on the resident-world contract, but exact persisted fields/data-version assumptions must be source-backed and import correctness must be independently checkable. |
| R2C.4 biome/height/light state | **PENDING PREREQUISITES** | Depends on admitted wire/persisted semantics and imported semantic state. |
| R2C.5 reference projector | **PENDING PREREQUISITES** | Must consume transparent Helve semantic state and become the permanent correctness oracle before projection optimization. |
| R2C.6 production projector | **NOT STARTED BY DESIGN** | Mechanism tournament is intentionally blocked until the reference path and representative corpus exist. |
| R2C.7 same-driver publication | **NOT STARTED BY DESIGN** | Must preserve the existing R2B driver/egress/backpressure ownership and publish bounded world work. |
| R2C.8 stock-client native world | **MILESTONE EXIT** | Complete only when an unmodified 26.2 client renders Helve-owned terrain with zero captured world/chunk/light replay. |

## Active pull-request stack

The current development stack is intentionally reviewable rather than one large R2C branch:

- **#200 — resident-world substrate**: minimal permanent dimension/resident chunk ownership and access
  contract. It has already reached a fully green repository workflow set on a clean squashed candidate
  identity, but remains unmerged until explicitly accepted.
- **#201 — R2C.1 source discovery/frontier tooling**: source-review machinery for the finite 26.2
  world-projection frontier. Tooling is green; this is not the same thing as admitting the missing
  Mojang semantic facts.
- **#202 — resident-world qualification**: stacked on #200. Owns correctness integration, lifecycle and
  HOT-path benchmark evidence, hosted diagnostic retention, target-run provenance and cross-process
  evidence analysis.

PR numbers are navigation aids, not evidence identities. Final milestone/selection records must cite
immutable merged commits and retained artifact/digest identities.

## R2C.2 resident-world evidence now required

The resident-world substrate may not be described as performance-qualified from one local or hosted
number. Its evidence ladder is defined in
`../qualification/R2C_RESIDENT_WORLD_QUALIFICATION.md`:

```text
hosted diagnostic
  -> explicit target-run artifact
  -> >=3 matching independent target processes
  -> mechanical cross-process report
  -> human baseline review / later decision
```

Permanent correctness facts include:

- resident discovery returns the exact live generation handle;
- unload/reload advances generation identity;
- stale generations fail closed;
- generated 26.2 block-state IDs survive real mutation/revision paths;
- signed/negative chunk coordinates are exercised;
- repeated directory routing and resolve-once HOT access are semantically identical;
- ordinary bounded HOT work can retain direct `LiveChunkCore` access rather than re-entering the sparse
  lifecycle directory per block read.

The last point is an architectural contract. Diagnostic timing may illustrate its cost, but semantic
equivalence plus the counted routing operations are the permanent CI evidence.

## Evidence classes: do not conflate them

| Evidence | What it may establish | What it may not establish |
| --- | --- | --- |
| unit/integration test | local semantic/structural invariant | vanilla parity or production performance |
| source/runtime admission | exact selected vanilla law | fastest Helve mechanism |
| hosted benchmark diagnostic | harness health, semantic equivalence, rough direction | production threshold or winner |
| qualified target-run artifact | controlled single-process measurement with provenance | cross-process stability or winner |
| cross-process report | consistent target-hardware distribution/direction | automatic performance admission |
| decision record | selected production mechanism/profile after review | future validity after a requalification trigger |

A lower class never substitutes for a higher one.

## Immediate next engineering moves

1. Keep #202 green, including the target-runner and evidence-combiner tests.
2. Freeze #202 to one clean squashed candidate commit and rerun the exact identity before merge.
3. Complete the pinned R2C.1 Mojang source review and materialize source-free admitted world-wire facts;
   do not block independent importer/runtime work on guesses.
4. Begin R2C.3 with a transparent typed/reference pregenerated-world import path before optimizing NBT
   decoding/storage construction.
5. Give the importer its own semantic digest, malformed-input/bounds tests, allocation/copy accounting
   and controlled performance protocol before any streaming optimization is selected.
6. Preserve the section-policy gate: importer/reference work may use the `BlockSection` contract, but
   the final R2C production composition must not silently freeze an unselected storage candidate.

## Things that are explicitly not yet proved

Current R2C resident-world evidence does **not** prove:

- exact 26.2 chunk/light packet bodies or ordering;
- NBT/Anvil import correctness for the selected R2C world;
- biome/heightmap/light semantic ownership;
- chunk serialization/publication throughput;
- collision or walkability;
- persistence/save/restart behaviour;
- final listener/executor/region ownership topology;
- R2C milestone completion.

Those claims receive evidence only at their own layer. This keeps progress real: a green lower-level
benchmark cannot accidentally turn into a claim that the server has already solved the next subsystem.
