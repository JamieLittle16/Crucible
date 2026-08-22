# Vanilla Section Extractor Record

Status: **M0.3D extractor v1 under qualification**  
Parent: #19  
Corpus contract: [`SECTION_VANILLA_CORPUS.md`](SECTION_VANILLA_CORPUS.md)

This document records what the first vanilla-save extractor does, what evidence it relies on, and what it does **not** yet prove.

## Extractor identity

```text
vanilla-save-region-v1-stored-sections
```

The policy name is intentionally explicit. This extractor emits only block sections physically represented by section records in the selected region files.

It is therefore useful for:
- validating 26.2 save decoding;
- measuring real stored-section state/cardinality/spatial distributions;
- generating a first real corpus for mechanism exploration.

It is **not yet sufficient for final corpus weighting** because omitted all-air sections are not reconstructed. Final #19 selection must not treat this policy as the complete distribution of live `LevelChunkSection` instances.

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

It loads the local source+runtime-qualified state dataset and requires its canonical digest to equal the committed state-data manifest `input_digest`. The committed target currently requires `assignment_policy=vanilla-identity` and `mapping=identity`.

Every saved palette entry is normalized to the same key form used by the official runtime probe:

```text
minecraft:block[property_a=value,property_b=value]
```

Properties are sorted lexically. An unknown saved state causes extraction to fail.

## Save snapshot identity

A source inventory is built from:
- `level.dat`;
- every selected `.mca` region file;
- every `.mcc` external chunk payload file in those selected region directories.

Each file is hashed independently with SHA-256. The source inventory digest is SHA-256 over canonical ordered records:

```text
<relative-path>\t<file-sha256>\n
```

This source-inventory digest is written into the normalized corpus header.

The optional local inventory JSON also records the individual paths/digests, extractor policy, output corpus digest and section count.

## Supported dimensions in v1

The extractor deliberately supports only the three standard vanilla dimension locations:

| Dimension | Save region path |
|---|---|
| `minecraft:overworld` | `region/` |
| `minecraft:the_nether` | `DIM-1/region/` |
| `minecraft:the_end` | `DIM1/region/` |

Custom dimensions are rejected rather than assigned an inferred resource location.

## Region/NBT checks

The implementation is a cold stdlib-only parser. It does not enter production server code.

It checks:
- region filename `r.<x>.<z>.mca`;
- 8 KiB header / 4 KiB sector alignment;
- nonzero valid location entries;
- chunk allocation bounds and declared length;
- region-slot absolute coordinate versus NBT `xPos/zPos`;
- chunk `DataVersion == 4903`;
- `level.dat` `Data.DataVersion == 4903`;
- duplicate section-Y rejection;
- strict NBT root/compound/list/array decoding;
- no duplicate compound keys;
- no trailing NBT bytes.

Compression support:
- 1 — gzip;
- 2 — zlib;
- 3 — uncompressed;
- external `.mcc` payloads through the region external flag;
- 4 — LZ4 is explicitly rejected by v1.

If real target evidence shows LZ4 is required, support will be added as a separately tested mechanism rather than approximated.

## Block-state container decoding

For each stored section with `block_states`:
- palette must be non-empty;
- a one-state palette may omit `data` or use an empty long array;
- larger palettes require a long array;
- bit width is `max(4, ceil(log2(palette_size)))`;
- entries are decoded in fixed groups per 64-bit word (`floor(64 / bits)` entries per word);
- packed values do not cross long boundaries in extractor v1;
- long-array length must exactly match the required 4096 values;
- every palette index must be in range;
- resulting cell order is preserved as the frozen y-z-x section order.

This packing rule is covered by synthetic cross-word/full-section tests but remains subject to the real-26.2/source qualification gate below.

## Current test evidence

The synthetic extractor suite covers:
- property sorting/canonical state identity;
- 4096-entry packed decode;
- end-to-end `level.dat + .mca + zlib chunk + NBT + palette + corpus + independent validator`;
- wrong level/chunk data versions;
- unknown target states;
- region slot/chunk coordinate mismatch;
- malformed packed-long count;
- out-of-range palette indices;
- explicit LZ4 refusal;
- qualified-state input-digest mismatch.

These tests prove internal parser invariants. They do not substitute for a real target-world run.

## Real-target admission gate

Before this extractor can produce qualifying corpus evidence, we require at least one real Minecraft 26.2 world produced by the pinned official server/client and the local source/runtime-qualified state dataset.

The first real run must record:
- world/source inventory SHA-256;
- dimensions selected;
- number of region files and stored chunks read;
- number of stored block sections emitted;
- corpus SHA-256;
- corpus validator manifest;
- any unsupported compression/container/state encountered.

If the real run exposes a save-format mismatch, the implementation and this record are updated before corpus timing is trusted.

## Final absent-air-aware policy

A later corpus policy must account for semantically present empty sections rather than weighting only serialized section tags.

The target dimension vertical ranges used for that reconstruction must themselves be source/runtime-qualified. They must not be copied from historical Minecraft values or hardcoded from memory.

Until that policy exists, all timing using `vanilla-save-region-v1-stored-sections` is exploratory real-data evidence, not the final production-selection weighting.
