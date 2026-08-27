# R2B Play-Entry Semantic Contract — Minecraft Java 26.2

Status: **selected fresh/default route frozen; final reusable codec seam and production implementation pending**  
Target: Minecraft **26.2**, protocol **776**, data version **4903**  
Source archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`  
Fingerprint algorithm: `java-token-v2-literal-sensitive`

> NOTE: the source SHA above is intentionally validated by the finalizer against
> `vanilla/vanilla.lock.toml`; this document must not be used as an independent source pin.

R2B replaces the non-world portion of the finite R1X Play replay with Crucible-owned semantic
bootstrap state and target projection. The contract describes the selected fresh/offline/default
entry profile only. Chunk/light publication and continuing world tracking are an explicit R2C/R2D
seam.

Mojang's runtime listener graph, command serializer registry, menu object graph, packet registry and
flush batching are implementation observations, not Crucible architecture.

## Exact obligations

- **SEM-NET-R2B-PLAY-001 — selected profile and target identity.** The first admitted R2B route is
  Minecraft 26.2 protocol 776, ordinary non-transfer entry, offline/no remote chat session, fresh
  player state, empty scoreboard, no active effects, no custom boss events relevant to the joining
  player, and a fresh empty inventory. Clientbound Play packet identities are derived from the
  reviewed `GameProtocols` insertion order; Crucible uses generated/static target facts and does not
  require a runtime packet registry. Any richer transfer/chat/persisted-player/plugin profile is a
  later explicit expansion.

- **SEM-NET-R2B-PLAY-002 — direct Play-entry publication prefix.** After Configuration acknowledgement
  reaches the prepared fresh-player Play handoff, the direct bootstrap prefix is ordered as:
  `login` -> `change_difficulty` -> `player_abilities` -> `set_held_slot` ->
  `update_recipes`. The packet bodies are projections of compact semantic state. This ordering is
  source law; the R1X capture is not its authority.

- **SEM-NET-R2B-PLAY-003 — permission and command publication.** Initial permission publication
  emits the permission-level entity event and then publishes the permission-filtered command tree.
  For a fixed target, command composition, built-in argument/registry composition, enabled-feature
  profile and permission profile, the resulting command publication is composition-stable and may
  be represented by one immutable qualified projection artifact. A different key is a cache miss or
  unsupported profile; it must never silently reuse another permission/composition artifact.

- **SEM-NET-R2B-PLAY-004 — initial recipe-book publication.** Initial recipe-book synchronization
  always sends recipe-book settings and then the initial recipe-add publication. The recipe-add
  packet remains present when the selected fresh profile has no known recipes. Recipe-book settings
  contain crafting/furnace/blast-furnace/smoker `open` and `filtering` state; all eight booleans are
  false in the selected fresh/default profile, while the semantic representation remains player
  state rather than an opaque process-wide constant.

- **SEM-NET-R2B-PLAY-005 — selected default-empty branches.** An empty scoreboard emits no selected
  scoreboard bootstrap packets. Zero active effects emit no effect packets. The selected default
  custom-boss-event path emits no additional player bootstrap packet. These are profile conditions,
  not permission to omit traffic when future state is non-empty.

- **SEM-NET-R2B-PLAY-006 — initial teleport transaction.** Fresh placement constructs an absolute
  position/rotation destination with zero velocity and an empty relative-coordinate set. The
  connection's teleport id advances, wrapping `Integer.MAX_VALUE` to zero, awaiting-position state
  is installed, server semantic position is updated, and one `player_position` packet is emitted.
  R2B must retain this as an explicit transaction awaiting the corresponding client acknowledgement;
  publication alone is not equivalent to a completed teleport.

- **SEM-NET-R2B-PLAY-007 — server-status branch.** Server-data publication occurs only when server
  status is present and the entry is not a transfer. The selected ordinary non-transfer profile may
  publish it when status is configured; this conditional stage must not be inferred from captured
  packet position.

- **SEM-NET-R2B-PLAY-008 — initial player-info visibility.** Before inserting the joining player into
  the active player collection, the joining connection receives initialization information for the
  existing players. The joining player is then inserted and an initialization entry for that player
  is broadcast to all active players, including the joining connection. Initial actions are
  `ADD_PLAYER`, `INITIALIZE_CHAT`, `UPDATE_GAME_MODE`, `UPDATE_LISTED`, `UPDATE_LATENCY`,
  `UPDATE_DISPLAY_NAME`, `UPDATE_HAT`, and `UPDATE_LIST_ORDER`. The selected offline profile has no
  remote chat-session payload.

- **SEM-NET-R2B-PLAY-009 — level bootstrap ordering.** Selected level information is ordered as:
  initialize border -> full clock synchronization -> default spawn -> optional weather publications
  when raining -> `LEVEL_CHUNKS_LOAD_START` game event -> joining-player tick-rate state. The clock
  and spawn values are semantic world/dimension state, not process constants. Weather absence in a
  clear-world profile is a conditional-empty result.

- **SEM-NET-R2B-PLAY-010 — initial inventory synchronization.** Installing the selected
  `InventoryMenu` synchronizer immediately sends one full `container_set_content` snapshot. The
  selected menu constructor installs no `DataSlot`, so the initial `container_set_data` count is
  zero. Every selected fresh inventory/carried `ItemStack` is empty and therefore uses the explicit
  empty stack encoding path. A non-empty stack presented to the fresh-only R2B encoder is an error;
  general non-empty/persisted ItemStack admission belongs to a later profile.

- **SEM-NET-R2B-PLAY-011 — composition-stable shared projections.** The permission-filtered command
  tree and synchronized recipe publication are immutable projection candidates for a fixed explicit
  composition key. Source review defines the construction dependencies and invalidation law; a
  pinned stock-server oracle may confirm exact bytes for one admitted key. Ordinary joins must be
  able to reuse the exact immutable artifact rather than repeat identical graph filtering,
  registry dispatch and serialization. Artifact sharing remains byte-bounded and backpressure-safe.

- **SEM-NET-R2B-PLAY-012 — dynamic target codecs.** Recipe-book settings, clock state, default spawn,
  dimension bootstrap facts, teleport/player state and player-info state are target-specific
  projections of compact semantic values. Reusable registry/holder/resource-key/global-position
  primitives used by these dynamic surfaces must be source-admitted before
  `GATE-NET-PLAY-ENTRY-26_2-001` can become green. `crucible-target-26-2`, not product composition,
  owns the 26.2 wire law.

- **SEM-NET-R2B-PLAY-013 — bounded staged publication.** R2B publication progresses through semantic
  stages, not a packet-name registry. `StagedPublicationCursor` may process at most one body or one
  stage boundary per service opportunity. Preparing/admitting output and advancing bootstrap state
  are transactional: failed bounded-egress admission cannot skip, duplicate or commit a stage. No
  second outbound queue is permitted.

- **SEM-NET-R2B-PLAY-014 — explicit world projection seam.** R2B does not own chunk data, light data,
  chunk-interest tracking or continuing world streaming. The semantic bootstrap terminates at an
  explicit `WorldProjection` seam. R2C supplies Crucible-owned dimension/chunk/light projection
  through that seam; R2B must not create a temporary network-owned world representation.

- **SEM-NET-R2B-PLAY-015 — capture/oracle evidence boundary.** The pinned stock-client/server capture
  may provide black-box exact-byte confirmation for a composition-stable projection only after
  source evidence establishes the semantic stage, conditions and complete artifact key. Capture
  order/bytes alone never define mandatory ordering, branch law or target semantics. Oracle-derived
  artifacts remain non-production until tied to this gate and a complete composition identity.

## Projection classes

The selected implementation model is deliberately heterogeneous:

```text
composition-stable immutable projection
  - permission-filtered command tree
  - synchronized recipes

small semantic runtime codec
  - recipe-book settings
  - clock state
  - default spawn
  - dimension bootstrap facts
  - player/teleport/player-info values

selected-profile specialization
  - fresh empty inventory
```

This distinction is semantic/performance architecture, not an excuse to omit source evidence.
Composition-stable artifacts still need source-backed dependency/invalidation laws and exact
black-box/golden qualification for the admitted key.

## Selected semantic stage plan

The eventual target image may use a stage plan structurally similar to:

```text
EnterWorld
CorePlayerState
CompositionRecipes
PermissionAndCommands
RecipeBook
Teleport
ConditionalServerStatus
PlayerInfo
LevelBootstrap
InventorySnapshot
WorldProjection
Complete
```

Stage labels are semantic groupings, not packet IDs. A stage may contain several ordered bodies or
be empty under an explicitly admitted branch. The exact grouping is an implementation mechanism so
long as the observable order required by the rules above is preserved.

## Implementation freedom

After source admission Crucible may:

- use generated direct packet identities and dense/static target facts;
- generate/cache command and synchronized-recipe artifacts once per exact composition key;
- share immutable encoded bodies among matching joining clients;
- encode compact player/world values directly without Mojang-style packet/object graphs;
- use the existing bounded publication driver and staged cursor instead of Mojang's suspended
  flushing representation;
- satisfy dimension bootstrap from a reference immutable fact provider until R2C installs the
  permanent `DimensionInstance` provider.

Crucible may not:

- reuse command/recipe bytes across a composition or permission mismatch;
- silently fall back to an unreviewed generic ItemStack encoder;
- derive semantics from R1X frame ordering;
- let the network layer mutate authoritative player/world state directly;
- mix chunk/light ownership into R2B.

## Qualification requirements

Before R2B can be considered qualified:

1. all control-flow, selected packet/writer and reusable dynamic seam source records are
   `VAR_REVIEWED` against the pinned archive;
2. `GATE-NET-PLAY-ENTRY-26_2-001` is independently green;
3. target-owned golden codec tests cover all direct dynamic packet surfaces;
4. composition artifacts have explicit keys, exact hashes and deterministic oracle/reference
   regeneration or comparison;
5. stage-selection/default-empty tests compare against a simple reference model;
6. exhaustive/small-capacity tests prove staged progression and backpressure rollback;
7. teleport acknowledgement state is exercised through valid/stale/wrong/duplicate cases;
8. a stock 26.2 client enters Play with the non-world R1X replay removed while the quarantined
   R1X world/chunk/light scaffold may remain until R2C;
9. shared-artifact versus per-join reconstruction is benchmarked on whole cost before the shared
   mechanism is declared a permanent performance winner.
