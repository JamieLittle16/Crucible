# R2C Pregenerated World Import Qualification

Status: **R2C.3 bounded framing + semantic stored-block transaction qualified; compressed codec, full target resolver, real-save differential and resident install pending**  
Target: **Minecraft: Java Edition 26.2 / DataVersion 4903**  
Architecture: `../architecture/R2C_WORLD_PROJECTION_IMPLEMENTATION.md`  
Execution plan: `../execution/R2C_EXECUTION_PLAN.md`

## Purpose

R2C needs a real Helve-owned semantic world before chunk/light projection can be meaningful. The first
world source is a pinned pregenerated Minecraft 26.2 world; world generation is deliberately not a
prerequisite.

This qualification prevents the cold persistence path from becoming a second live world model or a
permissive generic deserializer. The selected architecture is:

```text
bounded region / separately bounded external bytes
        ↓
validated Anvil framing
        ↓
static ChunkPayloadDecoder boundary
        ↓
bounded schema-directed NBT cursor
        ↓
exact 26.2 block-state semantics
        ↓
static final-section builder
        ↓
(uncommitted imported chunk)
        ↓
future DimensionInstance residency transaction
```

The production path must not become:

```text
bytes -> generic NBT object tree -> Mojang-shaped object graph
      -> second intermediate world model -> live Helve chunk
```

Persisted palette packing, live section storage and network palette encoding are three separate
mechanisms. They must not be collapsed into one representation merely because Mojang uses related
concepts for each.

## Independent oracle

`tools/vanilla_section_extractor.py` remains the independent transparent cold qualification oracle for
pinned vanilla saves. It validates important 26.2 persisted semantics including:

- 4096-byte region sectors and the two-sector Anvil header;
- location-table bounds and overlap rejection;
- inline/external chunk framing;
- gzip, zlib and uncompressed oracle payloads;
- exact `DataVersion == 4903`;
- stored `xPos` / `zPos` agreement with the region slot;
- `sections` as a list of compounds;
- duplicate section-Y rejection;
- block-state palette identity and non-spanning packed-long decoding;
- normalized dense target state IDs.

The Python oracle deliberately uses a generic NBT value representation because transparency matters
more than allocation cost in qualification tooling. Production Rust does **not** share that parser
shape. The permanent differential gate compares normalized semantics rather than parser internals.

The Python extractor currently rejects LZ4-compressed chunks rather than guessing. Rust Anvil framing
recognizes compression ID 4 as metadata, but that recognition is not an admission of LZ4.

## R2C.3A — bounded Anvil and NBT framing

`RegionView`:

- receives already bounded bytes and re-enforces the configured region limit;
- rejects short or non-sector-aligned region images;
- validates all 1024 location entries before exposing chunks;
- rejects header-sector, zero-sector, out-of-file and overlapping allocations;
- uses checked signed region-to-absolute chunk coordinate arithmetic;
- validates stored record length against sector allocation;
- separates the external-payload flag from compression identity;
- never aliases external-record sector padding as payload;
- enforces an explicit inline compressed-payload limit;
- rejects unknown compression identities;
- performs location-table validation without heap allocation;
- contains no input-dependent `expect`/panic path in production framing.

External `.mcc` I/O remains outside `RegionView`. An external record exposes no borrowed bytes from its
allocated region sectors.

`NbtReader` is a zero-copy schema cursor over one already decompressed payload. It:

- rejects truncation and unknown tag IDs;
- validates UTF-8 names/strings;
- rejects negative list/array lengths;
- enforces explicit string/list/array bounds;
- enforces a hard recursion ceiling plus the selected import depth;
- rejects non-empty `TAG_End` lists;
- uses checked length arithmetic;
- exposes borrowed strings and byte arrays;
- bounded-skips irrelevant fields without constructing a generic object tree;
- detects trailing bytes after the admitted root is complete.

## R2C.3B — exact semantic block-state decode

`decode_chunk_block_sections` consumes decompressed NBT in one schema-directed pass. It validates:

- `DataVersion : TAG_Int == 4903`;
- `xPos` / `zPos` against the Anvil-derived expected position;
- `sections : TAG_List<TAG_Compound>`;
- required signed section `Y` and duplicate section-Y rejection;
- `block_states : TAG_Compound` when present;
- non-empty compound block-state palettes;
- a first writer-compatible palette bound of at most 4096 entries;
- palette entry `Name : TAG_String`;
- optional `Properties : TAG_Compound` with string values;
- duplicate property rejection;
- deterministic lexicographic property ordering before semantic resolution;
- fail-closed unknown state identities through the injected `BlockStateResolver`;
- minimum local palette width `max(4, ceil(log2(palette_len)))`;
- 26.2 non-spanning packed-long cell layout;
- exact packed-word count for 4096 cells;
- packed indices remaining inside the resolved palette.

The decoded 4096-cell order is already Helve's frozen observable YZX section order, so no transpose or
layout-conversion pass is required.

The decoder deliberately does **not** select live section storage. `ImportedBlockSectionBuilder` is a
static construction boundary:

- uniform sections can construct directly from one semantic state without allocating 4096 transient
  cells;
- non-uniform sections reuse one caller-owned 4096-cell semantic scratch buffer;
- palette and packed-word scratch capacities are also retained and reused between sections;
- the final/reference section object is constructed before return, not represented as a Mojang-shaped
  intermediate section.

Sections without `block_states` remain represented in the validated stored-section count but emit no
block-bearing section object.

## R2C.3C — complete stored-block transaction

`StoredBlockImporter` binds one cold loading session's:

- payload limits;
- NBT limits;
- statically selected `ChunkPayloadDecoder`;
- semantic `BlockStateResolver`;
- final-section builder;
- reusable section decode scratch.

Per-chunk calls then perform:

```text
RegionView slot
 -> choose inline bytes or separately bounded external .mcc bytes
 -> enforce external compressed-byte bound
 -> static decoder
 -> bounded NBT + exact target semantic decode
 -> final block-section objects
 -> return uninstalled ImportedStoredChunk
```

The returned source timestamp/compression/external flag are diagnostic metadata only; they do not
become live world authority.

The transaction remains atomic with respect to resident state: no `DimensionInstance` mutation occurs
inside this crate, so a late schema/state failure cannot leave a partially installed chunk.

The dependency-free `UncompressedChunkPayloadDecoder` is currently the only admitted Rust production
mechanism. It returns uncompressed payloads zero-copy and enforces the decompressed-byte limit.
Gzip, zlib and LZ4 fail closed under that decoder.

Synthetic qualification covers both inline and separately supplied external uncompressed records. A
static test decoder also proves that compression mechanism selection does not alter the semantic
transaction shape.

## Compressed-codec admission and hermeticity

Gzip/zlib support is still required for the selected pregenerated-world product path, but codec
admission has a stronger constraint than an ordinary crates.io review.

Helve's controlled section qualification creates a fresh empty `CARGO_HOME` and builds the selected
section benchmark with `cargo --offline --locked`. Consequently, adding a registry dependency anywhere
in the workspace can invalidate unrelated hermetic evidence even when that package is not on the
selected benchmark's semantic path.

Therefore a production codec must satisfy **both**:

1. source/license/advisory/version review; and
2. workspace hermeticity under the existing empty-`CARGO_HOME` offline qualification boundary.

Do not weaken or add network access to the section qualification merely to make a cold codec convenient.
A future codec mechanism must instead be made hermetically available (for example through a separately
reviewed vendored/path mechanism) or prove an equally strong architecture that preserves existing
evidence guarantees.

Codec qualification must additionally cover:

- maximum compressed and decompressed sizes;
- decompression-bomb behaviour;
- exact EOF/trailing-data policy;
- checksum/framing policy for admitted wrappers;
- reusable scratch high-water mark;
- malformed-input regression/fuzz corpus;
- whole-import cost rather than codec-only throughput.

LZ4 remains explicitly unadmitted.

## Target block-state resolver

The importer core deliberately does not bake the 32,366 target states into generic NBT logic.
`BlockStateResolver` receives a saved resource name plus already sorted property pairs and returns the
existing semantic state identity or fails closed.

The production 26.2 resolver must be generated from the same source/runtime-qualified state dataset
that already defines Helve's dense vanilla-identity `BlockStateId`s. It must not invent a second state
numbering or derive an unchecked mapping from network data.

The intended cold representation is a deterministic generated index that can locate a candidate
without allocating a canonical key string and then exactly verify name/properties before returning the
existing state ID. Hashing may narrow lookup, but hash equality alone is never semantic equality on
hostile input.

Keep this cold index separate from HOT mutation/state-fact tables so persisted-world compatibility does
not tax ordinary block access.

## Differential qualification

Synthetic unit tests are necessary but not sufficient. Once the full generated target resolver and an
admitted gzip/zlib mechanism exist, the permanent gate feeds the same pinned 26.2 region corpus to:

```text
Python qualification oracle ─┐
                             ├─> normalized semantic digest / selected chunk facts must match
Rust production importer ────┘
```

Compare semantic facts, not internal byte/object layouts:

- dimension and chunk position;
- stored section Y lattice/presence;
- all 4096 dense block-state IDs per imported block section;
- biome IDs once admitted;
- height/light semantic state once admitted;
- selected block-entity semantic payloads once admitted.

The corpus must include negative coordinates, varied palette cardinalities, empty/single-state sections,
multiple region files, inline/external payloads and minimized malformed fixtures for every fail-closed
rule.

## Hostile-input qualification

Current deterministic tests cover malformed region allocations, chunk lengths, NBT truncation/lengths,
depth limits, duplicate semantic fields, packed-word counts, out-of-range palette indices, unknown
states, external payload bounds and unadmitted compression.

Before arbitrary external worlds become a supported product feature, add a persistent fuzz/property
corpus for at least:

- region location/length framing;
- NBT skip/length/depth logic;
- decompression boundary;
- block-state palette/packed-data decoding;
- chunk schema state machine;
- generated target state resolver.

Every discovered panic, excessive-resource case or semantic disagreement becomes a minimized permanent
regression fixture.

## Performance qualification

There is deliberately no standalone framing/NBT/palette microbenchmark. The first decision-grade
benchmark begins when this complete transaction exists:

```text
bounded stored chunk
 -> admitted decompression
 -> schema + semantic block-state decode
 -> final selected/reference section construction
 -> LiveChunkCore construction
 -> DimensionInstance installation
```

Measure at minimum:

- chunks/second and whole-chunk service latency;
- p50/p95/p99 and slowest-tail behaviour;
- compressed bytes read and decompressed bytes produced;
- retained bytes per imported chunk;
- allocation count/bytes where measurable;
- bytes copied between stages;
- decoder/section scratch high-water marks;
- multi-region RSS/process effects;
- independent-process direction stability on controlled target hardware.

Hosted timings remain diagnostic. An optimization is accepted only with unchanged semantic digests and
an improved whole transaction without adding architecture tax to resident HOT access.

## Evidence classes

| Evidence | Establishes | Does not establish |
| --- | --- | --- |
| synthetic Rust unit tests | parser/transaction bounds and local schema invariants | real-save parity |
| Python oracle tests | independent persisted-format interpretation | production Rust correctness by itself |
| pinned cross-language corpus | normalized semantic agreement | production throughput |
| hosted whole-import benchmark | harness health / diagnostic direction | target-hardware winner |
| controlled target-hardware run set | reproducible load-cost distribution | automatic mechanism selection |
| decision record | selected production import/decompression mechanism | future validity after a trigger changes |

## Requalification triggers

Re-run affected evidence when any of these changes materially:

- Minecraft target/DataVersion or admitted persisted schema;
- region/NBT/transaction/decompression parser logic;
- source-backed state-ID mapping or generation digest;
- generated persisted-state resolver;
- selected section-construction policy;
- import limits or unsupported-input policy;
- codec source/version/build mechanism;
- differential corpus identity;
- whole-import benchmark workload/harness;
- target hardware/toolchain for a numerical baseline.

## Explicit non-claims at the current R2C.3 boundary

A green current R2C.3 means Helve has a bounded Rust Anvil/NBT path, exact semantic block-state decode,
and a complete **uncompressed** stored-block transaction with replaceable static decoder/resolver/builder
seams. It does **not** yet mean a complete arbitrary 26.2 world can be loaded or played.

Still pending:

- hermetically admitted production gzip/zlib decompression;
- generated full 26.2 persisted-name/property -> `BlockStateId` resolver;
- pinned real-region Python/Rust semantic differential;
- final selected/reference section construction wired into `LiveChunkCore`;
- transactional `DimensionInstance` resident install;
- biome import;
- heightmap/light import or recomputation policy;
- block entities;
- whole-import performance baseline;
- arbitrary-world compatibility claim;
- chunk/light network projection.
