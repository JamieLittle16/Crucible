# R2C Pregenerated World Import Qualification

Status: **R2C.3 import boundary in implementation; framing/schema qualification active; semantic chunk import pending**  
Target: **Minecraft: Java Edition 26.2 / DataVersion 4903**  
Architecture: `../architecture/R2C_WORLD_PROJECTION_IMPLEMENTATION.md`  
Execution plan: `../execution/R2C_EXECUTION_PLAN.md`

## Purpose

R2C needs a real Helve-owned semantic world before chunk/light projection can be meaningful. The first
world source is a pinned pregenerated Minecraft 26.2 world; world generation is deliberately not a
prerequisite.

This qualification exists to prevent the cold persistence boundary from turning into a second live
world representation or a permissive generic deserializer.

The intended production flow is:

```text
bounded region / external-chunk bytes
        ↓
validated Anvil framing
        ↓
explicit decompression boundary
        ↓
bounded schema-directed NBT cursor
        ↓
exact 26.2 stored semantic identities
        ↓
final Helve section/chunk construction
        ↓
DimensionInstance residency
```

The following architecture is explicitly **not** the default:

```text
bytes -> generic NBT object tree -> Mojang-shaped object graph
      -> second intermediate world model -> live Helve chunk
```

Any intermediate representation added later must earn its allocations/copies on whole-import evidence.

## Independent oracle

`tools/vanilla_section_extractor.py` is the existing independent cold qualification oracle for pinned
vanilla saves. It already validates important parts of the 26.2 persisted boundary, including:

- 4096-byte region sectors and the two-sector Anvil header;
- location-table bounds and overlapping allocation rejection;
- inline/external chunk framing;
- gzip, zlib and uncompressed chunk payloads;
- exact `DataVersion == 4903`;
- stored `xPos` / `zPos` agreement with the region slot;
- `sections` as a list of compounds;
- duplicate section-Y rejection;
- block-state palette identity and non-spanning packed-long decoding;
- normalized dense target state IDs.

The oracle intentionally uses a generic Python NBT value representation because transparency matters
more than allocation cost in qualification tooling. **Production Rust must not copy that representation
merely to share implementation shape.** Independence is useful: the two paths should agree at a
normalized semantic boundary, not by sharing the same parser internals.

The Python extractor currently rejects LZ4-compressed region chunks rather than guessing. The Rust
Anvil framing layer may recognize compression ID 4 as framing metadata, but that recognition is **not**
an admission of an LZ4 decompressor or of LZ4 into the selected R2C import profile.

## R2C.3A — bounded framing and schema cursor

The first production slice introduces `helve-world-import` with no networking dependency and no live
storage-policy dependency.

### Anvil framing gates

`RegionView` must:

- receive an already bounded file image and enforce the configured region-byte limit again;
- reject files smaller than the mandatory header;
- reject non-sector-aligned files;
- validate all 1024 location entries before exposing a chunk;
- reject locations in header sectors, zero-sector allocations and out-of-file ranges;
- reject overlapping occupied allocations;
- use checked signed region -> absolute chunk coordinate arithmetic;
- validate the stored chunk record length against its allocation;
- separate the external-payload flag from compression identity;
- never treat padding in an external record's allocated sectors as the external payload;
- enforce an explicit inline compressed-payload limit;
- reject unknown compression identities.

The current location-table validator performs no heap allocation. This is a structural property, not a
performance claim.

External `.mcc` files are a separate bounded read. `RegionView` deliberately returns no borrowed
payload bytes for an external chunk record.

### NBT cursor gates

`NbtReader` is a zero-copy schema cursor over one already decompressed payload. It must:

- reject truncation and unknown tag IDs;
- require a compound root when chunk schema inspection begins;
- validate UTF-8 names/strings;
- reject negative list/array lengths;
- enforce explicit string/list/array bounds;
- enforce a hard recursion ceiling in addition to the selected import depth;
- reject non-empty `TAG_End` lists;
- use checked length arithmetic;
- expose borrowed strings/byte arrays rather than allocating copies;
- skip irrelevant fields by schema under the same bounds;
- detect trailing bytes after the admitted root is complete.

The production cursor deliberately does not build a generic compound map and therefore does not need to
allocate to remember every irrelevant field. A schema consumer must reject duplicates for every field
whose value it consumes. Unknown duplicate fields that are skipped have no semantic effect on that
consumer and are not promoted into live state.

### Exact chunk-header gates

The initial exact-target schema inspector consumes only the fields needed to establish chunk identity
and the next decoding boundary:

- `DataVersion : TAG_Int`;
- `xPos : TAG_Int`;
- `zPos : TAG_Int`;
- `sections : TAG_List<TAG_Compound>`.

It rejects:

- missing or duplicate required fields;
- wrong tag types;
- any `DataVersion` other than 4903;
- stored coordinates that disagree with the validated region slot;
- a `sections` list whose element type is not compound.

Section compounds are only bounded/skipped in R2C.3A. Their block/biome/light semantics are qualified in
later R2C.3/R2C.4 slices.

## Compression boundary

R2C.3A intentionally adds **no decompression dependency**. This keeps framing/schema correctness
independent from the first external Rust dependency decision and prevents a codec choice from being
smuggled into the architecture by convenience.

The decompression slice must separately establish:

- supported selected-profile compression identities;
- maximum compressed and decompressed sizes;
- decompression-bomb behaviour;
- exact EOF/trailing-data policy;
- scratch/buffer reuse policy;
- dependency license/advisory/source admission if a third-party crate is selected.

The CI roadmap requires automated dependency/advisory policy before a substantial third-party Rust
graph is admitted.

## Differential qualification

Synthetic unit tests are necessary but not sufficient. Once semantic section decoding exists, the
permanent differential gate should feed the same pinned 26.2 region corpus to:

```text
Python qualification oracle ─┐
                             ├─> normalized semantic digest / selected chunk facts must match
Rust production importer ────┘
```

Do not byte-compare internal object representations. Compare target-independent semantic facts such as:

- dimension and chunk position;
- section Y lattice/presence;
- all 4096 dense block-state IDs per imported block section;
- biome IDs once admitted;
- height/light semantic state once admitted;
- selected block-entity semantic payloads once admitted.

The corpus must include negative coordinates, varied section cardinalities, empty/single-state sections,
multiple region files and malformed fixtures for every fail-closed rule.

## Hostile-input qualification

R2C.3 is the activation point for NBT/import hostile-input testing. The first slice begins with
deterministic malformed-input tests for lengths, depth, truncation, region allocation and framing.

Before the importer is allowed to consume arbitrary external worlds as a supported product feature,
add a persistent fuzz/property corpus for at least:

- region location/length framing;
- NBT skip/length/depth logic;
- decompression boundary;
- block-state palette/packed-data decoding;
- chunk schema state machine.

Every discovered panic, excessive-resource case or semantic disagreement becomes a minimized permanent
regression fixture.

## Performance qualification

There is deliberately **no standalone framing/NBT microbenchmark in R2C.3A**. A fast cursor is not useful
if decompression, temporary allocation, section construction or sparse-directory installation dominates
the real load path.

The first performance benchmark starts when this complete transaction exists:

```text
bounded stored chunk
 -> decompression
 -> schema decode
 -> semantic block-state decode
 -> final selected/reference section construction
 -> LiveChunkCore construction
 -> DimensionInstance installation
```

Measure at minimum:

- chunks/second and whole-chunk service latency;
- p50/p95/p99 and slowest-tail behaviour;
- compressed bytes read and decompressed bytes produced;
- owned bytes retained per imported chunk;
- allocation count/bytes where the harness can measure them reliably;
- bytes copied between import stages;
- reusable scratch high-water mark;
- process/RSS effects for representative multi-region loads;
- independent-process direction stability on controlled target hardware.

Hosted timings remain diagnostic. A production optimization is accepted only when semantic digests are
unchanged and the whole transaction improves without introducing architecture tax into resident HOT
world access.

## Storage-policy boundary

The importer must not freeze the unresolved R2C section production winner. Reference/differential work
may construct `DirectBlockSection`; reusable decode should target a statically selected builder/semantic
boundary so the final production section policy can be chosen by the existing controlled Pareto gate.

Likewise, persisted palette packing is not the live section representation and is not the network
palette representation. It is decoded once at the cold boundary into semantic state IDs.

## Evidence classes

| Evidence | Establishes | Does not establish |
| --- | --- | --- |
| synthetic Rust unit tests | parser bounds and local schema invariants | real-save parity |
| Python oracle tests | independent persisted-format interpretation | production Rust correctness by itself |
| pinned cross-language corpus | normalized semantic agreement | production throughput |
| hosted whole-import benchmark | harness health / diagnostic direction | target-hardware winner |
| controlled target-hardware run set | reproducible load-cost distribution | automatic mechanism selection |
| decision record | selected production import/decompression mechanism | future validity after a trigger changes |

## Requalification triggers

Re-run the affected import evidence when any of these changes materially:

- Minecraft target/DataVersion or selected persisted schema;
- region/NBT/decompression parser logic;
- source-backed state-ID mapping/generation digest;
- selected section-construction policy;
- import limits or unsupported-input policy;
- decompression dependency/version/build flags;
- differential corpus identity;
- whole-import benchmark workload/harness;
- target hardware/toolchain for a numerical baseline.

## Explicit non-claims after R2C.3A

A green R2C.3A does **not** mean that Helve can yet load a complete playable 26.2 world. It proves only
the bounded persisted framing/schema foundation.

Still pending after this slice:

- production decompression;
- semantic block-state palette/data decoding in Rust;
- section construction and resident install from a real region;
- biome import;
- heightmap/light import or recomputation policy;
- block entities;
- full Python/Rust corpus differential;
- whole-import performance baseline;
- arbitrary-world compatibility;
- chunk/light network projection.
