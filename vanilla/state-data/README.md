# Target block-state data

This directory owns the **generated semantic input boundary** between the pinned official Minecraft target and Crucible's hot world engine.

The live server must not discover section mutation facts by repeatedly traversing registries, Java-shaped block objects, fluid objects, or dynamic property APIs. Instead, target tooling produces a normalized state dataset and `tools/state_data.py` deterministically turns that dataset into compact Rust tables.

## Qualification chain

Production target data is accepted through two independent official evidence paths and an explicit binding step:

```text
pinned official mc-src.zip
        ↓
Vanilla Atlas
        ↓
source-qualification-spec.json
        ↓
fingerprint-only source qualification
                 ┐
                 │
                 ├─→ qualify_state_data.py
                 │          ↓
                 │   source+runtime-qualified dataset
                 │          ↓
                 │     state_data.py
                 │          ↓
                 │   generated Rust + manifest
                 │
official 26.2 server runtime
        ↓
official_state_data.py
        ↓
raw complete state/facts dataset
                 ┘
```

The source archive, official server and disposable Atlas SQLite database are deliberately not committed. Durable evidence records hashes/fingerprints and deterministic qualification digests rather than Mojang source bodies or binaries.

## Source qualification

`tools/state_source_qualification.py` **fails closed** when:

- the Atlas database was not built from the exact `vanilla.lock.toml` source archive;
- Minecraft/protocol/data versions differ;
- Atlas schema/version/fingerprint algorithm differ;
- a required type, field or method is missing;
- a method locator is ambiguous at its declared parameter count.

The committed specification covers the complete block-state registry, vanilla global state-ID mapping, air predicate, block random-tick predicate, fluid-state projection, fluid emptiness, fluid random-tick predicate, and the minimal official bootstrap surfaces required by the runtime probe.

Generate a local qualification artifact with:

```text
python3 tools/state_source_qualification.py \
  --atlas .crucible/vanilla/atlas.sqlite \
  --output vanilla/state-data/26.2-source-qualification.json
```

Once an artifact is reviewed/committed, byte-identical regeneration is checked with `--verify`.

## Source/runtime binding

Runtime-derived state data is deliberately **not production-qualified** merely because the official probe succeeds. `tools/qualify_state_data.py` joins the two evidence paths and rejects any mismatch in:

- target Minecraft/protocol/data version;
- pinned source archive;
- Atlas version/fingerprint algorithm;
- committed source-qualification specification;
- source-qualification digest/evidence set;
- pinned official-server SHA-256;
- runtime probe identity;
- complete/dense runtime state universe invariants.

The official Minecraft 26.2 runtime is pinned in `vanilla.lock.toml`. The current runtime evidence exposes 32,366 dense vanilla state IDs (`0..32365`), which proves `u16` sufficient for this target once the final source binding is accepted.

Bind raw runtime evidence with:

```text
python3 tools/qualify_state_data.py \
  --runtime-data .crucible/vanilla/26.2-block-states.raw.json \
  --source-qualification vanilla/state-data/26.2-source-qualification.json \
  --output .crucible/vanilla/26.2-block-states.qualified.json
```

The qualified dataset provenance includes the raw runtime input digest, exact official-server SHA-256, source archive SHA-256, source-qualification digest, and binder version. The subsequent generator input digest therefore commits to both independent oracle paths.

## One-command finalization

`tools/finalize_state_data.py` orchestrates the complete evidence chain without collapsing the independent tools into one implementation. With the pinned local Atlas available it will:

1. regenerate the source qualification;
2. obtain fresh official-runtime state data (or consume `--runtime-data`);
3. bind both evidence paths;
4. generate `crates/data/crucible-generated/src/lib.rs`;
5. generate `vanilla/state-data/26.2-state-data-manifest.json`;
6. validate that target and provenance identities survive the whole chain.

Normal finalization:

```text
python3 tools/finalize_state_data.py
```

Reuse an already captured runtime dataset:

```text
python3 tools/finalize_state_data.py \
  --runtime-data .crucible/vanilla/26.2-block-states.raw.json
```

After the source qualification, generated Rust and manifest are reviewed/committed, exact replay is:

```text
python3 tools/finalize_state_data.py --verify
```

Any changed source fingerprint, source specification, source archive, official-server binary, runtime facts, assignment policy or generated output causes verification to fail rather than silently refreshing production data.

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

`key` is a canonical state identity including properties when present. The normalized dataset is generated evidence, not handwritten gameplay configuration.

## Semantic facts

Only facts required by the current section contract belong in the first hot table:

- `non_air` — contributes to the exact non-air count;
- `counted_fluid` — contributes to the exact section fluid count;
- `random_block` — the block state itself may receive random block ticks;
- `random_fluid` — its counted fluid may receive random fluid ticks.

`counted_fluid => non_air` and `random_fluid => counted_fluid` are validated by both binding/generation paths. These relations follow the reviewed `LevelChunkSection` counting domain rather than being generic statements about every Minecraft API.

Future collision, light, heightmap, block-entity, or other metadata must use separate SoA tables unless evidence shows a combined layout is better.

## Numeric state identity

`BlockStateId` is opaque to semantic/plugin-facing code. The generator may use the vanilla global state numbering when that gives a deterministic dense identity mapping, or a different deterministic dense ordering. The selected assignment policy is provenance-tracked.

The representation width is selected from the actual generated state count. `u16` is not a source-level assumption.

## State-data commands

The low-level deterministic generator exposes:

```text
python3 tools/state_data.py inspect QUALIFIED.json
python3 tools/state_data.py generate QUALIFIED.json --output generated.rs --manifest manifest.json
python3 tools/state_data.py verify QUALIFIED.json --output generated.rs --manifest manifest.json
```

The raw official runtime probe is:

```text
python3 tools/official_state_data.py --version 26.2 --output RAW.json
```

The runtime probe remains useful in hosted CI because it can obtain the official server artifact directly. The source qualification step intentionally requires the separately pinned local official source corpus and therefore runs when the source-backed artifact is created or requalified.

## Provenance rule

The final committed 26.2 generated artifact must record at least the target version/protocol/data version, exact official runtime hash, pinned source-qualification digest, generator version, numeric assignment policy, qualified normalized-input digest, and generation digest. Ordinary Crucible builds must not require Mojang source or server artifacts to be present.
