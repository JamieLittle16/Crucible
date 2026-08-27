# R2B Play-Entry Source Gate

Status: **67-body control flow closed; 117-body wire review complete; final reusable dynamic seams in qualification**  
Target: Minecraft Java Edition 26.2, protocol 776  
Gate: `GATE-NET-PLAY-ENTRY-26_2-001` (reserved; not yet admitted)

## Purpose

R2B replaces the non-world portion of the finite R1X Play replay with Crucible-owned semantic bootstrap state and target projection. The gate exists to reproduce observable vanilla semantics without inheriting Mojang's object graph, runtime packet registry, flushing architecture or world representation.

The first gate is intentionally narrow: fresh player, ordinary non-transfer entry, offline/no remote chat session, empty scoreboard, no active effects, **empty initial inventory/crafting-menu state**, default non-operator permission profile, pinned default composition, and no chunk/light/world streaming.

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
117-body seven-family wire review              COMPLETE
        |
composition-stable/dynamic boundary            FROZEN
        |
small reusable dynamic-codec seam review       IN QUALIFICATION
        |
VAR-NET-R2B-PLAY-* + SEM-NET-R2B-PLAY-*
        |
GATE-NET-PLAY-ENTRY-26_2-001
        |
Target26_2 replay-free semantic bootstrap
```

No packet identity, field order, default branch or mandatory stage may be admitted from replay position alone.

## Final 67-body result

`REVIEW-NET-R2B-PLAY-ENTRY-FINAL-26_2-001` contains 67 exact pinned bodies. Every body was inspected and the selected-route control-flow frontier is closed. See `R2B_PLAY_ENTRY_FINAL_67_REVIEW.md`.

The former inventory ambiguity is closed directly from source:

```text
ServerPlayer.initInventoryMenu()
    -> ServerPlayer.initMenu(inventoryMenu)
    -> AbstractContainerMenu.setSynchronizer(...)
    -> AbstractContainerMenu.sendAllDataToRemote()
    -> ServerPlayer#<fieldinit:containerSynchronizer>()
       .sendInitialData(...)
    -> ClientboundContainerSetContentPacket
```

The selected `InventoryMenu(Inventory, boolean, Player)` constructor installs no `DataSlot`, so the initial data-slot array is empty and no `ClientboundContainerSetDataPacket` is emitted for this profile.

The selected empty scoreboard emits no scoreboard packets. The no-active-effect profile emits no mob-effect packets. Weather remains conditional on `level.isRaining()`. Server-data remains conditional on `status != null && !cookie.transferred()`. `broadcastAll(...)` is self-visible after the joining player is inserted into the player list.

The source-free machine companion `vanilla/reviews/network/r2b-play-entry-final-67-review-result.json` binds the exact dossier SHA and records that every exact body and Atlas-observed hazard was reviewed. Canonical VAR generation must use the dossier fingerprints; the machine result does not contain or replace source evidence.

## 117-body wire result

`REVIEW-NET-R2B-PLAY-WIRE-CLOSURE-26_2-001` contains 117 exact bodies across seven deliberately bounded families. Every body was inspected. See `R2B_PLAY_ENTRY_WIRE_117_REVIEW.md`.

The review establishes three mechanism classes.

### Dynamic semantic projection

These values vary with live semantic state and require admitted target codecs:

- recipe-book settings;
- clock/time state;
- dimension holder;
- default spawn position;
- player-specific bootstrap fields already closed by the 67-body pass.

### Composition-stable publication artifacts

For this first profile, command and synchronized-recipe bodies are fixed by the pinned target/composition/profile and are not rebuilt per connection.

The command review closes permission filtering, node enumeration, flags, redirect/child structure, literal/argument envelope and suggestion identifiers. The final argument-template payload delegates to concrete `ArgumentTypeInfo` serializers. R2B does **not** admit a generic dynamic command serializer. Instead, the selected default non-operator command body is a named immutable artifact keyed by:

```text
protocol + composition lock + permission profile + source commitment + body commitment
```

Likewise, the update-recipes packet is composition-stable. Its outer property-set/stonecutter structure is source-reviewed, while deeper recipe/item-display serialization is sealed in the selected immutable body.

This follows the same deliberately narrow mechanism already used for large Configuration registry/tag publication: source establishes why/when/which semantic publication exists; independent body-equivalence qualification establishes the exact bytes for the frozen composition. Replay position is never semantic authority.

A future dynamic commands/plugins/datapacks/recipe-reload profile must separately source-admit the concrete serializers before it can replace these artifacts.

### Explicit empty-inventory state

`ItemStack.OPTIONAL_STREAM_CODEC` proves:

```text
empty     -> VarInt 0
non-empty -> count + Item holder + DataComponentPatch
```

The first R2B profile is therefore stated as an explicit empty initial inventory/crafting-menu state. Only the empty branch is admitted. Persisted/non-empty inventory bootstrap is a later profile expansion and must admit Item/DataComponentPatch encoding before use.

The source-free machine companion `vanilla/reviews/network/r2b-play-entry-wire-117-review-result.json` binds the exact dossier SHA, family counts, mechanism disposition and reviewed-hazard status. It remains blocked on the named final dynamic seam until that review closes.

## Final reusable dynamic seam

Only reusable generic wire law remains:

1. `ByteBufCodecs.map` plus registry-holder/id-mapper helpers used by clock and dimension-holder projection;
2. `GlobalPos.STREAM_CODEC`, `ResourceKey.streamCodec`, and the packed `BlockPos` primitive used by default-spawn projection.

`vanilla/reviews/network/r2b-play-entry-final-seams-plan.json` freezes this boundary. It hard-pins the SHA-256 of the exact 117-body source-rich dossier and forbids commands, recipes, non-empty ItemStack, world/chunk/light/movement and gameplay helpers.

`tools/r2b_play_entry_final_seams_source_review.py` preflights only those named reusable types/methods and emits source-rich evidence outside Git.

## Final evidence execution path

The remaining local work is deliberately one bounded evidence operation rather than another exploration pass.

`tools/r2b_play_entry_collect_evidence.py` runs both required collectors in one fail-closed external directory:

```text
exact 117-body dossier
        -> final reusable dynamic-seam source dossier + worksheet

pinned validated full R1X capture
        -> commands/update-recipes black-box oracle bodies
```

The oracle input is the **full validated source-free capture JSON** (`2331` Play bodies / `6,135,522` Play body bytes), not the later 385-frame runtime prefix. The collector validates the complete capture commitment first and only then extracts the two source-qualified composition artifacts. This prevents a truncated runtime image from being mistaken for the black-box oracle source.

The combined directory is `EPHEMERAL_DO_NOT_COMMIT` because it contains official source excerpts. The composition oracle is still explicitly `BLACK_BOX_CONFIRMATION_ONLY` and `production_admitted=false` even though it shares the same external bundle.

After every final-seam body is inspected and the source-free worksheet is completed, `tools/r2b_play_entry_finalize.py` validates the exact 67/117 dossier commitments, every final-seam decision/hazard, the complete 15-rule semantic contract and both oracle bodies. Its successful output is only:

```text
ADMISSION_INPUTS_VERIFIED
gate_emission_ready=true
production_admitted=false
```

`tools/r2b_play_entry_gate_materialize.py` then canonicalizes the exact reviewed dossier fingerprints into source-free `VAR-NET-R2B-PLAY-*` records, a candidate `GATE-NET-PLAY-ENTRY-26_2-001.json`, and the selected composition-artifact contract. It has explicit SEM mappings for the complete historical 67-body plan and all seven 117-body families; unknown candidates/families fail closed.

The materializer is still not admission. The generated gate must be installed into the repository evidence tree and run through the ordinary `tools/vanilla_source_gate.py` against the pinned Atlas database. Only that independent pass may report `admitted=true`.

This pipeline intentionally allows the source-rich dossiers to be discarded after successful source-free canonicalization; no later implementation step should require reopening official source text merely to recover reviewed fingerprints.

## Architectural admission rules

The resulting target implementation must preserve:

- semantic stage labels rather than a runtime packet registry;
- target-owned 26.2 packet identities/codecs in `crucible-target-26-2`;
- target-neutral bounded progression in `crucible-publication-core`;
- compact per-connection bootstrap progress;
- one bounded transactional egress path and no second outbound queue;
- immutable composition-stable bodies resolved once and shared across connections;
- player/world-specific projection from compact semantic state rather than Mojang-shaped object graphs;
- explicit later `WorldProjection` ownership for chunk/light data;
- composition/profile commitments that fail closed rather than silently applying a static body to an incompatible datapack/permission profile.

## Gate admission requirements

Before `GATE-NET-PLAY-ENTRY-26_2-001` can report `admitted=true`, source-free repository artifacts must establish:

1. exact fingerprints for every dynamic selected-route control-flow/wire body actually executed by the first profile;
2. reviewed hazards and no unresolved material delegate on those dynamic paths;
3. explicit immutable-artifact contracts and independent body commitments for selected commands/update-recipes publication;
4. SEM rules for mandatory order, conditional/default-empty branches, teleport transaction and inventory bootstrap;
5. target packet identities and wire contracts/generated facts with drift checks;
6. selected stage ordering derived from SEM rather than capture order.

The first production implementation then requires golden codec/body tests, stage-selection differential tests, exhaustive bounded-cursor/backpressure tests, teleport transaction tests and an independent stock 26.2 client probe while R1X replay is removed.

## Exit

This source-gate phase exits when the small final reusable-codec review is accepted, the immutable command/recipe artifact contracts are materialized, the independent Atlas source gate reports `admitted=true`, and the target stage plan can be implemented without replay-derived semantics.
