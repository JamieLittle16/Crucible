# R2C Current State — Native World Projection

Status: **live engineering ledger**  
Target: **Minecraft: Java Edition 26.2 / protocol 776 / DataVersion 4903**  
Canonical plan: `R2C_EXECUTION_PLAN.md`  
Architecture: `../architecture/R2C_WORLD_PROJECTION_IMPLEMENTATION.md`  
Milestone qualification: `../qualification/R2C_WORLD_PROJECTION_QUALIFICATION.md`

This file records the implementation/evidence boundary actually present on `main`. The canonical architecture and exit criteria remain in the documents above; this ledger exists so contributors do not infer status from historical branches or benchmark experiments.

## Current boundary

R2B is complete: the stock 26.2 client reaches replay-free Play and the same bounded connection driver remains live through the explicit `WorldProjection` handoff.

R2C has now crossed the world-import boundary and established its final transport substrate. `main` owns resident chunk lifecycle, exact target block-state import, genuine-save differential evidence, import-to-residency qualification, selected cold-import optimizations, target-neutral biome semantic storage, atomic same-driver publication admission and allocation-free bounded publication progression. The remaining milestone work is concentrated in source-admitted biome/heightmap/light/world-wire law, composite semantic ownership, reference/production projection, and integration of admitted world output into that landed publication substrate.

```text
pinned 26.2 source review/admission
          +
resident world + exact stored-block import + biome substrate
          ↓
heightmap/light semantic ownership
          ↓
reference projection
          ↓
production projection mechanism
          ↓
landed same-driver bounded publication substrate
          ↓
stock-client native Helve world
```

Packet/wire facts are never guessed merely because the world-runtime or publication substrate exists.

## Slice ledger

| Slice | State | Evidence / boundary |
| --- | --- | --- |
| R2C.0 frontier/contracts | **ESTABLISHED** | Finite pregenerated-world-first profile, ownership boundaries and qualification contracts are frozen. World generation and movement-driven interest are not prerequisites for the first R2C gate. |
| R2C.1 source review/admission | **PIPELINE + ONE-COMMAND LOCAL BUNDLE IMPLEMENTED; ACTUAL ADMISSION PENDING** | Focused discovery/review packing, source-free finalization, human SEM authoring, manifest-bound Atlas gating, atomic promotion, committed-evidence verification and readiness diagnostics are CI-qualified. `r2c_world_state_source_review_bundle.py` now produces one bounded uploadable local review archive outside the repository. The pinned 26.2 biome/heightmap/light/world-wire review itself still must be completed; unadmitted packet IDs, packing, masks and ordering remain forbidden. |
| R2C.2 resident-world substrate | **LANDED** | `DimensionInstance`, compact chunk generations, direct resolved access, unload/reload and stale-handle rejection are on `main`. |
| R2C.2 resident qualification | **CORRECTNESS/HOSTED DIAGNOSTICS LANDED; TARGET PERFORMANCE ADMISSION SEPARATE** | Lifecycle/HOT-path qualification exists. Hosted numbers validate the harness and rough direction only; they are not target-hardware throughput claims. |
| R2C.2S block-section production policy | **DECISION PIPELINE LANDED; REAL EVIDENCE/POLICY NOT FROZEN** | Direct, adaptive-local, fast-local and packed candidates are correctness-qualified. M0.3D/issue #19 now has representative-population admission, same-commit correctness sealing, controlled-hardware orchestration, synthetic promotion-tail evidence, dimension-separated Pareto analysis and policy validation machinery. The remaining gate is the admitted four-seed population + exact-revision correctness bundle + controlled physical target-hardware runs + explicit human-reviewed winner/loser record. `SEC-REF-DIRECT` remains reference-only and must not silently become production storage. |
| R2C.3 pregenerated-world block import | **LANDED AND GENUINE-SAVE DIFFERENTIAL GREEN** | Bounded Anvil framing, DEFLATE, schema-directed NBT, exact 26.2 persisted-state resolution and final section construction are on `main`. Independent official-save comparison currently covers 12,696 block-bearing sections / 52,002,816 block cells exactly. |
| R2C.3 import → residency qualification | **LANDED** | Genuine official save is imported through resident admission with exact corpus identity, section/cell accounting and scratch high-water gates. A recent hosted run exercised 529 resident chunks; timing remains diagnostic rather than decision-grade. |
| R2C.3 selected cold-import mechanisms | **LANDED** | Byte-table gzip CRC32 and four-bit packed-state specialization are selected by semantic + performance evidence. Generic five-bit-and-wider packed decoding remains intact and regression-covered. |
| R2C.4 biome semantics | **TARGET-NEUTRAL SUBSTRATE LANDED** | One 4×4×4 biome lattice per logical chunk section with exact Y-Z-X semantics and signed section routing is on `main`; it deliberately assigns no target registry IDs or wire/persistence law. |
| R2C.4 heightmap/light semantics | **BLOCKED ON SOURCE ADMISSION** | Production implementation must wait for the finite pinned source review and source-free SEM/gate promotion. Unsupported required state must fail closed rather than being fabricated. |
| R2C.5 reference projector | **PENDING R2C.4 CLOSURE** | Must consume transparent Helve semantic state and become the permanent correctness oracle before projection optimization. |
| R2C.6 production projector | **PENDING REFERENCE PATH + MECHANISM EVIDENCE** | Production projection/cache/snapshot choices remain a mechanism tournament, not protocol semantics. |
| R2C.7 same-driver publication | **TRANSPORT/FAIRNESS SUBSTRATE LANDED; PROJECTOR INTEGRATION PENDING** | `R2bPlaySession` now admits already target-encoded Play bodies through the exact continuing `ConnectionDriver`: atomic batches use existing transactional `queue_batch`, while large ordered publications reuse the generic one-word `PublicationCursor` and admit at most one body per service opportunity. Backpressure leaves egress and publication progress unchanged. No second queue/socket or R2C-specific cursor was introduced. Packet identities/order remain source-admission responsibilities. |
| R2C.8 stock-client native world | **MILESTONE EXIT** | Complete only when an unmodified 26.2 client renders Helve-owned terrain with zero captured world/chunk/light replay. |

## Permanent evidence currently on `main`

### Stored-world semantic import

The production importer is compared against an independent normalized official-save oracle, not only against synthetic Rust fixtures. The current genuine-save gate covers:

- 12,696 stored block-bearing sections;
- 52,002,816 semantic block cells;
- exact target state-data identity;
- exact section coordinates and 4096-cell Y-Z-X ordering;
- malformed/bounds/unknown-state fail-closed behavior.

### Import through residency

The permanent qualification path exercises:

```text
preloaded official Anvil region bytes
 -> validated RegionView
 -> bounded decompression
 -> schema-directed semantic import
 -> final qualification section construction
 -> install_imported_chunk
 -> DimensionInstance residency
```

CI also requires importer/decompressor scratch to stop growing after warm-up and makes dense/reference materialization costs explicit rather than hiding them inside an aggregate timing number.

### Selected importer optimizations

Two cold-path mechanisms have crossed Helve's benchmark-first gate:

1. **256-entry byte-table CRC32** for mandatory gzip checksum verification. It preserves CRC/FHCRC/ISIZE/framing failure semantics and adds no allocation, dependency, unsafe code or runtime initialization.
2. **Four-bit cell-major packed-state specialization**. It branches once per section, keeps checked palette access and exact error indices, and retains the generic non-spanning path for five-bit and wider palettes with a focused 17-entry regression.

The detailed selection evidence and requalification triggers live under `docs/qualification/`.

### Biome semantic substrate

World code now has a target-neutral biome contract and direct reference implementation:

- 64 biome samples per logical section;
- exact local Y-Z-X index law;
- contiguous signed logical-section routing;
- static dispatch and no target registry/wire identity in world storage;
- range errors rather than implicit normalization.

This is semantic storage only. It is not evidence for the 26.2 persisted or clientbound biome codec.

### Same-driver publication substrate

World publication no longer needs a transport architecture decision. The server composition now exposes two source-independent paths over the exact R2B Play driver:

- an atomic batch path for already target-encoded body groups that genuinely require all-or-nothing egress admission;
- a fairness path backed by the existing one-machine-word `PublicationCursor`, admitting at most one immutable body per service opportunity.

Both inherit the driver's bounded frame/egress law. Capacity or wire rejection leaves existing logical egress unchanged; the cursor path also leaves client publication progress unchanged. This proves the transport/progression mechanism only. It does not admit any 26.2 world packet identity, payload layout or ordering law.

## R2C.1 source-admission boundary

The committed tooling can take the finite selected world-state review through:

```text
one-command bounded local source-review bundle
 -> reviewer-completed selected/rejected/hazard closure
 -> source-free finalized review result
 -> structured human-authored SEM worksheet
 -> source-free materialization bundle
 -> manifest-bound independent Vanilla Atlas gate
 -> atomic repository promotion
 -> source-free committed-bundle verification
```

The local bundle wrapper is `tools/r2c_world_state_source_review_bundle.py`. It composes the existing discovery and focused review-pack tools, requires its source-rich archive to live outside the repository, and does not infer or admit semantics.

The tooling is deliberately incapable of converting method names, call graphs or free-form observations into semantic rules. Actual reviewer-authored source closure remains required.

The pinned source identity for the current R2C frontier is:

- Minecraft 26.2;
- protocol 776;
- DataVersion 4903;
- source archive SHA-256 `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`.

Until promotion succeeds for the relevant groups, production code may not invent biome registry IDs, heightmap identities/packing, light masks/arrays, clientbound packet IDs, or publication ordering.

## R2C.2S section-policy gate

The section representation laboratory remains an independent blocker for the final production composition, but the benchmark/decision infrastructure itself is no longer the missing piece. The current registry explicitly has **no production winner frozen**.

M0.3D/issue #19 already has candidate-isolated population and synthetic-tail measurement, controlled repeated-run orchestration, exact-revision correctness sealing, dimension-separated Pareto analysis and a fail-closed human policy validator. The remaining decision evidence is:

1. one admitted complete `vanilla-section-representative-v1` four-seed population artifact;
2. a fresh sealed `crucible-section-full-bundle` from the exact source revision being measured;
3. at least five balanced rounds of `tools/section_m03d_qualification.py` on controlled physical target hardware;
4. inspection of the resulting per-dimension Pareto frontiers/noise state;
5. an explicit reviewed production-policy record and durable loser/survivor rationales.

The final R2C server must therefore not confuse the transparent direct/reference section used by qualification with the eventual production storage mechanism. This gate can advance in parallel with R2C.1/R2C.4 source work.

## Immediate next engineering moves

1. Complete and independently admit the finite pinned R2C source review required for biome/heightmap/light and selected world publication law. Do not infer missing facts from captures or implementation names.
2. In parallel, finish the M0.3D evidence run and human Pareto decision using the already-landed target-run/evidence tooling; do not build another benchmark stack.
3. Once the required source groups are promoted, implement heightmap/light semantic ownership as Helve-native world state with reference/differential tests first and compose biome/height/light under one authoritative resident freshness boundary.
4. Build R2C.5 as a transparent reference projector from semantic chunk state into admitted 26.2 projection law.
5. Only then qualify optimized projection/cache/snapshot candidates and select production mechanisms by whole-path cost.
6. Integrate the selected projector into the existing R2B `WorldProjection` handoff using the landed atomic-batch / one-body cursor publication paths; no second egress path or R2C-specific cursor is permitted.
7. Close R2C with an unmodified 26.2 stock-client test that renders the Helve-owned pregenerated world with zero captured world/chunk/light replay.

## Evidence classes: do not conflate them

| Evidence | What it may establish | What it may not establish |
| --- | --- | --- |
| unit/integration test | local semantic/structural invariant | vanilla parity or production performance |
| source/runtime admission | exact selected vanilla law | fastest Helve mechanism |
| hosted benchmark diagnostic | harness health, semantic equivalence, rough direction | target-hardware throughput guarantee |
| qualified target-run artifact | controlled single-process measurement with provenance | cross-process stability or automatic winner |
| cross-process report | consistent target-hardware distribution/direction | automatic performance admission |
| decision record | selected production mechanism/profile after review | future validity after a requalification trigger |

A lower evidence class never substitutes for a higher one.

## Explicitly not yet proved

Current `main` does **not** yet prove:

- complete admitted 26.2 chunk/light packet bodies or publication ordering;
- persisted/clientbound biome codec parity;
- selected heightmap identities/values/packing;
- sky/block light mask/array semantics;
- production block-section representation selection;
- reference or optimized native chunk projection;
- final world-publication sequencing or throughput on target hardware;
- movement/collision/walkability;
- persistence/save/restart behavior;
- R2C milestone completion.

Those claims become valid only at their own evidence gates.
