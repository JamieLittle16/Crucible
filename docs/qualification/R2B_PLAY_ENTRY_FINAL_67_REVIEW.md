# R2B final 67-body Play-entry review

Status: **control-flow frontier closed; finite wire-only closure remains**  
Target: Minecraft Java Edition 26.2 / protocol 776  
Source archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`  
Review: `REVIEW-NET-R2B-PLAY-ENTRY-FINAL-26_2-001`  
Candidate count: **67**

## Result

Every exact body in the final 27 + 35 + inventory-closure review was inspected. The selected fresh/default Play-entry **control-flow frontier is now closed**: no further gameplay/helper discovery is required before R2B implementation.

This is not yet `GATE-NET-PLAY-ENTRY-26_2-001` admission. Seven custom wire families remain delegated by otherwise-reviewed packet roots. They are a finite codec/serializer closure, not another Play-semantic frontier.

The review therefore establishes:

```text
67 exact Play-entry bodies       REVIEWED
selected route/control flow      CLOSED
inventory synchronization        CLOSED
scoreboard/effects default-empty CLOSED
packet registration identities   CLOSED
primitive packet writers         CLOSED
custom second-order wire codecs  FINAL BLOCKER
world/chunk/light                 OUT OF R2B
```

## Selected profile

The first admitted R2B profile remains deliberately narrow:

- fresh player;
- ordinary non-transfer entry;
- selected offline profile with no remote chat session;
- empty scoreboard;
- no active effects;
- fresh/default inventory menu;
- chunk/light/world streaming excluded.

A later profile expansion must explicitly admit richer persisted inventory/effects/scoreboard/chat/transfer state rather than widening this gate implicitly.

## Inventory closure

The formerly ambiguous menu bootstrap is now exact.

`ServerPlayer.initInventoryMenu()` delegates to `initMenu(inventoryMenu)`. `initMenu` installs the player container synchronizer. `AbstractContainerMenu.setSynchronizer` immediately creates its remote synchronization slots and calls `sendAllDataToRemote()`.

`sendAllDataToRemote()` snapshots all slot items, the carried item and menu data slots, then invokes `ContainerSynchronizer.sendInitialData(...)`. The actual synchronizer is an anonymous implementation stored in `ServerPlayer.containerSynchronizer`; the new `<fieldinit:containerSynchronizer>()` Atlas evidence node fingerprints that implementation directly.

Its initial callback always publishes exactly one `ClientboundContainerSetContentPacket`, followed by one `ClientboundContainerSetDataPacket` for each initial data slot. The selected `InventoryMenu(Inventory, boolean, Player)` constructor creates the result/crafting/armor/player-inventory/offhand slots but installs **no `DataSlot` values**. Therefore the selected initial route emits the full content packet and **zero** container-data packets.

This is an observable semantic result. Crucible does not need Mojang's `LoadingCache`, `RemoteSlot`, copied `ItemStack` object graph or menu synchronization representation to reproduce it.

## Default-empty and conditional branches

The reviewed empty-scoreboard profile emits no scoreboard packet. The reviewed active-effect loop emits one update packet per active effect, so the selected zero-effect profile emits none and does not need an effect codec in this gate.

Weather events remain conditional on `level.isRaining()`. Server-data publication remains conditional on `status != null && !cookie.transferred()`. R2B must preserve those conditions rather than make either branch universally mandatory.

The post-add `broadcastAll(...)` is self-visible even on a one-player server: `placeNewPlayer` adds the joining player to `this.players` before broadcasting the second player-info initialization packet.

## Selected clientbound Play identities

The reviewed `GameProtocols.CLIENTBOUND_TEMPLATE` fixes the relevant zero-based insertion identities. Crucible may compile these into target constants; it does not need a runtime packet registry.

| Packet | ID |
|---|---:|
| `change_difficulty` | 10 |
| `commands` | 16 |
| `container_set_content` | 18 |
| `container_set_data` | 19 (not emitted initially by the selected menu) |
| `entity_event` | 34 |
| `game_event` | 38 |
| `initialize_border` | 43 |
| `login` | 49 |
| `player_abilities` | 64 |
| `player_info_update` | 70 |
| `player_position` | 72 |
| `recipe_book_add` | 74 |
| `recipe_book_settings` | 76 |
| `server_data` | 86 (conditional) |
| `set_default_spawn_position` | 97 |
| `set_held_slot` | 105 |
| `set_time` | 113 |
| `ticking_state` | 127 |
| `ticking_step` | 128 |
| `update_recipes` | 133 |

These identities still require the ordinary target codegen/golden drift checks before production use.

## Wire law already closed by the 67

The review directly closes the packet-local law for:

- Login outer field ordering and `CommonPlayerSpawnInfo` call position;
- abilities flags `1/2/4/8`, flying speed and walking speed;
- held-slot VarInt;
- entity event fixed int entity id + event byte;
- border snapshot and writer;
- game event id byte + float parameter, including `LEVEL_CHUNKS_LOAD_START = 13`;
- ticking-state float + boolean;
- ticking-step VarInt;
- player-position outer `teleport-id VarInt + PositionMoveRotation + Relative mask`;
- `PositionMoveRotation = position Vec3 + delta Vec3 + yRot float + xRot float`;
- relative-coordinate bit assignments 0 through 8;
- player-info initial action selection and action-table ordering;
- generic R1A/R1B primitives already admitted elsewhere: VarInt, booleans, strings, UUID/profile fields, nullable, resource keys and the relevant generic collection/composite helpers.

## Finite wire-only closure

Seven custom families remain before the Play-entry gate may be admitted:

1. **Command tree** — permission-filtered `fillUsableCommands`, command node enumeration/entry creation and `Entry.write`/argument-type serialization.
2. **Recipe-book settings** — `RecipeBookSettings.STREAM_CODEC`.
3. **Synchronized recipes** — `RecipePropertySet.STREAM_CODEC` and `SelectableRecipe.SingleInputSet.noRecipeCodec()`.
4. **Clock full sync** — `ClientboundSetTimePacket` and the clock network-state pack/codec surface.
5. **Default spawn** — `LevelData.RespawnData.STREAM_CODEC`.
6. **Common spawn dimension type** — `DimensionType.STREAM_CODEC`.
7. **Initial inventory item values** — `ItemStack.OPTIONAL_STREAM_CODEC` / `OPTIONAL_LIST_STREAM_CODEC` selected wire law.

These are the final source-review frontier. The closure tool must select only serializer/codec evidence for these families, preflight every exact Atlas row before source extraction, and remain source-rich/ephemeral outside Git.

Composition-stable command/recipe structures may later be precomputed or shared in production after their semantics and wire law are admitted. That optimization is encouraged; it does not justify inferring opaque bytes from replay.

## Architecture consequence

The 67-body review strengthens the intended R2B implementation shape rather than forcing a Mojang-shaped port:

```text
semantic bootstrap snapshot
        ↓
Target26_2 static packet identities + compact codecs
        ↓
composition-stable shared bodies where legal
        ↓
player-specific projected bodies
        ↓
StagedPublicationCursor
        ↓
existing bounded transactional egress
```

No runtime Play packet registry, second outbound queue, Java menu object graph or general-purpose command/recipe object graph is admitted by this source review.

## Next gate step

Run the bounded wire-only source review, inspect every returned exact serializer body, then canonicalize:

```text
67-body reviewed frontier
        +
wire-only codec closure
        ↓
VAR-NET-R2B-PLAY-*
        ↓
SEM-NET-R2B-PLAY-*
        ↓
GATE-NET-PLAY-ENTRY-26_2-001
        ↓
Target26_2 replay-free bootstrap implementation
```

Only after the independent source gate reports `admitted=true` may production `Target26_2` replace the non-world R1X Play replay.