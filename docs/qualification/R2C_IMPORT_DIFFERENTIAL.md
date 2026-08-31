# R2C Import Semantic Differential

Status: **synthetic and genuine Minecraft Java 26.2 block-section differentials qualified**

## Purpose

This gate compares Helve's production Rust persisted-world importer with an independently implemented
Python qualification oracle at the exact semantic section boundary.

It is intentionally not a second production importer. The Python side favors transparent parsing; the
Rust side uses the bounded schema cursor, generated target resolver, qualified gzip/zlib decoder and
reusable section scratch that the real cold import path uses.

The comparison surface is deliberately simple:

```text
SECTION|dimension|chunk_x|chunk_z|section_y|4096 dense BlockStateIds
```

Every block-bearing section therefore compares all 4096 cells in Helve's frozen YZX semantic order.
No NBT object representation, compressed bytes, palette representation or packed-long layout is shared
between implementations.

## Independence rules

The Python oracle and Rust importer must not share:

- Anvil record parsing;
- gzip/zlib decompression implementation;
- NBT parser implementation;
- palette decoding;
- packed-long cell extraction;
- section construction logic.

The Rust fact emitter is qualification-only glue. It invokes the actual production `RegionView`,
`DeflateChunkPayloadDecoder`, `Target262BlockStateResolver`, `StoredBlockImporter` and
`BlockSectionDecodeScratch`. Its `Vec<BlockStateId>` section representation exists solely to make all
4096 semantic cells visible to the comparison and is not a proposed resident section mechanism.

## Synthetic real-format differential

`tools/r2c_import_differential.py` deterministically writes sector-aligned Anvil region files and one
external `.mcc` payload, then feeds the same files to both implementations.

The corpus covers:

- positive and negative region/chunk coordinates;
- zlib, gzip and uncompressed Anvil records;
- an external compressed `.mcc` payload;
- homogeneous block sections;
- a stored section with no `block_states` payload;
- a two-entry four-bit packed palette with cell patterns crossing packed-word boundaries;
- a 17-entry five-bit non-spanning packed palette with deliberately repeated semantic identities to
  isolate packed-index/cell-order interpretation;
- multiple section Y values, including negative Y;
- section-list ordering normalized only at the final semantic fact boundary.

The fixture uses only `minecraft:air` and `minecraft:stone`. Their dense identities are part of the
separately source/runtime-qualified target-state universe; this gate tests persisted-format
interpretation rather than duplicating the resolver's official-source qualification.

Qualified synthetic evidence:

| Field | Value |
| --- | --- |
| block-bearing sections | `7` |
| exact semantic cells | `28,672` |
| semantic SHA-256 | `98cf921d050b0270c305138664d8fadd9fb85966f2e71a9eb7337cc9a4c24b12` |

## Genuine official 26.2 save differential

The stronger gate reuses Helve's existing official Section Corpus Probe rather than introducing a
second world-generation path.

`tools/official_section_world.py` starts the pinned official Minecraft Java 26.2 server under Java 25,
uses the fixed seed `6842363988700132471`, waits for startup completion, performs `save-all flush`, then
stops the server cleanly. `vanilla/vanilla.lock.toml` binds the official server artifact to SHA-256:

```text
cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5
```

The same generated overworld region files are then consumed independently:

```text
pinned official 26.2 server
        ↓
actual dimensions/minecraft/overworld/region/*.mca
        ├── Python vanilla_section_extractor.py ─┐
        │                                        ├─ exact normalized SECTION equality
        └── production Rust importer ────────────┘
```

`tools/r2c_genuine_save_differential.py` streams the Python corpus into per-region expected facts and
runs the production Rust fact emitter over every actual `.mca` in the selected dimension. Regions for
which the Python oracle emits zero block-bearing sections still receive an empty expected file, so a
Rust-only extra section cannot escape comparison. Negative chunk coordinates use floor-region
partitioning. A disagreement reports the exact region, section identity and semantic cell index.

The first fully qualified run (`Section Corpus Probe` run 373) established:

| Field | Qualified value |
| --- | --- |
| Minecraft | `26.2` |
| DataVersion | `4903` |
| official overworld region files compared | `4` |
| block-bearing sections | `12,696` |
| exact semantic cells | `52,002,816` |
| distinct dense states observed | `81` |
| production semantic SHA-256 | `2b012ebf055e7eecec15ee503aba6103a456bb78fe199a9fe57dac8bc4d7163f` |
| normalized corpus SHA-256 for that run | `934b4d641fe016dcd51bebd6cf0dc05566719f6d26275fc3eae3b9e1e213b883` |
| source inventory SHA-256 for that run | `1d2f0386eefa450ad4390917f6b5cddc9e484142bc5840710e25d85769fcb16a` |

The semantic SHA hashes only canonical `SECTION|...` records and is therefore the meaningful block-state
comparison identity. The complete corpus/source hashes also bind the exact generated-save evidence for
a run and may include source-inventory identity beyond the semantic cell stream.

The workflow additionally verifies that:

- the runtime state extraction contains exactly 32,366 official target states;
- runtime identities bind to the committed source-qualified state-data input digest;
- the generated world used the exact server SHA pinned by `vanilla.lock.toml`;
- the Python corpus manifest and source inventory agree;
- production Rust compared every corpus section and every corpus cell;
- the corpus reconstructs through every correctness-qualified Rust section candidate;
- the parser-admission corpus remains explicitly ineligible to select the production section policy;
- the complete evidence bundle is uploaded as a short-lived CI artifact.

## What this establishes

The two independent implementations now agree on both adversarially selected synthetic format shapes
and naturally emitted official 26.2 writer output for:

```text
Anvil framing
 -> wrapper selection / external payload routing where present
 -> decompression
 -> Java NBT structure
 -> target chunk coordinates/version
 -> stored section presence/Y
 -> block-state name/property resolution
 -> palette interpretation
 -> non-spanning packed-long decoding
 -> exact dense semantic cell order
```

This is strong evidence that the admitted R2C.3 block-state import path is semantically faithful to the
pinned target without copying Mojang's runtime object architecture.

It does **not** yet qualify biome, heightmap, light or block-entity import semantics; those remain owned
by later R2C slices. It also does not select a production section-storage mechanism: M0.3D still owns
that decision.

## Next admission step

The block-state importer can now move to the resident installation boundary. The next slice should
prove an atomic imported-chunk -> `DimensionInstance` transaction with:

- the dimension's full contiguous section lattice, including implicit empty sections;
- exact position and fresh generation identity;
- no partial residency on any installation failure;
- `load -> resolve/project -> unload -> reload` semantic recreation;
- stale-handle/deferred-work rejection after reload;
- memory release after unload rather than hidden world/network/watch retention;
- no repeated sparse-directory lookup once a resident handle has been resolved;
- no dependence on worker or active-region placement.

Production section storage remains statically mechanism-open until the separate section-policy
qualification freezes a winner.
