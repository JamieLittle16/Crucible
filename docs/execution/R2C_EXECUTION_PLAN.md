# R2C Execution Plan — Native World Projection

**Status:** executable milestone plan  
**Target:** Minecraft: Java Edition 26.2 / protocol 776  
**Architecture:** `../architecture/R2C_WORLD_PROJECTION_IMPLEMENTATION.md`  
**Qualification:** `../qualification/R2C_WORLD_PROJECTION_QUALIFICATION.md`

## Purpose

R2B has established the replay-free Configuration -> Play bootstrap and hands one continuing `R2bPlaySession` to the explicit `WorldProjection` boundary. R2C turns that seam into Helve-owned visible world state.

This plan divides R2C into reviewable slices. Each slice is small enough to qualify independently and has an explicit exit condition. Later slices may begin source/research work in parallel, but no implementation may consume semantics or freeze a mechanism before its required gate is satisfied.

The plan deliberately separates:

```text
semantic discovery/admission
        ↓
reference correctness path
        ↓
production mechanism search
        ↓
integration
        ↓
stock-client product checkpoint
```

That order prevents the fastest-to-write representation from becoming permanent architecture by accident.

## R2C-wide rules

Every slice must preserve:

- strict vanilla-observable behavior for the admitted profile;
- no captured world/chunk/light Play publication in the production R2C path;
- one existing bounded connection driver across R2B -> R2C;
- target-neutral world state;
- target-owned 26.2 wire semantics;
- no network-specific second section/chunk truth;
- no mandatory dynamic dispatch/hash registry in HOT paths;
- no avoidable per-operation allocation/copy/scanning;
- exact generation/revision freshness for deferred or cached work;
- future regionized ownership compatibility;
- permanent independent reference/equivalence paths;
- benchmark evidence before nontrivial performance complexity is frozen.

## Dependency graph

```text
R2C.0 frontier + contracts
   ├── R2C.1 source-admitted 26.2 world wire law
   ├── R2C.2 dimension/resident-world substrate
   └── section-policy qualification continues in parallel

R2C.2 + source format evidence
   └── R2C.3 pregenerated-world import

R2C.1 + R2C.3
   └── R2C.4 height/biome/light semantic state

R2C.1 + R2C.3 + R2C.4
   └── R2C.5 transparent reference projector

R2C.5 + representative corpus
   └── R2C.6 production projection tournament

R2C.5 + bounded reference publication
   └── R2C.7 R2B -> R2C integration

R2C.6 + R2C.7
   └── R2C.8 stock-client native-world gate
```

The section-production policy blocks the final production storage freeze, not early semantic/reference work.

---

## R2C.0 — freeze the R2C frontier

### Goal

Turn the broad milestone paragraph into finite source, implementation and qualification boundaries before packet/world code grows.

### Work

1. Freeze the selected first R2C profile:
   - Minecraft 26.2 / protocol 776;
   - pregenerated world only;
   - fixed initial spawn/observation profile;
   - exact dimension(s) required by the qualification world;
   - explicit treatment of block entities and any temporarily unsupported semantic feature.
2. Add the canonical R2C semantic/source-review plan under `vanilla/` without official source text.
3. Name the required semantic groups, but do not assert packet/method details until source review proves them.
4. Define exact source-review closure criteria and Atlas admission pipeline.
5. Freeze architecture interfaces between:
   - importer;
   - resident world;
   - derived state;
   - target projector;
   - server/client publication cursor.
6. Record explicit non-goals so R3 gameplay does not leak into R2C.

### Exit

- architecture/execution/qualification docs reviewed;
- selected profile is explicit and fail-closed;
- every unresolved semantic fact has an owner/gate;
- no implementation PR needs to infer semantics from R1X captures.

### Expected PR shape

Docs + source-review tooling skeleton only. No production packet output.

---

## R2C.1 — source-admit initial 26.2 world projection

### Goal

Close the finite exact source frontier required to make the selected world visible.

### Discovery families

The source review should discover and then freeze the selected route for:

- initial client world-observation entry/order;
- chunk coordinate/section span semantics;
- block-state section serialization;
- biome serialization;
- heightmap publication;
- light masks/data;
- selected block-entity publication;
- any center/batch/pacing/acknowledgement semantics actually required by 26.2;
- exact target packet identities.

Do not broaden into general gameplay/movement/worldgen serializers unless an exact selected-route delegate requires them.

### Evidence pipeline

Use the established R2B discipline:

```text
pinned official source/runtime
        ↓
bounded exact review dossiers
        ↓
human-reviewed closure/hazards
        ↓
source-free VAR/SEM materialization
        ↓
independent Atlas gate
        ↓
admitted R2C target contract
```

Captures may act as black-box confirmation only after the semantic route is source-backed.

### Implementation

Once admitted:

- add target-owned generated packet IDs/constants;
- add transparent checked codecs/packing helpers in `helve-target-26-2`;
- keep wire-specific code out of world crates;
- add golden/reference tests from source-free admitted fixtures.

### Exit

- independent source gate reports admitted;
- no unresolved material delegate for the selected profile;
- exact target packet/packing law has generated drift checks;
- target code can be implemented without reopening official source merely to recover a field/order rule.

---

## R2C.2 — dimension and resident-world substrate

### Goal

Build the minimal persistent world ownership shape that R2C needs without pre-implementing R3.

### Types

Introduce compact equivalents of:

```text
DimensionId
DimensionRuntimeProfile
DimensionInstance<StoragePolicy>
ResidentChunk / chunk handle identity
```

The exact names may vary if existing contracts yield a cleaner fit.

### Requirements

- resource strings resolved at boundary, not hot lookup keys;
- vertical lattice facts pre-resolved;
- sparse lifecycle directory explicitly COLD/BOUNDARY;
- direct/resolved chunk access path available to projection/import code;
- chunk position + generation + semantic revision remain authoritative;
- resident semantic data is independent of worker/thread placement;
- active tick/watch/scheduler structures are not permanently embedded merely because R3 will need them later;
- static composition over the existing `BlockSection` law.

### Performance review

Structural tests/benchmarks must verify:

- no ordinary resolved chunk/section read requires a global hash lookup;
- no per-read heap allocation;
- no dynamic property bag for dimension facts;
- compact object sizes and owned-byte accounting are explicit.

### Exit

A synthetic dimension can own/resolved-access resident chunks with exact identity/revision and no networking dependency.

---

## R2C.2S — finish section production-policy selection (parallel workstream)

### Goal

Resolve M0.3D before R2C freezes its production storage profile.

### Current state

The candidate set and policy validator are ready, but the candidate registry still records no production winner. The final selection requires controlled target-hardware qualification, Pareto analysis and explicit human-reviewed policy selection.

### Rule

R2C semantic/reference work may proceed against `BlockSection` and correctness-qualified fixtures. The R2C production composition must not hard-code one candidate by convenience.

### Exit

- canonical production-policy decision exists;
- selected mechanism is statically wired into the production profile;
- rejected candidates remain in experiment history;
- R2C benchmark results name the exact section policy they used.

---

## R2C.3 — pregenerated-world importer

### Goal

Load a pinned pregenerated world directly into Helve semantic state with bounded scratch and minimal intermediate representation.

### Work

1. Source-review the exact persisted chunk fields used by the selected 26.2 world/profile.
2. Introduce a cold import crate/module boundary separate from live world storage.
3. Validate region/chunk framing, coordinates, target data version and bounds.
4. Decode blocks/biomes and admitted side data directly toward semantic IDs/final storage.
5. Reuse parser/packing scratch across chunks.
6. Avoid materializing a generic tree/object graph where a streaming/typed decode is sufficient.
7. Fail closed on unsupported required state.
8. Produce deterministic semantic digests for imported chunks/worlds.

### Reference path

A slower transparent importer is acceptable as an independent oracle if the optimized importer streams directly into final storage.

### Performance tournament

At minimum compare:

- generic/reference typed tree -> semantic conversion;
- streaming typed decode -> final section construction;
- any reusable scratch/reservation policy that materially changes allocation/copy cost.

Measure:

- cold/warm load throughput;
- p50/p95/p99 per-chunk decode;
- allocations/chunk;
- bytes copied;
- peak scratch;
- RSS after representative world import;
- final semantic digest equivalence.

### Exit

The selected qualification world imports deterministically into Helve state with exact source/version identity and no Mojang-shaped live object graph.

---

## R2C.4 — protocol-relevant biome, height and light state

### Goal

Own every world-derived semantic input required by the admitted R2C target projector.

### Biomes

- compact semantic IDs;
- no repeated string/registry resolution in section scans;
- target-independent live representation;
- reference recomputation/roundtrip fixtures.

### Heightmaps

- source-backed semantic predicate/value law;
- simple direct reference representation;
- exact import/recompute cross-check;
- no premature packed/hot representation freeze;
- future mutation update hooks designed around generated old/new facts rather than broad dirtiness.

### Light

- source-backed light result/publication law;
- explicit zero/full/allocated state where valid;
- validated import from stored pregenerated light when admitted;
- independent reference validation/recompute path where practical;
- exact section masks/ranges;
- future incremental frontier boundary, not whole-chunk relight baked into API.

### Exit

A resident chunk exposes a target-neutral, independently checkable semantic view containing every required R2C projection input.

---

## R2C.5 — transparent reference projector

### Goal

Produce exact admitted 26.2 world output from transparent semantic state before optimizing the mechanism.

### Implementation

Create a reference projector under the target/qualification boundary that:

- consumes `PublishedChunk` or similarly transparent canonical semantic views;
- uses checked obvious packing/length arithmetic;
- performs no cache reuse;
- allocates explicitly/boundedly;
- is deterministic;
- fails closed on unsupported semantic state;
- exposes packet-body/frame hashes for fixtures.

### Tests

- synthetic all-air/uniform/low/high-cardinality sections;
- negative/minimum Y boundaries;
- palette-width boundaries identified by source admission;
- all standard admitted dimensions/profiles;
- light empty/full/mixed cases;
- heightmap boundaries;
- selected block-entity cases;
- malformed/overflow rejection;
- differential comparison against source-free vanilla/runtime fixtures.

### Exit

The reference projector is byte/semantic correct for the complete selected profile and is permanent enough to serve as the oracle for production mechanisms.

---

## R2C.6 — production projection mechanism tournament

### Goal

Select the cheapest clean projection architecture rather than shipping the full-copy reference by default.

### Mandatory candidates

At least:

1. full-copy `PublishedChunk` -> encode baseline;
2. owner-local direct target projection into bounded reusable/immutable storage;
3. revision-keyed encoded chunk reuse/cache;
4. one incremental/structural-sharing candidate if trace evidence indicates rebuild avoidance can matter.

Layer revisions, COW, per-section sharing, compressed artifact caching and special allocators are separate hypotheses. Do not bundle several unproven ideas into one opaque candidate.

### Workloads

- first cold projection of representative chunks;
- warm repeated projection of unchanged chunks;
- fan-out: 1/2/8/32/128 observers;
- clustered joins;
- mutation-heavy observed chunk;
- sparse one-section change;
- light-only / block-entity-only change where supported;
- slow-client backpressure;
- cache eviction/rebuild;
- stale background completion;
- dimension-separated representative corpus.

### Metrics

- owner-thread CPU/stall;
- background CPU;
- encode/compress CPU;
- p50/p95/p99/p99.9/max;
- allocations;
- semantic/encoded copied bytes;
- cache hit rate;
- retained bytes/RSS;
- cache invalidation/rebuild frequency;
- atomic/refcount operations if sharing uses them;
- complete join/world-ready latency under the real R2C composition.

### Selection law

- semantic equivalence is a prerequisite;
- hosted CI timing is diagnostic only;
- target-hardware balanced runs own close decisions;
- complexity requires a material whole-cost win beyond noise;
- a mechanism that moves cost into every mutation must include that cost in the decision;
- a memory-amplifying cache must earn its retained bytes with realistic hit/fan-out evidence;
- losers leave production linkage but remain documented.

### Exit

One explicit production mechanism/policy is frozen with a decision record, or the simple baseline remains because no candidate earns complexity.

---

## R2C.7 — bounded R2B -> R2C integration

### Goal

Use the existing live R2B connection/session as the only transport owner while publishing a bounded initial Helve world observation.

### Shape

```text
R2bEntryOutcome::WorldProjectionReady(session)
        ↓
R2cWorldOwner {
    session,
    dimension/world handle,
    initial observation cursor,
}
```

The exact owner name is not semantic API law.

### Requirements

- move, do not recreate, the R2B session/driver/read scratch;
- keep teleport and keep-alive control serviceable while world output is pending;
- process only a bounded amount of world publication per service opportunity;
- no `Vec<Packet>`/full-view packet graph per join;
- shared immutable world artifacts referenced independently of per-client cursor;
- backpressure leaves cursor state transactional;
- ordinary unclaimed gameplay/movement frames remain available for later owners and cannot head-of-line block liveness;
- no captured Play world traffic fallback.

### Stress tests

- tiny egress capacity across every publication boundary;
- fragmented writes/reads;
- keep-alive deadline during large initial projection;
- teleport acknowledgement interleaved with chunk publication;
- slow transport;
- stale artifact/cache rebuild while client is pending;
- disconnect midway through projection without leaking/pinning unbounded state.

### Exit

A scripted/loopback client reaches the admitted initial world-observation completion using the exact continuing R2B driver and zero captured world bodies.

---

## R2C.8 — stock-client native-world gate

### Goal

Demonstrate the actual milestone with an unmodified Minecraft 26.2 client.

### Required observation

```text
Handshake/Login
 -> source-admitted Configuration
 -> replay-free R2B bootstrap
 -> same-driver R2C world projection
 -> stock client renders the admitted pregenerated Helve world
```

### Success criteria

1. zero captured Play world/chunk/light bodies;
2. no protocol disconnect;
3. correct dimension/spawn/world appearance for the admitted fixture;
4. initial teleport transaction remains correct;
5. at least one keep-alive cycle completes during/after world publication;
6. server-side world/projection hashes match the qualified fixture;
7. no second driver/queue/read scratch appears at R2C handoff;
8. memory remains bounded through repeated connection attempts;
9. unchanged shared artifacts are reused according to the selected production policy;
10. failures are attributable and fail closed rather than falling back to replay.

### Exit

R2C is complete. Terrain is now Helve-owned end to end. R2D begins the broader persistent-visible-world red team: indefinite residence, deterministic reconnect, repeated joins and resource stability.

---

## Recommended PR sequence

Keep source/semantic and mechanism changes reviewable. A good initial sequence is:

| Slice | Suggested PR purpose |
| --- | --- |
| R2C.0 | freeze R2C profile/source-review/evidence boundaries |
| R2C.1a | source discovery + exact review frontier |
| R2C.1b | materialize/admit source-free R2C SEM/VAR gate |
| R2C.1c | target 26.2 reference wire facts/codecs |
| R2C.2 | dimension/resident-world substrate |
| R2C.3a | typed reference world import |
| R2C.3b | optimized direct-to-final import + qualification |
| R2C.4a | biome/height semantic substrate |
| R2C.4b | light semantic/import substrate |
| R2C.5 | complete reference world projector |
| R2C.6a | projection benchmark/evidence harness |
| R2C.6b+ | one candidate per PR where practical |
| R2C.6 final | production projection decision/cleanup |
| R2C.7 | same-driver bounded world publication integration |
| R2C.8 | stock-client native-world gate + milestone record |

Do not force exactly this PR count if a source frontier naturally closes in a different bounded slice. The rule is one independently reviewable claim per change, not bureaucracy.

## Parallelism opportunities

These can proceed concurrently without semantic conflict:

- R2C source review and section-policy target-hardware qualification;
- dimension substrate and source review;
- importer reference tooling and target wire source review;
- benchmark harness construction after interfaces freeze;
- representative fixture/corpus extraction and reference projector implementation.

Do **not** parallelize by implementing multiple incompatible production architectures before the semantic/reference boundary is stable.

## CI progression

### Early slices

Ordinary CI:

- format/check/Clippy/rustdoc;
- unit/property tests;
- source-free gate schema tests;
- reference codec goldens;
- malformed-input tests.

### Mid R2C

Add:

- import fixture/corpus smoke;
- world semantic digest comparison;
- reference projector differential smoke;
- tiny-capacity backpressure matrix;
- stale-revision async/cache tests.

### Late R2C

Add:

- production/reference equivalence over representative corpus;
- hosted benchmark smoke with structural validation only;
- R2C server integration gate;
- stock-client/manual/automated external probe where infrastructure permits.

Heavy controlled-hardware qualification artifacts remain separate from hosted CI timing thresholds.

## Milestone anti-goals

R2C must not be "finished" by:

- replaying R1X chunk/light frames;
- decoding a world into a second generic object graph and leaving it as live state;
- copying Mojang's runtime registry/container hierarchy;
- introducing a global main-thread world lock;
- making a networking palette the simulation representation;
- serializing every unchanged chunk separately per client;
- adding unbounded send work until the client happens to render;
- selecting a section representation without the existing M0.3D evidence gate;
- adding COW/refcount operations to every block mutation without whole-cost proof;
- hard-coding hosted-runner benchmark numbers into correctness CI;
- implementing R3 movement/scheduler complexity before it is needed.

## Definition of a successful R2C programme

R2C succeeds when the codebase has become **more** structurally final, not merely more featureful:

- the world exists independently of networking;
- networking sees target-owned immutable projections rather than world internals;
- source-backed semantics are explicit;
- reference and production paths are independently comparable;
- expensive repeated work is shared/eliminated where evidence supports it;
- owner-local HOT paths stay clean;
- resource bounds remain explicit;
- future regionization, movement, persistence and multiple protocol targets can extend the same state rather than replace it.
