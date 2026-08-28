# R2C World-Projection Semantic Frontier — Minecraft Java 26.2

Status: **selected profile frozen; exact world-projection source route not yet admitted**  
Target: Minecraft **26.2**, protocol **776**, data version **4903**  
Parent milestone: R2C — Helve-owned world projection

> This file freezes the finite R2C profile and evidence boundary. It intentionally does **not**
> assert packet identities, field order, packing law, light-mask semantics, or publication ordering
> until the corresponding official-source review has been materialized and independently admitted.

## Selected profile

R2C's first admitted world-projection route is deliberately narrow:

- the existing fresh/offline/default R2B player profile;
- one pregenerated, source-compatible world imported into Helve semantic state;
- initial observation around the configured spawn only;
- no world generation requirement;
- no movement-driven interest frontier requirement;
- no gameplay/block-interaction requirement;
- no captured world/chunk/light Play publication in the production route;
- block entities are admitted only for the exact selected qualification-world cases proven by the
  R2C source gate; unsupported required block-entity state fails closed;
- the exact dimension set is the set actually present in the pinned qualification world and required
  by the selected initial spawn route. R2C does not infer generic custom-dimension semantics from
  the standard dimensions.

The stock 26.2 client remains the black-box integration oracle. Official source evidence and
source-free VAR/SEM records remain semantic authority.

## Frozen architectural boundaries

### World ownership

The imported world becomes Helve-owned semantic state. Anvil/NBT and target packet representations
are boundary formats only and may not become live world truth.

### Chunk identity

A resident chunk is identified by semantic `ChunkPos` plus a live `ChunkGeneration`; real semantic
mutation advances the generation-local `ChunkRevision`. Deferred/cached world-projection work must
bind to the exact chunk identity/revision required by its freshness contract.

### Dimension identity

HOT world code uses compact process-local `DimensionId` / `DimensionTypeId` identities and a
pre-resolved target-neutral dimension runtime profile. Resource locations and target registry IDs
remain cold/protocol-boundary identities.

### Projection ownership

`helve-target-26-2` owns all 26.2 wire law. World crates own no packet IDs, VarInt/bit-packing rules,
NBT protocol payloads, compression framing, or client publication ordering.

### Publication

R2C continues the exact bounded connection driver and live R2B session through the existing
`WorldProjection` handoff. There is no second egress queue, socket owner, read scratch, or packet
object graph.

## Source-review groups

The exact selected route must close, at minimum, the following groups before production target code
may rely on them:

1. **R2C-WORLD-ENTRY** — the selected initial world-observation entry conditions and observable
   ordering after R2B's `WorldProjection` handoff;
2. **R2C-CHUNK-SPAN** — chunk coordinates, vertical section span/count and any selected batch/center
   state needed by the client;
3. **R2C-BLOCK-SECTIONS** — exact block-state section serialization/palette/packing semantics;
4. **R2C-BIOMES** — exact biome section serialization/palette/packing semantics;
5. **R2C-HEIGHTMAPS** — exact selected heightmap identities, values and wire representation;
6. **R2C-LIGHT** — exact sky/block light masks, empty/full/array semantics and ordering;
7. **R2C-BLOCK-ENTITIES** — exact selected block-entity publication path required by the pinned
   qualification world;
8. **R2C-PACING** — any 26.2 chunk batching, acknowledgement, pacing or completion semantics that
   are material on the selected route;
9. **R2C-PACKET-IDS** — exact clientbound packet identities generated from the pinned target law.

Unknown delegates discovered inside one group are added to that group's bounded dossier. Discovery
must not silently broaden R2C into movement, interaction, world generation or general persistence.

## Admission pipeline

```text
pinned official source/runtime
        ↓
bounded exact source-review dossiers
        ↓
human-reviewed closure + hazard/delegate accounting
        ↓
source-free VAR/SEM materialization
        ↓
independent Atlas source gate
        ↓
admitted R2C target contract
```

A capture may confirm exact output after source-backed semantic closure. Capture bytes/order never
become semantic authority and must never be replayed by the admitted R2C production route.

## Fail-closed rules

Until a group is admitted:

- target code may contain reference/scaffolding types that do not assert unreviewed wire facts;
- production code must not invent packet IDs, field order, packing widths, mask meaning or omitted
  branches;
- unsupported required world state is an explicit error, not an implicit default;
- a partial codec cannot call itself generic.

## Exit condition for R2C.0

R2C.0 is complete when:

- this selected profile is accepted as the finite first R2C scope;
- each unresolved semantic fact belongs to one named source-review group;
- world/import/projection/publication ownership boundaries are fixed;
- no R2C implementation needs to infer semantics from the R1X capture.

R2C.1 then closes the source groups and promotes only independently admitted facts into
`helve-target-26-2` generated/static target law.
