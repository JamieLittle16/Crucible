# Target block-state data

This directory owns the **generated semantic input boundary** between the pinned official Minecraft target and Crucible's hot world engine.

The live server must not discover section mutation facts by repeatedly traversing registries, Java-shaped block objects, fluid objects, or dynamic property APIs. Instead, target tooling produces a normalized state dataset and `tools/state_data.py` deterministically turns that dataset into compact Rust tables.

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

## Commands

The low-level generator currently exposes:

```text
python3 tools/state_data.py inspect INPUT.json
python3 tools/state_data.py generate INPUT.json --output generated.rs --manifest manifest.json
python3 tools/state_data.py verify INPUT.json --output generated.rs --manifest manifest.json
```

M0.3A will connect the authoritative official-runtime/source extractor to this normalized boundary before generated 26.2 Rust data is accepted into the normal workspace.

## Provenance rule

The final committed 26.2 artifact must record at least the target version/protocol/data version, official input hashes, generator version, numeric assignment policy, normalized-input digest, and generation digest. Ordinary Crucible builds must not require Mojang source or server artifacts to be present.
