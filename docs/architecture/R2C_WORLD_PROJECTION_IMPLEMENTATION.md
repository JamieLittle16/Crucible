# R2C World Projection Implementation Contract

**Status:** normative R2C implementation contract; unresolved performance mechanisms remain evidence-gated  
**Target:** Minecraft: Java Edition 26.2 / protocol 776  
**Parents:** `R2_R3_LIVE_ENGINE_ARCHITECTURE.md`, `R2_R3_PERFORMANCE_SEARCH_PLAN.md`, `R2_R3_PERFORMANCE_DECISION_REGISTER.md`, `WORLD_SECTION_IMPLEMENTATION_SLICE.md`, `OWNERSHIP_SIMULATION_CONTRACT.md`  
**Qualification companion:** `../qualification/R2C_WORLD_PROJECTION_QUALIFICATION.md`  
**Execution companion:** `../execution/R2C_EXECUTION_PLAN.md`

## 1. Purpose

R2B ends at an explicit `WorldProjectionReady(R2bPlaySession)` handoff. The exact bounded connection driver, retained read scratch, teleport transaction and keep-alive state remain live at that boundary. R2C owns the next transition:

```text
Helve-owned semantic world
        ↓
source-admitted Minecraft 26.2 world-observation law
        ↓
target-owned chunk / light projection
        ↓
bounded publication through the same connection driver
        ↓
stock client renders Helve-owned terrain
```

R2C is not a translation exercise from Mojang classes to Rust structs. Vanilla is the external semantic oracle. Helve is free to use a radically different engine if the client-observable result is equivalent.

The design objective is stronger than merely making terrain appear:

> **Reach native visible-world projection without introducing a temporary representation, copying architecture, cache policy or networking shortcut that R3 must later dismantle.**

R2C therefore freezes semantic boundaries and coherence laws, preserves simple reference mechanisms, and requires production mechanisms to earn permanent complexity through whole-cost evidence.

## 2. Scope

R2C owns:

- source admission for the selected 26.2 initial-world observation/projection boundary;
- compact dimension runtime facts and loaded dimension identity;
- import of a pinned pregenerated world into Helve semantic state;
- resident Helve chunk/section state;
- protocol-relevant biome, heightmap, light and selected block-entity state;
- immutable/revision-bound target projection;
- a bounded initial observation plan sufficient for a stock client to render Helve-owned terrain;
- integration with the existing `R2bPlaySession` without replacing its driver, buffers, teleport state or liveness state;
- reference and optimized projection mechanisms plus their differential/performance qualification.

R2C does **not** require:

- world generation;
- authoritative player movement or collision;
- moving-view incremental interest tracking;
- a final region scheduler;
- a final persistence format;
- generic plugin/datapack mutation support beyond the explicitly admitted composition/profile;
- a complete block-entity gameplay implementation;
- a general dynamic command/registry architecture.

Those boundaries belong to later R2D/R3 or explicit profile expansion. R2C must nevertheless leave them architecturally unobstructed.

## 3. Non-negotiable R2C laws

### 3.1 Semantic authority

1. No packet identity, field order, branch, mask, palette rule, light rule, chunk-observation rule or ordering rule is admitted from a capture position alone.
2. Source-backed VAR/SEM evidence defines the selected 26.2 behavior.
3. Black-box stock-server/client evidence may confirm exact output, but does not become semantic authority by itself.
4. Unsupported input fails closed. R2C must not silently fabricate, drop or normalize semantic state merely to make the client accept a packet.

### 3.2 World/target separation

World storage must never depend on:

- Minecraft packet IDs;
- frame layout;
- compression;
- connection state;
- the target's section palette encoding;
- a runtime packet registry.

The 26.2 target crate owns wire representation. World crates own semantic state and semantic freshness only.

### 3.3 HOT-path law

Ordinary owner-local world reads/mutations must not acquire R2C projection overhead merely because networking exists.

The default HOT expectation is:

- zero per-block heap allocation;
- zero world-global lock/atomic authority check;
- zero runtime trait-object dispatch requirement;
- zero registry/hash lookup for immutable target facts that can be generated densely;
- zero packet construction during ordinary mutation;
- no `Arc`/reference-count operation on every block access merely to support snapshots;
- no full-section/full-chunk scan on an ordinary local mutation;
- no repeated global chunk-directory lookup after a local handle/window is resolved;
- no broad invalidation when generated old/new state facts prove a derived layer cannot have changed.

### 3.4 One connection path

R2C inherits the exact `R2bPlaySession` and continues to use its sole bounded `ConnectionDriver`.

R2C must not introduce:

- a second egress queue;
- a second ingress buffer;
- replacement read scratch;
- a hidden unbounded chunk-send queue;
- a separate socket owner solely for world projection.

Publication progress is compact per-client state over shared/borrowed immutable projection artifacts.

### 3.5 Revision/freshness law

Any asynchronous or cached world projection must identify the semantic state that produced it.

The existing baseline is:

```text
ChunkIdentity = position + generation
ChunkStamp    = generation + semantic revision
```

A result prepared for one identity/stamp cannot be installed or published as current after that identity/stamp becomes stale.

Whole-chunk revision is the initial coherence baseline. Finer block/biome/light/height/block-entity revision layers remain a mechanism candidate under D10 and may be admitted only if their saved rebuild work materially exceeds their HOT bookkeeping/memory cost.

## 4. Layer topology

The intended R2C dependency direction is:

```text
cold world source bytes
        ↓
R2C world importer / format boundary
        ↓
DimensionInstance<StoragePolicy>
        ↓
resident semantic chunks
        ↓
reference or optimized semantic publication view
        ↓
helve-target-26-2 R2C projector
        ↓
immutable bounded target artifacts
        ↓
per-client R2C publication cursor
        ↓
existing R2bPlaySession / ConnectionDriver
```

Dependencies flow downward only. In particular:

- `helve-world-*` must not depend on `helve-target-26-2`;
- target projection may depend on narrow semantic world contracts but not concrete disk/NBT parser types;
- the server composition binds target + world + session but does not become the owner of encoding law;
- qualification may compose reference and optimized paths side by side without production code depending on qualification crates.

## 5. Dimension runtime substrate

R2C should introduce three distinct concepts.

### `DimensionId`

A compact runtime identity. Resource-location strings are load/configuration identities, not ordinary inner-loop keys.

The exact scalar width is a representation detail; the API should make accidental string/hash lookup in HOT code difficult.

### `DimensionRuntimeProfile`

One immutable resolved fact bundle for a loaded dimension. It should contain only facts needed broadly/hot enough to justify direct access, such as:

- minimum block Y;
- exclusive maximum block Y;
- minimum section Y;
- section count;
- skylight capability;
- coordinate-scale/runtime facts required by admitted semantics;
- compact protocol-facing dimension identities needed by target projection;
- precomputed vertical offsets/masks where they remove repeated derivation.

Do not turn this into a dynamic property bag. A fact that is not needed in ordinary runtime code should not be added for completeness.

### `DimensionInstance<StoragePolicy>`

Loaded state for one dimension. The R2C form should own or resolve:

- compact `DimensionId`;
- immutable runtime profile;
- a sparse chunk/cell lifecycle directory at the cold/discovery boundary;
- resident chunk storage whose chunk identity is independent of worker placement;
- enough ownership/generation structure to become regionized later without moving semantic world contents into a scheduler object.

R2C may initially execute through a simple owner, but its APIs must not encode `main thread == authority`.

## 6. Chunk storage and section policy

`LiveChunkCore` already establishes the correct core shape:

```text
ChunkPos
ChunkGeneration
ChunkRevision
VerticalSectionLattice
contiguous logical sections
compact section summary masks
```

R2C must consume that architecture rather than creating a network-specific chunk truth.

The M0.3 section programme currently has correctness-qualified `direct`, `adaptive`, `fast-local` and `packed-local` production candidates, but the candidate registry still records no frozen production winner. Therefore:

- reference R2C work may use a correctness-qualified concrete section mechanism for fixtures;
- reusable R2C world/import/projection contracts must remain compatible with the `BlockSection` semantic boundary and static composition;
- no `dyn BlockSection` is required in production HOT paths;
- the R2C production profile must not be frozen until the existing controlled target-hardware Pareto + explicit production-policy decision has selected the section mechanism;
- R2C must never invent a second packed/palette representation merely because it resembles the network format.

Once the section policy is selected, production composition should monomorphize/static-bind it at install/build time rather than performing a runtime service lookup per section access.

## 7. Pregenerated-world import

### 7.1 Boundary principle

Anvil/NBT is an interchange/persistence boundary, not the live engine representation.

The importer should be shaped as:

```text
bounded file/region read
    -> validated target/version schema
    -> streaming decode with reusable scratch
    -> semantic state IDs / biome IDs / derived-state inputs
    -> final Helve chunk construction
```

Avoid:

```text
bytes
 -> generic NBT object tree
 -> Mojang-shaped chunk object graph
 -> generic registry objects
 -> second intermediate world model
 -> final Helve chunk
```

unless profiling/evidence proves an intermediate representation is actually cheaper or required for correctness.

### 7.2 Version policy

The first R2C importer should support the exact admitted/pinned target world data it has been qualified against and fail closed on incompatible `DataVersion`/schema identity.

Helve does not need to reproduce Mojang's DataFixer object graph in R2C. Historical migration support is a separate boundary and can later be implemented as an offline/import transformation if desired.

### 7.3 Construction policy

Decode directly toward final semantic state where practical:

- convert target block-state identity to Helve's dense semantic `BlockStateId` once;
- construct final section representation through a statically selected builder/policy;
- preserve semantic biome identities without per-cell string lookup;
- import validated height/light state when its exact meaning is source-admitted and cheaper than recomputation;
- recompute only when recomputation is the admitted/correct path or materially simplifies validation without excessive cost;
- reuse bounded scratch across chunks/files;
- report allocation/copy counts in importer qualification.

### 7.4 Selected first-world profile

R2C may intentionally start with a constrained pregenerated qualification world, but the constraint must be explicit.

If a selected profile excludes unsupported block entities or another semantic feature, the importer must reject such a world/chunk instead of silently dropping that state. Later profile expansion adds the relevant semantic/wire admission.

## 8. Biome state

Biomes are semantic world state, not packet bytes.

R2C should use compact target-independent semantic biome identities with a dimension/runtime mapping resolved at load/composition time. Inner section scans must not resolve resource strings or registries repeatedly.

The target 26.2 projector owns the client wire palette/packing for biome data. A future target may project the same semantic biome state differently without changing world storage.

Biome storage and block storage need not use the same representation if their workload/cardinality differs; however, any additional live representation must have an independent correctness/performance case rather than being copied from Mojang's container architecture by habit.

## 9. Heightmap state

Source-admitted heightmap semantics are frozen; live representation is a mechanism choice.

The reference path should use a transparent direct representation plus independent recomputation oracle.

Production candidates from D18 include:

- direct compact integer per column;
- packed live storage;
- hot-direct/cold-packed split;
- occupancy-assisted downward search.

R2C initial import/projection must preserve exact client-visible values. R3 mutation support should be able to update them incrementally:

```text
raise -> O(1) where predicate becomes newly highest
lower -> bounded downward search, ideally skipping impossible section ranges via summaries
```

Do not force every R2C block mutation to maintain an elaborate heightmap index before evidence shows that cost is required.

## 10. Light state

Lighting is a first-class semantic/derived subsystem, not an opaque chunk-packet byte array.

R2C must source-admit the selected 26.2 light result and publication law before target encoding is frozen.

The live mechanism should preserve the search space from D19:

- zero/full section special states where semantically valid;
- nibble arrays only when information requires them;
- compact masks for populated/empty light sections;
- incremental local frontier for later mutations rather than whole-chunk relight as the default;
- asynchronous preparation only behind exact generation/revision/freshness installation checks.

For a pregenerated R2C world, validated stored light may be imported when it is source-compatible and cheaper than recomputation. The implementation must not assume stored bytes are trustworthy merely because they exist.

## 11. Block entities and other chunk-adjacent state

Chunk projection may require semantic state beyond blocks/biomes/light/heightmaps.

R2C should separate:

```text
resident chunk core
protocol-relevant semantic side data
active simulation sidecars
```

A block-entity map/list should not force every ordinary block read to traverse or carry a Java-style polymorphic object. Presence/type facts that are static per block state should come from generated dense data when useful.

The first admitted profile may exclude unsupported block-entity payloads, but the target projector must never silently omit a required one.

## 12. Source-admitted 26.2 projection boundary

Before production target code is written, R2C must close a finite source frontier for the selected initial-world path.

The review must establish, for the exact selected profile, at least:

- what clientbound world-observation publications are required before terrain is usable;
- ordering/conditionality relationships among those semantic publications;
- chunk coordinate and section-range rules;
- block-state section wire palette/packing law;
- biome wire law;
- heightmap publication law;
- light masks/section payload law;
- selected block-entity inclusion law;
- any view-center/batch/pacing/acknowledgement semantics required by 26.2 for the selected path;
- exact packet identities generated into target-owned facts.

This list is a discovery boundary, not a claim about Mojang's exact method/class topology. The source review determines the exact final surface.

The result should become source-free `VAR-NET-R2C-*` / `SEM-NET-R2C-*` evidence plus an independently evaluated admission gate before production projection depends on it.

## 13. Reference target projection

The first encoder must optimize for transparency and independent correctness, not speed.

A useful shape is:

```text
PublishedChunk / transparent semantic projection
        +
transparent biome/height/light semantic views
        ↓
ReferenceChunkProjection26_2
        ↓
canonical packet bodies
```

The reference encoder should:

- perform no caching;
- use obvious checked arithmetic;
- expose deterministic byte output;
- fail closed on unsupported semantic state;
- be easy to compare against source/runtime-derived fixtures;
- remain permanently available for differential tests after an optimized encoder exists.

The current `PublishedChunk` full semantic copy remains suitable as one reference input/oracle. It is specifically **not** the production performance default.

## 14. Production projection search

R2C must run a mechanism tournament before freezing its hot projection path.

### Candidate A — full semantic copy then encode

The existing reference baseline. Expected strength: simplicity. Expected weakness: copies/scans every semantic cell when publication is rebuilt.

### Candidate B — owner-local direct target projection

Encode directly from a stable owner-local semantic view into one bounded reusable/pool-backed immutable destination, avoiding the full `PublishedChunk` copy.

Measure owner stall carefully. Eliminating one copy is not a win if the owner thread now performs excessive serialization/compression work.

### Candidate C — dirty-section incremental shadow projection

Maintain/rebuild only affected projection pieces and assemble a chunk artifact from revision-bound components.

Must prove the saved publication work exceeds added mutation bookkeeping and retained memory.

### Candidate D — publication-boundary structural sharing/COW

Share immutable section/page material only at publication boundaries. Ordinary owner-local reads/writes must not pay an atomic/refcount branch solely for this mechanism.

### Candidate E — revision-keyed wire projection cache

For observed unchanged chunks, build the target body/frame once and share it across equivalent clients until the exact coherence key becomes stale.

This may be combined with B/C/D after evidence.

### Candidate F — layered projection cache

Cache independently useful block/biome/height/light/block-entity projection pieces only if D10 evidence shows layer invalidation materially avoids rebuild work.

Do not introduce per-layer revisions by aesthetic preference.

## 15. Projection artifact identity

Every immutable reusable artifact must be keyed by all semantic inputs capable of changing its bytes.

Conceptually:

```text
ProjectionKey {
    chunk identity,
    semantic stamp or admitted layer stamps,
    target protocol/composition identity,
    projection variant,
    compression policy identity if compressed bytes are cached,
}
```

A missing key component is a correctness bug, not a cache-efficiency issue.

Artifacts must not retain mutable world references.

## 16. Cache policy

Projection caches are bounded acceleration structures, never world truth.

Required properties:

- exact revision/coherence key;
- explicit byte accounting;
- bounded retained memory;
- deterministic stale rejection;
- no slow client may pin unbounded historical revisions;
- eviction cannot change semantics, only force reconstruction;
- cache miss path remains correct;
- cache lookup must not become a world-global contended lock on ordinary fan-out.

D9/D12/D13 remain separate questions:

- which layer to cache;
- how immutable bytes are shared across clients;
- whether compressed output is worth retaining/shared.

Do not collapse them into one unreviewed `Arc<Vec<u8>>` architecture.

## 17. Initial client observation plan

R2C needs a bounded initial observation sufficient to make the selected world visible. R3C owns the full moving-view incremental interest engine.

Therefore the R2C client state should be deliberately small:

```text
R2cWorldProjectionCursor {
    selected initial observation plan identity,
    next semantic publication/chunk index,
    per-item publication progress if required,
}
```

It should **not** allocate a `HashSet<ChunkPos>` or a giant `Vec<Packet>` per joining player merely to bootstrap terrain.

For the first fixed spawn/view profile, the initial chunk order/set may be precomputed or represented arithmetically once its source semantics are admitted. Later R3 interest tracking replaces/extends the plan without changing chunk storage or projection artifact identity.

## 18. Backpressure and service budgeting

World projection is potentially much larger than R2B bootstrap, so bounded work is mandatory.

One service opportunity must have explicit limits for relevant resources such as:

- semantic chunks/bodies attempted;
- bytes admitted to egress;
- encode/compress work scheduled or completed;
- cache misses/rebuilds started;
- background permits consumed.

Backpressure law:

```text
prepare/borrow candidate artifact
        ↓
try existing bounded egress admission
        ↓
commit per-client cursor only after admission succeeds
```

If egress rejects, cursor/semantic observation state must not advance as if the client received the artifact.

Large artifacts may require segmented/vectored publication support later, but this must preserve one logical bounded queue and exact ordering/accounting.

## 19. Background preparation

Projection, compression, import and lighting may eventually use background CPU. R2C must make this safe before making it parallel.

Every deferred operation carries:

- semantic identity/stamp;
- target/composition identity where relevant;
- an explicit freshness policy;
- bounded resource permit/accounting.

Completion is installed/reused only after revalidating freshness and authority/lifecycle identity.

Background work must not starve simulation/network deadlines. R2C qualification must include join/exploration bursts under constrained worker budgets before a background pool policy becomes permanent.

## 20. Compression boundary

Compression is target/network representation, not world storage.

The search order is:

1. eliminate repeated semantic projection;
2. eliminate repeated framing/copying;
3. measure whether compression is material;
4. if it is, compare compressor implementations/levels and shared compressed artifacts;
5. preserve the per-connection encryption boundary when online mode is later admitted.

Do not retain compressed chunks merely because compression is expensive; cache hit rate, retained memory and invalidation rate must justify it.

## 21. Allocation and copy budget

Every production R2C benchmark/evidence record must make these structural costs visible:

- allocations per imported chunk;
- allocations per first projection build;
- allocations per cache hit/send;
- semantic bytes copied;
- encoded bytes copied;
- framing/compression copies;
- retained bytes per resident chunk;
- retained bytes per cached projection;
- per-client world-projection state bytes;
- peak scratch bytes;
- owner-thread versus background-worker CPU.

A candidate cannot claim victory by moving work outside a benchmark timer or from CPU into unbounded retained memory.

## 22. Generated target facts

R2C should extend the existing generated-data approach instead of rebuilding runtime registries.

Potential generated 26.2 facts, where source review proves they are stable/useful, include:

- packet identities;
- block-state protocol identity;
- light emission/opacity classes;
- heightmap predicate classes;
- block-entity presence/type classes;
- biome protocol identity/facts;
- fixed packing constants/bounds.

Use the narrowest representation that safely covers the target domain. Do not generate giant state-pair matrices when old/new per-state facts plus compact comparisons suffice.

## 23. Unsafe/SIMD/low-level specialization

The repository currently forbids unsafe code workspace-wide. R2C does not relax that by default.

If profiling later shows palette bit packing, light copy, compression or another scalar loop is a material bottleneck, an optimized experiment may be proposed only under D27 and the performance qualification standard:

- independent reference encoder exists;
- fuzz/adversarial equivalence exists;
- target-hardware whole-cost win is material beyond noise;
- portability and fallback are explicit;
- complexity does not leak into semantic APIs.

The order is: remove work first, tune instructions last.

## 24. Failure semantics

R2C fails closed on at least:

- incompatible world data/schema/version;
- invalid coordinates/section lattice;
- unknown/unmapped block or biome identities;
- malformed palette/packed storage;
- invalid height/light payloads;
- unsupported required block-entity state in the selected profile;
- arithmetic/size overflow;
- projection body exceeding admitted bounds;
- stale async/cache result installation;
- projection key mismatch;
- backpressure admission failure without cursor rollback;
- target packet decode/encode invariant failure.

No failure should silently fall back to capture bytes.

## 25. Architecture review checklist for every R2C PR

A reviewer should be able to answer **yes** to all applicable questions:

1. Is every new semantic behavior source-backed or explicitly still reference-only?
2. Is world state still target/network independent?
3. Is target 26.2 wire law confined to target/projection code?
4. Does the change avoid per-block/per-section dynamic dispatch in HOT paths?
5. Does it avoid unnecessary allocation, copying, hashing and broad scanning?
6. Does a hot lookup resolve generality once outside the inner loop where possible?
7. Are immutable/static facts generated/dense rather than dynamically interpreted?
8. Is freshness/revision identity explicit for deferred/shared work?
9. Can stale work fail without changing gameplay?
10. Does one bounded connection driver remain the only egress queue?
11. Are memory retention and tail latency measured alongside throughput?
12. Is a simpler independent reference path retained for equivalence?
13. If complexity was added for speed, is there evidence that the whole-cost benefit is material?
14. Does the API remain compatible with future regionized ownership rather than assuming one global main thread?
15. Does the implementation avoid creating a second world truth for networking?

## 26. R2C exit contract

R2C is complete when all of the following are true:

- the selected 26.2 initial-world projection semantic/source gate is admitted;
- an admitted pregenerated world loads into Helve-owned semantic dimension/chunk/section state;
- protocol-required height/biome/light/selected side state is Helve-owned and validated;
- a permanent transparent reference projector is byte/semantic qualified;
- the production projection mechanism has passed the required mechanism tournament or is explicitly left as a measured simple winner;
- target projection uses exact revision/coherence identity and bounded artifacts;
- `WorldProjectionReady(R2bPlaySession)` passes into R2C without replacing the connection driver/read scratch/liveness/teleport state;
- an unmodified stock 26.2 client receives **zero captured world/chunk/light Play bodies** and renders the admitted Helve world;
- ordinary keep-alive/teleport control remains live during/after world publication;
- repeated unchanged projection does not redo expensive identical work when the selected cache/share mechanism says it should be reused;
- bounded-resource, stale-result, malformed-input and backpressure qualification is green;
- hosted CI carries correctness/smoke gates while controlled target-hardware evidence owns close performance decisions.

R2D then red-teams the complete persistent visible-world product boundary: indefinite residence, reconnect determinism, repeated joins, resource stability and broader product-facing qualification.
