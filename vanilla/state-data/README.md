# Target block-state data

This directory owns the **generated semantic input boundary** between the pinned official Minecraft target and Crucible's hot world engine.

The live server must not discover section mutation facts by repeatedly traversing registries, Java-shaped block objects, fluid objects, or dynamic property APIs. Instead, target tooling produces a normalized state dataset and `tools/state_data.py` deterministically turns that dataset into compact Rust tables.

## Qualification chain

Production target data is accepted through two independent official evidence paths:

```text
pinned official mc-src.zip
        ↓
Vanilla Atlas
        ↓
source-qualification-spec.json
        ↓
fingerprint-only qualification artifact

pinned official server runtime + mappings
        ↓
official_state_data.py
        ↓
normalized complete state/facts dataset
```

The source archive itself and the disposable Atlas SQLite database are deliberately not committed. The qualification artifact records only source locators, file hashes, normalized/body fingerprints, target identity, Atlas identity, the source-archive digest and a deterministic qualification digest.

`tools/state_source_qualification.py` **fails closed** when:

- the Atlas database was not built from the exact `vanilla.lock.toml` source archive;
- Minecraft/protocol/data versions differ;
- Atlas schema/version/fingerprint algorithm differ;
- a required type, field or method is missing;
- a method locator is ambiguous at its declared parameter count.

The committed specification currently covers the complete block-state registry, vanilla global state-ID mapping, air predicate, block random-tick predicate, fluid-state projection, fluid emptiness, fluid random-tick predicate, and the minimal official bootstrap surfaces required by the runtime probe.

Generate a local qualification artifact with:

```text
python3 tools/state_source_qualification.py \
  --atlas .crucible/vanilla/atlas.sqlite \
  --output .crucible/vanilla/26.2-state-source-qualification.json
```

Once an artifact is reviewed/committed, byte-identical regeneration is checked with `--verify`. Runtime-derived state data is not production-qualified merely because the runtime probe succeeds; the source and runtime evidence must be joined before the final generated target crate is frozen.

## Normalized input schema

```json
{
  "schema": 1,
  "target": {
    "minecraft_version": "26.2",
    "protocol_version": 776,
    "data_version": 4903
  },
  "air_key": "minecraft:air",
  "states": [
    {
      "key": "minecraft:air",
      "vanilla_id": 0,
      "non_air": false,
      "counted_fluid": false,
      "random_block": false,
      "random_fluid": false
    }
  ]
}
```

`key` is a canonical state identity including properties when present. The normalized dataset is authoritative generated evidence, not handwritten gameplay configuration.

## Semantic facts

Only facts required by the current section contract belong in the first hot table:

- `non_air` — contributes to the exact non-air count;
- `counted_fluid` — contributes to the exact section fluid count;
- `random_block` — the block state itself may receive random block ticks;
- `random_fluid` — its counted fluid may receive random fluid ticks.

`counted_fluid => non_air` and `random_fluid => counted_fluid` are validated by the generator. These relations follow the reviewed `LevelChunkSection` counting domain rather than being generic statements about every Minecraft API.

Future collision, light, heightmap, block-entity, or other metadata must use separate SoA tables unless evidence shows a combined layout is better.

## Numeric state identity

`BlockStateId` is opaque to semantic/plugin-facing code. The generator may use the vanilla global state numbering when that gives a deterministic dense identity mapping, or a different deterministic dense ordering. The selected assignment policy is provenance-tracked.

The representation width is selected from the actual generated state count. `u16` is not a source-level assumption.

## State-data commands

The low-level deterministic generator exposes:

```text
python3 tools/state_data.py inspect INPUT.json
python3 tools/state_data.py generate INPUT.json --output generated.rs --manifest manifest.json
python3 tools/state_data.py verify INPUT.json --output generated.rs --manifest manifest.json
```

The official runtime probe is:

```text
python3 tools/official_state_data.py --version 26.2 --output INPUT.json
```

The runtime probe remains useful in hosted CI because it can obtain the official server artifact directly. The source qualification step intentionally requires the separately pinned local official source corpus and therefore runs when the source-backed artifact is created or requalified.

## Provenance rule

The final committed 26.2 artifact must record at least the target version/protocol/data version, official runtime input hashes, pinned source-qualification digest, generator version, numeric assignment policy, normalized-input digest, and generation digest. Ordinary Crucible builds must not require Mojang source or server artifacts to be present.
