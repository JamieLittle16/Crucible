# R2C World Projection Qualification Programme

**Status:** normative correctness/performance qualification plan for R2C  
**Target:** Minecraft: Java Edition 26.2 / protocol 776  
**Architecture:** `../architecture/R2C_WORLD_PROJECTION_IMPLEMENTATION.md`  
**Execution:** `../execution/R2C_EXECUTION_PLAN.md`  
**Performance standard:** `PERFORMANCE_QUALIFICATION_STANDARD.md`

## 1. Purpose

R2C is the first milestone where large world state crosses from Helve's semantic engine into high-volume client projection. That makes it unusually easy to build something that is either:

- semantically plausible but subtly non-vanilla;
- fast in one microbenchmark while wasting memory/copy/owner time elsewhere;
- clean in isolation but architecturally toxic to R3;
- correct for one captured chunk while not implementing the real source law.

This qualification programme prevents those outcomes by keeping four evidence classes separate:

```text
A. source/semantic admission
B. reference correctness
C. production equivalence
D. performance/product qualification
```

A later class cannot compensate for failure in an earlier class.

## 2. Claim ladder

### Claim A — source law understood

Evidence proves the selected 26.2 world-observation path: required publications, ordering/branches and exact wire semantics.

### Claim B — Helve semantic world is correct

Imported/resident block, biome, height, light and admitted side state reproduce independently checkable semantic fixtures.

### Claim C — reference projection is correct

A transparent target projector maps the semantic world to the admitted 26.2 bytes without cache/performance complexity.

### Claim D — production projection is equivalent

The optimized mechanism reproduces the reference output/semantic trace for the same exact inputs and freshness states.

### Claim E — production mechanism is cheaper

Controlled evidence shows a material whole-cost benefit or demonstrates that the simple baseline is already the best justified choice.

### Claim F — actual stock client works

An unmodified 26.2 client renders the Helve-owned pregenerated world over the normal bounded connection path with zero captured world publication.

No claim implies the next one automatically.

## 3. Source-admission qualification

The R2C source gate should use the existing Atlas/VAR/SEM discipline.

### Required properties

- exact target/source archive identities are pinned;
- source-rich dossiers remain external/ephemeral where required;
- repository records contain source-free fingerprints/SEM/VAR only;
- every selected-route material delegate is reviewed;
- unresolved hazards fail the gate;
- generated packet identities/packing facts bind to admitted evidence;
- black-box captures do not infer branch/order law independently;
- source gate is evaluated by the independent ordinary admission tool rather than self-promoted by the collector/materializer.

### Drift tests

CI must reject:

- source fingerprint changes;
- target/protocol/data-version drift;
- method/type identity drift;
- missing SEM linkage;
- duplicate/unknown candidate families;
- incomplete reviewed-hazard state;
- a source-free record containing official source text.

## 4. World-import correctness

### 4.1 Fixture tiers

Use several levels of input rather than one magic world.

#### Synthetic typed chunks

Constructed directly as semantic fixtures to isolate:

- all-air/uniform sections;
- one-state and low-cardinality transitions;
- high-cardinality/noisy sections;
- negative/minimum section coordinates;
- top/bottom vertical boundaries;
- each admitted dimension profile;
- biome cardinality/packing boundaries;
- heightmap edge cases;
- light zero/full/mixed states;
- selected block-entity cases.

#### Minimal persisted fixtures

Small pinned world/region chunks exercising individual persisted format features.

#### Vanilla-derived representative corpus

A source/runtime-derived external corpus spanning the admitted standard dimensions and representative section/world populations. Content-addressed source-free metadata records exact corpus identity.

#### Product world fixture

The exact pregenerated world used by the stock-client R2C gate.

### 4.2 Import invariants

For every imported chunk verify:

- requested/embedded chunk coordinates agree;
- section Y/index arithmetic is exact;
- block state at every cell equals the oracle/reference decode;
- biome state equals oracle/reference decode;
- chunk generation/revision starts at the admitted initial identity;
- section summary masks equal independent recomputation;
- imported heightmaps equal independent source-backed recomputation/fixture where applicable;
- imported light state passes independent semantic validation;
- selected block-entity state is present and exact;
- unsupported required state rejects rather than disappears;
- import is deterministic across repeated runs;
- semantic chunk/world digest is representation-independent.

### 4.3 Adversarial persisted-input tests

Reject safely:

- truncated region/chunk/NBT input;
- length overflow;
- allocation bombs / declared sizes beyond admitted bounds;
- invalid packed-array length/bit width;
- duplicate/missing required fields;
- coordinate mismatch;
- unsupported data version;
- unknown state/biome identity;
- section outside dimension lattice;
- malformed height/light arrays/masks;
- invalid selected block-entity data;
- trailing data where the format forbids it.

The parser/importer must not panic or partially install a semantically invalid chunk.

## 5. Section-policy firewall

R2C correctness must run across all still-active correctness-qualified section candidates until M0.3D freezes the production default.

At minimum the same imported semantic corpus should reconstruct through:

- direct production candidate;
- adaptive candidate;
- fast-local candidate;
- packed-local candidate;
- permanent direct semantic oracle where applicable.

Every mechanism must produce the same representation-independent semantic digest and reference projection output.

After production selection, ordinary R2C CI may focus on the selected production mechanism plus permanent oracle/reference paths; candidate-history qualification remains available for reopen/forensics.

## 6. Heightmap qualification

### Semantic oracle

Maintain an independent direct/recomputed heightmap oracle from the admitted block-state predicate law.

### Cases

- empty column;
- highest valid block at each vertical boundary;
- raise above current maximum;
- remove current maximum with next match nearby;
- remove current maximum with large empty vertical gap;
- multiple predicate classes if the target uses multiple client-relevant maps;
- dimension minimum/maximum boundaries;
- repeated mutation/reversal.

### Production comparison

Any optimized live heightmap form must reproduce the oracle after every deterministic mutation barrier and across long random traces.

Performance tests must separate:

- read;
- raise;
- lower/search;
- construction/import;
- serialization/projection;
- resident bytes.

## 7. Lighting qualification

Lighting requires stronger evidence than “the client accepts it.”

### Reference checks

For the selected source-admitted light semantics, include:

- all-zero sections;
- semantically full/special sections where applicable;
- mixed nibble data;
- section-range boundary masks;
- skylight/no-skylight dimension behavior;
- emission/opacity transitions identified by generated facts;
- vertical boundary propagation;
- neighboring section/chunk boundary cases required by the selected R2C path.

### Import validation

If R2C imports stored light, independently validate its masks/ranges/content. Corrupt or semantically incompatible stored light must not silently become authoritative.

### Future mutation readiness

An incremental frontier candidate should be tested against a transparent recomputation oracle over deterministic mutation traces before R3 relies on it.

Performance evidence for a light mechanism must include local edits, bulk edits and memory cost; a whole-chunk recomputation path cannot hide rare catastrophic tails inside average throughput.

## 8. Reference target-projection qualification

The transparent reference projector is permanent correctness infrastructure.

### Golden/differential tests

For each fixture/corpus chunk:

1. build the canonical semantic reference view;
2. encode with the Helve reference 26.2 projector;
3. compare packet/body semantics and exact bytes against source-free admitted oracle fixtures where exact-byte comparison is valid;
4. decode/inspect independently where a second target-side parser can provide structural cross-checking;
5. record deterministic hashes.

### Wire boundary matrix

Exercise every source-admitted boundary, including:

- coordinate sign/boundaries;
- section counts/ranges;
- palette cardinality/bit-width transitions discovered by source review;
- direct/global versus local palette branches if admitted;
- packed-long boundary and padding behavior;
- biome palette branches;
- heightmap packed values;
- light masks and payload counts;
- empty/non-empty side data;
- maximum admitted packet/body sizes;
- malformed semantic input rejection.

Do not predeclare Mojang-specific branch thresholds before the source gate establishes them.

## 9. Production/reference equivalence

Every production projection candidate must run against exactly the same reference input set.

For each semantic chunk state compare:

```text
reference packet sequence / semantic groups
reference packet bodies
reference final stream digest

vs

candidate packet sequence / semantic groups
candidate packet bodies
candidate final stream digest
```

Where a candidate caches/compresses at a different layer, compare at the earliest common semantic/wire boundary and then independently validate the later framing/compression layer.

### Mutation/revision traces

Run long deterministic traces:

```text
mutate semantic chunk
maybe change derived layer
request projection
possibly delay candidate completion
mutate again
attempt install/send
```

Verify:

- same-state writes preserve valid artifacts when the semantic stamp does not change;
- real mutations stale whole-revision artifacts;
- old-generation artifacts never install into a new generation;
- wrong-position artifacts reject;
- layer-specific candidates invalidate exactly required layers;
- stale completion cannot overwrite/replace the current cache entry or be published as current;
- cache eviction/reconstruction does not change bytes.

## 10. Property/fuzz testing

R2C should use property/fuzz tests where the input domain is especially error-prone:

- VarInt/length/framing boundaries already covered by network cores;
- section palette encode/decode;
- packed integer/long arrays;
- heightmap packing;
- light mask/popcount/payload-count relationships;
- importer bounds/lengths;
- chunk/section coordinate arithmetic;
- projection freshness state machines;
- cache key equality/invalidation;
- tiny-capacity publication rollback.

Fuzz targets should assert no panic/UB, bounded memory policy where harnessable, and equivalence with a checked/reference implementation for valid inputs.

## 11. Backpressure qualification

R2C output can be large enough to expose hidden queue/transaction bugs that R2B does not.

For every meaningful egress capacity around body/frame boundaries:

- attempt one publication service opportunity;
- record pre-state cursor and egress;
- force rejection/partial transport write patterns;
- prove cursor only advances after logical admission;
- prove existing queued bytes remain intact;
- prove retry produces the exact same final byte stream;
- interleave keep-alive and teleport control events;
- prove unclaimed gameplay frames do not get consumed by R2C accidentally.

Include fragmented network writes and slow-writer schedules.

## 12. Resource-bound qualification

Track monotone growth across:

- repeated projections of unchanged chunks;
- repeated connect/disconnect before projection completes;
- cache churn across many chunk revisions;
- slow clients retaining old pending artifacts;
- repeated world import/unload where implemented;
- failed malformed imports/projections;
- stale background completions.

Evidence should record:

- connection-owned bytes;
- per-client R2C cursor bytes;
- cache bytes by layer;
- retained artifact count/revisions;
- resident world bytes;
- scratch high-water marks;
- allocation counts where measurable.

No test should depend on the allocator returning RSS to the OS immediately; distinguish logical ownership/high-water from process RSS.

## 13. Performance mechanism tournament

The performance standard applies in full. Hosted runners provide smoke/diagnostic evidence only.

### 13.1 Candidate identity

Every benchmark artifact names:

- exact commit;
- section production policy/candidate;
- target/protocol/data identity;
- corpus/world identity;
- projection candidate;
- cache/compression/share policy;
- compiler/profile/flags;
- CPU/topology/affinity/frequency provenance;
- warm-up and sampling regime.

### 13.2 Structural counters

Record deterministic or instrumented counts for:

- semantic cells read;
- semantic bytes copied;
- encoded bytes copied;
- allocations/frees;
- cache probes/hits/misses;
- complete rebuilds;
- layer rebuilds;
- compression invocations;
- shared artifact references/atomic ops if applicable;
- owner-thread service operations;
- stale completions discarded.

These counters help explain *why* a timing moved rather than replacing timing evidence.

### 13.3 Timing statistics

Where sample counts support them, report:

- p50;
- p90/p95;
- p99;
- p99.9;
- max as diagnostic;
- mean;
- MAD/IQR or equivalent robust spread;
- slowest 1%/0.1% means;
- balanced paired/epoch ratios for close candidate comparisons.

Do not average percentiles across independent processes.

### 13.4 Hardware counters

When available and reliable on target hardware, collect useful PMU evidence such as:

- cycles;
- instructions;
- branches/misses;
- cache/TLB misses;
- context switches;
- CPU migrations.

Use counters to answer a concrete hypothesis, not as decoration.

## 14. Projection benchmark workloads

### Cold first projection

A resident chunk has no cached target projection.

Measures construction latency, copy/allocation volume and owner/background split.

### Warm unchanged projection

Repeated observers request the exact same chunk revision.

Measures cache/share lookup cost and retained-memory trade-off.

### Fan-out matrix

Observer counts:

```text
1, 2, 8, 32, 128
```

Expand if real server populations justify it.

Measures whether shared immutable output beats per-client copy/reference-count overhead.

### Clustered join

Many clients require the same initial spawn view nearly together.

Measures deduplication, cache contention, queueing and world-ready tail.

### Independent exploration

Clients require mostly distinct chunks.

Prevents a sharing-heavy design from looking good only because every benchmark client stands at spawn.

### Mutation-heavy shared chunk

One observed chunk changes repeatedly.

Measures invalidation/rebuild cost and whether fine-grained revision/cache machinery is worthwhile.

### Sparse local mutation

One section/layer changes while most chunk state remains identical.

This is the key D10 workload for whole-chunk versus layer revisions.

### Slow clients

Clients consume output at different bounded rates.

Measures artifact lifetime, backpressure, old-revision retention and fast-client interference.

### Cache-pressure sweep

Working set crosses cache budget.

Measures hit ratio versus retained bytes and eviction/rebuild behavior.

### Compression threshold/implementation sweep

Only after semantic projection is optimized enough for compression to be a measured material cost.

## 15. Import benchmark workloads

Separate cold filesystem/page-cache and warm decode regimes.

Measure:

- small uniform chunks;
- representative Overworld/Nether/End corpus independently;
- high-entropy sections;
- light/height-heavy chunks;
- selected block-entity chunks;
- full qualification world startup;
- repeated import after warm-up.

Candidates compare the whole decode-to-final-semantic-state path, not only an isolated NBT parser function.

## 16. Whole-entry R2C benchmark

A production mechanism that wins isolated projection must also survive the actual connection composition.

The integrated timed boundary should include a clearly declared route such as:

```text
R2B WorldProjectionReady
        -> R2C initial observation start
        -> final required R2C artifact admitted/drained
        -> R2C initial world projection complete
```

Run at least:

- warm shared spawn view;
- cold cache;
- clustered joins;
- independent worlds/chunks if supported;
- slow client among fast clients;
- constrained background permits.

Record:

- admission-to-world-ready latency;
- queue/egress occupancy;
- completion spacing/jitter under offered joins;
- liveness deadline behavior;
- CPU/core-seconds;
- memory/cache residency.

An isolated encoder win that worsens world-ready p99 or tick/liveness interference is not automatically a production win.

## 17. Cache/share decision matrix

D9 and D12 should be decided with a matrix rather than one favored container.

For each artifact size/fan-out regime compare reasonable candidates such as:

- copy into each bounded egress;
- reference-counted immutable bytes;
- explicit slab/pool handle;
- adaptive small-copy / large-share threshold.

Measure:

- CPU;
- atomics/contention;
- copies;
- memory;
- lifetime under slow clients;
- queue integration complexity.

The chosen mechanism may legitimately vary by payload size/fan-out only if the branch/policy overhead is itself justified and remains statically/simplely resolved where possible.

## 18. Layer-revision decision gate

Whole-chunk `ChunkStamp` is the baseline.

A finer layer-stamp candidate must quantify:

### Cost added

- extra counters/fields per chunk;
- extra mutation branches/table checks;
- dirty-bit maintenance;
- cache key size/lookup cost;
- invalidation complexity.

### Work saved

- avoided block-section encoding;
- avoided biome encoding;
- avoided heightmap encoding;
- avoided light encoding;
- avoided block-entity encoding;
- cache retention/hit improvement.

Qualify over realistic mutation mixes. Do not select layer revisions from a synthetic trace where only one layer ever changes.

## 19. Compression decision gate

If compression becomes material, compare implementations/policies under:

- representative chunk sizes/content;
- cold/warm dictionary/cache state as relevant;
- fan-out;
- clustered join;
- mutation invalidation;
- CPU saturation;
- retained compressed bytes.

Record compression ratio alongside CPU/tail. A faster compressor that materially amplifies network bytes is a trade-off, not an unconditional win.

## 20. Low-level optimization admission

Unsafe/SIMD/custom allocation is not part of baseline R2C.

A low-level candidate is eligible only when:

1. profiling identifies the exact material hotspot;
2. transparent reference/equivalence exists;
3. structural work-elimination candidates have been exhausted or are orthogonal;
4. controlled hardware shows a material whole-cost gain beyond noise;
5. tail/memory regressions are acceptable;
6. adversarial/fuzz coverage is stronger than the scalar baseline;
7. target/architecture fallback is explicit.

Do not optimize bit-manipulation aesthetics before proving that bit packing dominates the end-to-end cost.

## 21. Stock-client qualification

The final R2C black-box gate uses an unmodified Minecraft Java 26.2 client.

### Required evidence

- exact server commit/config/world identity;
- terminal/server event trace showing R2B -> R2C handoff;
- `captured world publication = 0` evidence;
- initial projection semantic/artifact hashes;
- client reaches rendered Helve terrain without protocol disconnect;
- spawn/dimension/terrain matches the admitted world fixture;
- teleport acknowledgement remains correct;
- at least one keep-alive cycle completes while/after world output is active;
- no hidden replacement driver/queue/buffer;
- bounded cache/session/world-projection memory.

### Negative control

The gate should fail if the required Helve world projection is removed/corrupted; it must not pass because an old replay path remains accidentally available.

## 22. R2C CI structure

### Required on ordinary PRs once relevant

- Rust format/check/strict Clippy/tests/rustdoc;
- source-gate/schema/drift tests;
- import parser/property tests;
- world semantic fixture tests;
- reference projection goldens;
- production/reference differential smoke;
- tiny-capacity backpressure tests;
- stale revision/cache state-machine tests;
- benchmark binaries compile and smoke artifact schema validates.

### Heavy but not every hosted PR

- large vanilla-derived corpus reconstruction;
- long fuzzing;
- full representative differential projection;
- multi-process controlled-hardware timing;
- full import/RSS qualification;
- stock-client external probe where infrastructure cost makes it unsuitable for every commit.

Heavy evidence remains content-addressed and tied to the exact decision commit.

## 23. Performance acceptance law

A production R2C mechanism is selected only when:

1. source semantics are admitted;
2. reference equivalence is complete;
3. target-hardware evidence is eligible under the project performance standard;
4. the measured improvement is material relative to noise and frequency of the operation;
5. p99/p99.9/player-visible tails are not materially worse without explicit higher-level benefit;
6. memory/retention is acceptable;
7. construction/invalidation/eviction/slow-client costs are included;
8. architecture remains simpler or the added complexity is justified by the measured win;
9. complete R2C correctness/server gates remain green.

A candidate can be rejected for architecture even if a narrow microbenchmark is faster.

## 24. Reopen triggers

Requalify relevant R2C decisions when materially changing:

- Minecraft target/protocol semantics;
- section production policy;
- world/corpus distribution;
- dimension profile;
- projection wire format/admission;
- revision granularity;
- cache/share/compression policy;
- ownership/scheduler/background preparation topology;
- compiler/codegen/CPU deployment class;
- evidence that production traces differ materially from the qualifying workload.

## 25. R2C evidence ledger

Every accepted/rejected production experiment should record:

```text
candidate ID / hypothesis
semantic gate identity
reference-equivalence identity
commit + binary identity
world/corpus identity
machine/topology identity
warm-up/sampling protocol
structural counts
CPU/tail/memory results
known regressions
winner/rejection rationale
reopen trigger
```

Code can later disappear; the reason it won or lost must not.

## 26. Exit checklist

R2C qualification is complete only when:

- source law is admitted for the complete selected initial-world path;
- the qualification world imports into exact Helve semantic state;
- block/biome/height/light/selected side state have independent semantic checks;
- reference target projection is exact and permanent;
- production/reference equivalence covers representative and adversarial cases;
- projection freshness/cache/backpressure state machines are exhaustively tested at small bounds;
- selected production mechanisms have controlled whole-cost evidence;
- world projection cannot cause unbounded memory/queue growth;
- the real continuing R2B driver handles R2C output without replacement queues/buffers;
- the stock 26.2 client renders the admitted Helve world with zero captured world publication;
- liveness and teleport behavior remain correct;
- all claims are scoped to the exact admitted profile rather than generalized beyond evidence.

At that point R2D can focus on persistent-product qualification rather than repairing R2C architecture.
