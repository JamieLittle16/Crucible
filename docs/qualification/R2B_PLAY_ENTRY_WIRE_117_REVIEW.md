# R2B Play-Entry 117-Body Wire Review

Status: **reviewed; broad seven-family frontier closed, three dynamic codec seams remain**  
Review: `REVIEW-NET-R2B-PLAY-WIRE-CLOSURE-26_2-001`  
Target: Minecraft Java Edition 26.2, protocol 776

## Evidence identity

The source-rich review contained exactly 117 fingerprinted bodies from the pinned 26.2 archive:

- `COMMAND_TREE`: 37
- `CLOCK_FULL_SYNC`: 42
- `RECIPE_BOOK_SETTINGS`: 12
- `SYNCHRONIZED_RECIPES`: 11
- `DIMENSION_TYPE`: 8
- `DEFAULT_SPAWN`: 4
- `INITIAL_INVENTORY`: 3

The review was prepared from the already-closed 67-body Play-entry dossier and did not reopen world,
chunk, light, movement or general gameplay discovery.

## Review result

Every exact body was inspected for the contribution named by its review group. The seven-family pass
successfully closes the broad serializer frontier, but it also proves that pretending every selected
payload should become a dynamic runtime encoder would be the wrong R2B architecture.

Three classes of evidence must be treated differently:

1. **dynamic semantic projection** — values vary with the live player/world and require an admitted
   target codec;
2. **composition-stable publication** — values are fixed by the target/composition/profile and should
   be an immutable, provenance-bound publication artifact rather than rebuilt per connection;
3. **outbound-irrelevant implementation detail** — source was intentionally over-selected to make the
   review finite but the body is not part of the selected server-to-client wire law.

This distinction is semantic, not an optimization loophole. A static artifact is admitted only for an
explicit composition/profile and must carry source/capture/body commitments. A future dynamic profile
must separately admit the serializer law it executes.

## Command tree

The selected command envelope is source-closed:

- usable commands are copied recursively only when `child.canUse(commandFilter)`;
- redirects point at the converted node;
- enumeration is breadth-first over children and redirects;
- entry flags encode redirect (`8`), executable (`4`), restricted (`32`), node kind and optional
  suggestions (`16`);
- each entry writes flags, child ids, optional redirect and then the node stub;
- literal stubs write their UTF identifier;
- argument stubs write their UTF identifier, command-argument registry id and optional suggestion id.

The final argument payload call is deliberately polymorphic:

```text
info.serializeToNetwork(template, output)
```

and concrete `ArgumentTypeInfo.unpack/type/serializeToNetwork` implementations are outside the 117
bodies. R2B therefore does **not** claim a generic dynamic command-tree encoder.

For the first admitted profile the command body is instead a **composition-locked immutable artifact**
for the default non-operator permission profile. It is named by semantic stage, not replay position,
and must be keyed by protocol, composition lock, permission profile, source commitment and body
commitment. The source-reviewed command construction above proves why and when the stage exists; an
independent body-equivalence qualification proves the exact composition bytes.

Dynamic command/plugin/datapack profiles remain a later source gate and may not silently reuse this
admission.

## Synchronized recipes

`ClientboundUpdateRecipesPacket` was already established as mandatory and composition-stable. This
review closes the property-set and `SingleInputSet` envelope, including the seven vanilla property-set
keys and list structure. `SingleInputSet.noRecipeCodec()` delegates once more to
`SingleInputEntry.noRecipeCodec()`, and property sets delegate item-holder serialization.

R2B again chooses the correct first mechanism: the default-composition update-recipes body is a
composition-locked immutable artifact. Runtime recipe/datapack mutation is not admitted by this gate.
This avoids building a cold, complex serializer into every connection while preserving exact bytes.

## Recipe-book settings

This family is dynamically closed for the selected fresh profile. `RecipeBookSettings.STREAM_CODEC`
serializes four `TypeSettings` in this order:

```text
crafting -> furnace -> blast_furnace -> smoker
```

Each `TypeSettings` is exactly `(open: BOOL, filtering: BOOL)`. The default constructor selects
`TypeSettings.DEFAULT` for all four, and `DEFAULT` is `(false, false)`.

The target may therefore project the eight booleans directly from compact recipe-book state without a
Mojang-shaped object.

## Clock/full time sync

The mandatory time packet is:

```text
LONG game_time
map<WorldClock holder, ClockNetworkState>
```

and `ClockNetworkState` is:

```text
VAR_LONG total_ticks
FLOAT partial_tick
FLOAT rate
```

`ClockInstance.packNetworkState` also binds the paused rule: the published rate is `0.0` when the
instance is paused or the global `ADVANCE_TIME` rule is disabled; otherwise it is the instance rate.

Most of the 42 selected clock bodies are persistence/command/internal state and are not required by
the first outbound encoder. The remaining dynamic wire seam is the reusable `ByteBufCodecs.map` /
registry-holder encoding used by the map key.

## Dimension type

The Play wire does not serialize the large `DimensionType` direct/data codec. Its `STREAM_CODEC` is
only:

```text
ByteBufCodecs.holderRegistry(Registries.DIMENSION_TYPE)
```

The direct codec and monster/environment fields in this review are therefore outbound-irrelevant for
R2B. The only remaining dynamic seam is the reusable registry-holder wire codec.

## Default spawn

`LevelData.RespawnData.STREAM_CODEC` is exactly:

```text
GlobalPos.STREAM_CODEC
FLOAT yaw
FLOAT pitch
```

`RespawnData.of` wraps yaw and clamps pitch to `[-90, 90]`. The remaining dynamic seam is
`GlobalPos.STREAM_CODEC` and its reusable resource-key/block-position primitives.

## Initial inventory

`ItemStack.OPTIONAL_STREAM_CODEC` gives an important selected-profile shortcut directly from source:

```text
empty ItemStack    -> VarInt 0
non-empty ItemStack -> count + Item holder + DataComponentPatch
```

R2B's first profile is now stated precisely as **fresh player with an empty initial inventory and
crafting-menu snapshot**. Under that explicit state restriction the non-empty Item/DataComponentPatch
branch is not material to R2B admission. Persisted or non-empty inventory bootstrap is a later profile
expansion and must source-admit those subordinate codecs before use.

## Remaining source seam

The broad source archaeology is finished. One deliberately small reusable-codec review remains:

1. `ByteBufCodecs.map` and registry-holder/id-mapper helpers required by time/dimension projection;
2. `ResourceKey.streamCodec` / `GlobalPos.STREAM_CODEC` and the exact packed `BlockPos` primitive it
   uses for default-spawn projection.

`REVIEW-NET-R2B-PLAY-FINAL-SEAMS-26_2-001` is reserved for exactly this generic wire seam and hard-pins
the SHA-256 of the 117-body dossier. It must not include commands, recipes, ItemStack non-empty state,
world/chunk/light/movement or gameplay helpers.

After that review is accepted, the source gate can canonicalize directly into VAR/SEM records plus two
composition-artifact contracts (`commands`, `update_recipes`) and the dynamic target codecs.
