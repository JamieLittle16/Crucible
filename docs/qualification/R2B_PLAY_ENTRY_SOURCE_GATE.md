# R2B Play-Entry Source Gate

Status: **67-body selected-route review complete; final wire-only closure in qualification**  
Target: Minecraft Java Edition 26.2, protocol 776  
Gate: `GATE-NET-PLAY-ENTRY-26_2-001` (reserved; not yet admitted)

## Purpose

R2B replaces the non-world portion of the finite R1X Play replay with Crucible-owned semantic bootstrap state and target projection. The gate exists to reproduce observable vanilla semantics without inheriting Mojang's object graph, runtime packet registry, flushing architecture or world representation.

The first gate is intentionally narrow: fresh player, ordinary non-transfer entry, offline/no remote chat session, empty scoreboard, no active effects, fresh/default inventory, and no chunk/light/world streaming.

## Current evidence chain

```text
pinned Minecraft 26.2 source
        |
Play-entry discovery/frontier hardening        COMPLETE
        |
instance-field evidence projection             COMPLETE
        |
final 67 exact-body selected-route review      COMPLETE
        |
selected route/control-flow frontier           CLOSED
        |
seven-family wire/serializer closure           IN QUALIFICATION
        |
VAR-NET-R2B-PLAY-* + SEM-NET-R2B-PLAY-*
        |
GATE-NET-PLAY-ENTRY-26_2-001
        |
Target26_2 replay-free semantic bootstrap
```

No packet identity, field order, default branch or mandatory stage may be admitted from replay position alone.

## Final 67-body result

`REVIEW-NET-R2B-PLAY-ENTRY-FINAL-26_2-001` contains 67 exact pinned bodies: the historical 27-body pass, 35-body follow-up, four inventory/synchronizer closure nodes, and the one actual `InventoryMenu` constructor reported by the pinned Atlas.

Every body has now been inspected. The selected-route control-flow frontier is closed. See `R2B_PLAY_ENTRY_FINAL_67_REVIEW.md` for the source-free result, exact selected packet identities and branch dispositions.

The important former ambiguity—initial inventory/menu synchronization—is closed directly from source:

```text
ServerPlayer.initInventoryMenu()
    -> ServerPlayer.initMenu(inventoryMenu)
    -> AbstractContainerMenu.setSynchronizer(...)
    -> AbstractContainerMenu.sendAllDataToRemote()
    -> ServerPlayer#<fieldinit:containerSynchronizer>()
       .sendInitialData(...)
    -> ClientboundContainerSetContentPacket
```

The selected `InventoryMenu(Inventory, boolean, Player)` constructor installs no `DataSlot`. Therefore the initial data-slot array is empty and no `ClientboundContainerSetDataPacket` is emitted for this profile.

The anonymous synchronizer is fingerprinted through the general synthetic Atlas evidence node:

```text
net.minecraft.server.level.ServerPlayer#<fieldinit:containerSynchronizer>()
```

This is evidence tooling only; it creates no runtime abstraction.

## Branches now closed

The selected empty scoreboard emits no scoreboard packets. The selected no-active-effect profile emits no mob-effect packets. Weather game events remain conditional on `level.isRaining()`.

Server-data publication remains conditional on `status != null && !cookie.transferred()` and must not be promoted to a universal stage without separately binding status presence.

`broadcastAll(...)` is self-visible even for the first player because `placeNewPlayer` inserts the joining player into `this.players` before the post-add player-info broadcast.

## Final wire-only frontier

The 67-body review leaves seven custom subordinate wire families. These are not new gameplay semantics:

1. command-tree permission filtering/node entry serialization;
2. `RecipeBookSettings.STREAM_CODEC`;
3. synchronized recipe-property/stonecutter codecs;
4. `ClientboundSetTimePacket` and clock network-state packing;
5. `LevelData.RespawnData.STREAM_CODEC`;
6. `DimensionType.STREAM_CODEC`;
7. `ItemStack` optional/list stream codecs used by the initial container snapshot.

`vanilla/reviews/network/r2b-play-entry-wire-closure-plan.json` freezes that boundary. `tools/r2b_play_entry_wire_closure_source_review.py` validates the exact prior 67-body dossier, excludes already-reviewed identities, resolves only the committed wire families, preflights required names/types before source extraction, and emits source-rich output outside Git.

The command and clock selectors are deliberately bounded at their named serializer/type-family boundary rather than one guessed private helper. That prevents another iterative missing-helper loop while still forbidding world/chunk/movement/general gameplay expansion.

## Architectural admission rules

The resulting target implementation must preserve:

- semantic stage labels rather than a runtime packet registry;
- target-owned 26.2 packet identities/codecs in `crucible-target-26-2`;
- target-neutral bounded progression in `crucible-publication-core`;
- compact per-connection bootstrap progress;
- one bounded transactional egress path and no second outbound queue;
- shared/precomputed immutable composition-stable bodies where semantics permit;
- player-specific projection from compact semantic state rather than Mojang-shaped object graphs;
- explicit later `WorldProjection` ownership for chunk/light data.

Composition-stable command/recipe bodies are excellent candidates for process/composition-level precomputation after their source law is admitted. Replay bytes may remain differential/golden evidence; they are not the semantic authority.

## Gate admission requirements

Before `GATE-NET-PLAY-ENTRY-26_2-001` can report `admitted=true`, source-free repository artifacts must establish:

1. exact fingerprints for all material selected-route control-flow and outbound wire bodies;
2. reviewed hazards and no unresolved material delegates;
3. SEM rules for mandatory order, conditional/default-empty branches, teleport transaction and inventory bootstrap;
4. target packet identities and wire contracts/generated facts with drift checks;
5. selected stage ordering derived from SEM rather than capture order.

The first production implementation then requires golden codec tests, stage-selection differential tests, exhaustive bounded-cursor/backpressure tests, teleport transaction tests and an independent stock 26.2 client probe while R1X replay is removed.

## Exit

This source-gate phase exits only when the seven-family wire review is inspected, all material outbound delegates are closed, the independent Atlas source gate reports `admitted=true`, and the target stage plan can be implemented without replay-derived semantics.
