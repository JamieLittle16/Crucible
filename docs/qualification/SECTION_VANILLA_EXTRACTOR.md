# Vanilla Section Extractor Record

Status: **M0.3D extractor v1 under real-target qualification**  
Parent: #19  
Corpus contract: [`SECTION_VANILLA_CORPUS.md`](SECTION_VANILLA_CORPUS.md)

This document records what the first vanilla-save extractor does, what evidence it relies on, defects found by real-target qualification, and what it does **not** yet prove.

## Extractor identity

```text
vanilla-save-region-v1-stored-sections
```

The policy name is intentionally explicit. This extractor emits only block sections physically represented by section records in the selected region files.

It is useful for:
- validating Minecraft 26.2 save decoding;
- measuring real stored-section state/cardinality/spatial distributions;
- generating a first real corpus for mechanism exploration.

It is **not sufficient for final corpus weighting** because omitted all-air sections are not reconstructed. Final #19 selection must not treat this policy as the complete distribution of live `LevelChunkSection` instances.

## Independent evidence chain

```text
source+runtime-qualified 26.2 state dataset
                ↓
canonical state key -> vanilla ID map
                ↓
vanilla save NBT palette Name + Properties
                ↓
normalized semantic state IDs
                ↓
merged independent corpus validator
```

The extractor does not derive or guess vanilla numeric state ordering.

It loads the source+runtime-qualified state dataset and requires its canonical digest to equal the committed state-data manifest `input_digest`. The committed target requires `assignment_policy=vanilla-identity` and `mapping=identity`.

Every saved palette entry is normalized to the same key form used by the official runtime probe:

```text
minecraft:block[property_a=value,property_b=value]
```

Properties are sorted lexically. An unknown saved state causes extraction to fail.

## Save snapshot identity

A source inventory is built from:
- root `level.dat`;
- every selected canonical `r.<x>.<z>.mca` region file;
- every `.mcc` external chunk payload file in those selected region directories.

Each file is hashed independently with SHA-256. The source inventory digest is SHA-256 over canonical ordered records:

```text
<relative-path>\t<file-sha256>\n
```

The digest is written into the normalized corpus header. Optional local/artifact inventory JSON records individual paths/digests, extractor policy, corpus digest and section count.

## Target world-storage layout

Minecraft Java **26.1 changed the world storage layout**. All default dimensions moved under the `dimensions` subfolder. This remains the layout for Crucible's pinned 26.2 target.

Official change record:
- [Minecraft Java Edition 26.1 — World Storage](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-1)

The exact v1 paths are therefore:

| Dimension | Minecraft 26.2 save region path |
|---|---|
| `minecraft:overworld` | `dimensions/minecraft/overworld/region/` |
| `minecraft:the_nether` | `dimensions/minecraft/the_nether/region/` |
| `minecraft:the_end` | `dimensions/minecraft/the_end/region/` |

Legacy pre-26.1 paths (`region/`, `DIM-1/`, `DIM1/`) are deliberately **not** accepted as standard 26.2 dimension locations. Custom dimensions remain unsupported by v1 rather than receiving inferred resource locations.

### Defect found by the first official-world probe

The initial extractor implementation encoded the historical pre-26.1 paths. Synthetic fixtures inherited the same assumption, so they passed while the first real official 26.2 world probe failed immediately after clean server shutdown because the expected root `world/region` did not exist.

This is classified as a **qualification-system defect**, not a Minecraft/parser failure:
- official runtime state identity binding had already passed;
- official server startup and clean save had passed;
- failure occurred at the world-layout postcondition before NBT extraction;
- the official 26.1 change record identified the target storage change;
- extractor paths, world-probe postconditions and synthetic fixture layout were corrected together;
- a regression now rejects a legacy root `region/` as the target's standard dimension layout.

The lesson is durable: version-specific filesystem layout belongs in target qualification and must not be inherited from older Minecraft conventions merely because binary formats remain familiar.

## Region/NBT checks

The implementation is cold stdlib-only tooling and does not enter production server code.

It checks:
- region filename `r.<x>.<z>.mca`;
- 8 KiB header / 4 KiB sector alignment;
- valid nonzero location entries;
- no overlapping sector allocations between location-table entries;
- chunk allocation bounds and declared length;
- region-slot absolute coordinate versus NBT `xPos/zPos`;
- chunk `DataVersion == 4903`;
- root `level.dat` `Data.DataVersion == 4903`;
- duplicate section-Y rejection;
- strict NBT root/compound/list/array decoding;
- preservation of list/int-array/long-array type identity;
- compound-list element types for `sections` and block-state palette;
- actual `TAG_Long_Array` for block-state packed data;
- no duplicate compound keys;
- no trailing NBT bytes.

Compression support:
- 1 — gzip;
- 2 — zlib;
- 3 — uncompressed;
- external `.mcc` payloads through the region external flag;
- 4 — LZ4 is explicitly rejected by v1.

If real target evidence shows LZ4 is required for a selected corpus, support is added as a separately tested mechanism rather than approximated.

## Block-state container decoding

For each stored section with `block_states`:
- palette must be non-empty;
- a one-state palette may omit `data` or use an empty long array;
- larger palettes require an actual long array;
- bit width is `max(4, ceil(log2(palette_size)))`;
- entries are decoded in fixed groups per 64-bit word (`floor(64 / bits)` entries per word);
- packed values do not cross long boundaries in extractor v1;
- long-array length must exactly match the required 4096 values;
- every palette index must be in range;
- resulting cell order is preserved as the frozen y-z-x section order.

The packing rule has synthetic full-section coverage and remains part of the real-26.2 admission probe.

## Synthetic test evidence

The extractor suite covers:
- exact 26.2 namespaced standard-dimension paths;
- explicit rejection of the legacy root-region path as a standard 26.2 layout;
- property sorting/canonical state identity;
- 4096-entry packed decode;
- end-to-end namespaced `level.dat + .mca + zlib chunk + NBT + palette + corpus + independent validator`;
- wrong level/chunk data versions;
- unknown target states;
- region slot/chunk coordinate mismatch;
- overlapping region-sector allocation rejection;
- malformed packed-long count;
- out-of-range palette indices;
- list-vs-long-array substitution rejection;
- wrong palette/list element types;
- explicit LZ4 refusal;
- qualified-state input-digest mismatch.

Synthetic tests prove internal invariants. They do not substitute for the official-world probe.

## Real-target admission gate

The hosted `Section Corpus Probe` generates a deterministic world with the pinned official 26.2 server, freshly reconstructs the exact source+runtime-qualified state identity map, extracts the stored Overworld sections, then passes the normalized output through the independently merged corpus validator.

The admission evidence records:
- exact official server SHA-256;
- fixed world seed and generator policy;
- source inventory SHA-256;
- number of stored sections emitted;
- observed state/cardinality/fact statistics;
- normalized corpus SHA-256;
- exact frozen state-data input/generation digests;
- any unsupported compression/container/state encountered.

A mismatch is treated as an extractor/qualification defect and fixed before real corpus timing is trusted.

## Final absent-air-aware policy

A later corpus policy must account for semantically present empty sections rather than weighting only serialized section tags.

The target dimension vertical ranges used for reconstruction must themselves be source/runtime-qualified. They must not be copied from historical Minecraft values or hardcoded from memory.

Until that policy exists, timing using `vanilla-save-region-v1-stored-sections` is exploratory real-data evidence, not the final production-selection weighting.
