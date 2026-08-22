# Vanilla Algorithm Review — LevelChunkSection / PalettedContainer

Target: Minecraft Java 26.2. This review records what the target implementation demonstrates and separates those facts from Crucible mechanism choices.

## `LevelChunkSection`

The target owns four `short` counters: non-empty blocks, fluids, random-ticking blocks, and random-ticking fluids. `setBlockState` removes the previous state's contributions and adds the new state's contributions. `recalcBlockCounts` independently derives the same four quantities through `PalettedContainer.count`.

Fluid and random-fluid contributions are nested under the non-air branch in both mutation and recomputation. Crucible therefore defines generated section facts using the same counted-domain rather than assuming arbitrary `FluidState` queries are interchangeable.

`ServerLevel.tickChunk` checks `section.isRandomlyTicking()` before generating random positions for the section. The gate therefore controls RNG consumption, not merely CPU work.

`maybeHas` delegates to palette membership. Because target palettes do not remove an entry merely because its final cell was replaced, `true` can be stale while `false` proves absence. This is a useful semantic asymmetry: Crucible may implement a different conservative summary, but it may not introduce false negatives.

`write` emits non-empty count, fluid count, block states, then biomes. `getSerializedSize` includes a four-byte counter prefix. The live section representation is not constrained to this layout.

## Palette/container mechanism observations

For block states, `Strategy.createForBlockStates` maps requested bit counts 1–4 to a four-bit linear palette, 5–8 to hash-map palettes, and larger requests to the global palette. A uniform container uses zero-bit storage. `PalettedContainer.onResize` allocates/reuses a target representation then copies every old logical entry into it.

That O(4096) promotion mechanism is **not** a semantic requirement. It is an explicit candidate for elimination in Crucible through stable local IDs, different promotion thresholds, or direct storage.

The target's coordinate strategy computes `(y << bitsPerAxis | z) << bitsPerAxis | x`, yielding y-z-x linearization for both 16³ blocks and 4³ biomes.

The target uses `ThreadingDetector` around checked mutation/read-write serialization paths. Crucible treats this as evidence that mutation authority matters, not as evidence that every section should carry a lock/detector in production. Owner-local authority is expected to remove that replicated hot-state cost.

## First production hypotheses

These remain experiments, not decisions:

- `Uniform(state)` should consume only a compact header and no 4096-cell backing.
- Low-entropy mutable sections should compare packed local IDs against byte-addressed stable local IDs.
- Stable local IDs may retain dead palette entries to avoid O(section) cleanup from a small mutation.
- High-entropy/hot sections should compare direct target IDs against packed/global alternatives.
- Backing storage should compare ordinary boxed allocations against owner/domain-local size-class arenas before integration into `LiveChunkCore`.
- World generation should use a separate write-optimized construction representation and finalize into live storage.

No hypothesis becomes the default without EQUIV and benchmark evidence.
