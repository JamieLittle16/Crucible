# R2C Pregenerated World Import Qualification

Status: **R2C.3 block-state import qualified through genuine official 26.2 save differential; resident install and whole-import performance decision pending**  
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

`UncompressedChunkPayloadDecoder` remains the zero-copy reference mechanism for compression ID 3.
The qualified `DeflateChunkPayloadDecoder` candidate admits uncompressed, zlib and gzip through the
same static transaction boundary; LZ4 remains fail-closed. Its compressed path owns one fallibly
allocated reusable output buffer sized to the caller-selected decompressed bound and reuses one
allocation-free DEFLATE state between chunks. Once that buffer is initialized, a chunk decode performs
no codec-side allocation or runtime decoder construction.

Synthetic qualification covers external uncompressed records plus complete inline zlib and gzip
region -> decompression -> NBT -> semantic-section transactions. The same decoder instance is reused
across both compressed wrappers without changing semantic output. Existing static decoder tests prove
that mechanism selection does not alter the transaction shape.

## Compressed-codec admission and hermeticity

Gzip/zlib now have a qualified first production candidate without weakening Helve's hermetic build
boundary. The exact reviewed dependency surface is:

- `miniz_oxide 0.8.9`, `default-features = false`;
- `adler2 2.0.1` as its checksum dependency;
- repository-owned Cargo vendor sources selected through `.cargo/config.toml`;
- exact versions/checksums frozen by `Cargo.lock` and the dependency allowlist;
- no git dependency, SIMD checksum feature or codec-side allocator feature.

The production wrapper calls the safe allocation-free `DecompressorOxide` core directly instead of
miniz's vector or generic streaming convenience APIs. This avoids a per-loader streaming dictionary
and writes decompressed bytes directly into one Helve-owned reusable output slice. The current
single-pass candidate sizes that initialized slice to the caller-selected decompressed bound. An
adaptive smaller retained scratch policy is a future mechanism candidate and must win on whole-import
CPU + memory evidence before replacing this simpler baseline.

Zlib admission requires exact stream consumption and the wrapper's Adler-32 check. Helve owns gzip
framing and validates magic, DEFLATE method, reserved flags, bounded optional extra/name/comment
fields, optional header CRC16, exact raw-DEFLATE consumption, CRC32 and ISIZE. Concatenated members or
hidden trailing compressed bytes are not silently admitted. Output exhaustion fails as a hard
`max_decompressed_bytes` violation, providing a decompression-bomb boundary before NBT parsing.

Hermeticity is permanent qualification, not a one-shot setup fact. The dedicated import workflow
creates a fresh empty `CARGO_HOME` and performs `cargo check` plus `cargo test` with
`--offline --locked`; therefore a missing vendor source or accidental registry dependency fails before
normal cached qualification can mask it. The repository guard and dependency allowlist also remain
active. Existing Helve Cargo aliases are preserved alongside the vendor source replacement.

Codec qualification currently establishes:

- maximum compressed-size enforcement in Anvil/external framing;
- hard decompressed-size enforcement in the decoder;
- exact EOF/trailing-data policy;
- zlib Adler-32 and gzip header/CRC32/ISIZE policy;
- reusable output-state behaviour;
- malformed/truncated/checksum/unsupported-wrapper regressions;
- end-to-end zlib/gzip semantic import on deterministic 26.2 NBT fixtures.

LZ4 remains explicitly unadmitted. Gzip/zlib semantic correctness is additionally exercised by the
genuine official-save differential below. Whole-import target-hardware cost still remains before the
codec/scratch policy is considered production-selected for performance.

## Target block-state resolver

The exact Minecraft Java 26.2 persisted-state resolver is qualified. It is generated from the same
source/runtime-qualified state dataset that defines Helve's dense vanilla-identity `BlockStateId`
universe and therefore does not invent a second numbering scheme.

The generated cold index covers all 32,366 admitted states in 416,665 bytes. Lookup hashes the already
canonicalized saved name/properties without constructing a canonical key string, maps to one generated
candidate, and then performs exact structured name/property verification before returning the existing
`BlockStateId`. Hash equality is indexing only and never semantic authority on hostile input.

The committed table is reproducible from regenerated official 26.2 runtime identities bound to the
frozen source qualification. The dedicated lookup workflow regenerates and byte-verifies the exact
artifact before Rust qualification. Keep this cold index separate from HOT mutation/state-fact tables
so persisted-world compatibility adds no ordinary block-access tax.

## Differential qualification

The permanent differential boundary is documented in `R2C_IMPORT_DIFFERENTIAL.md`.

The synthetic gate compares independent Python and production Rust implementations over sector-aligned
fixtures covering positive/negative coordinates, gzip/zlib/uncompressed records, an external payload,
uniform sections and both four-bit and forced five-bit non-spanning packed palettes. It compares all
4096 cells in every block-bearing section.

The genuine gate then generates an actual overworld with the pinned official Minecraft Java 26.2
server and fixed seed `6842363988700132471`. The same raw `.mca` files are independently decoded by the
Python oracle and the production Rust importer. `Section Corpus Probe` run 373 qualified:

- `4` actual overworld region files;
- `12,696` block-bearing sections;
- `52,002,816` exact semantic cells;
- `81` distinct dense state IDs observed;
- semantic SHA-256 `2b012ebf055e7eecec15ee503aba6103a456bb78fe199a9fe57dac8bc4d7163f`;
- normalized corpus SHA-256 `934b4d641fe016dcd51bebd6cf0dc05566719f6d26275fc3eae3b9e1e213b883` for that generated-save evidence run.

The workflow validates every actual region, including regions with zero oracle block-bearing sections,
so Rust-only extra output cannot hide. It also proves the official runtime state universe still binds
to the committed source-qualified state data and that every compared section/cell belongs to the same
corpus identity.

This closes the block-state real-save differential for the admitted R2C.3 profile. Biome, height/light
and selected block-entity semantics remain later admission work and are not implied by this result.

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
| synthetic cross-language differential | independent agreement on selected real-format packing/compression shapes | naturally emitted writer coverage |
| genuine official-save cross-language differential | normalized block-state semantic agreement on actual 26.2 writer output | biome/light/height/block-entity parity or production throughput |
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
- differential corpus/generator identity;
- whole-import benchmark workload/harness;
- target hardware/toolchain for a numerical baseline.

## Explicit non-claims at the current R2C.3 boundary

A green current R2C.3 block-state boundary means Helve has:

- bounded Anvil/NBT framing;
- qualified gzip/zlib and zero-copy uncompressed decoding;
- exact generated 26.2 persisted-name/property -> `BlockStateId` resolution;
- direct schema-directed decode toward final/reference section construction;
- synthetic and genuine official-save Python/Rust semantic differential coverage through all imported
  block cells;
- no Mojang-shaped live object graph and no networking dependency in the importer.

It does **not** yet mean a complete arbitrary 26.2 world can be loaded or played.

Still pending:

- final selected/reference section construction wired atomically into `LiveChunkCore` / `DimensionInstance`;
- transactional resident install and load/unload/reload qualification;
- biome import;
- heightmap/light import or recomputation policy;
- block entities;
- whole-import performance baseline and decompression scratch-policy decision;
- arbitrary-world compatibility claim;
- chunk/light network projection.
