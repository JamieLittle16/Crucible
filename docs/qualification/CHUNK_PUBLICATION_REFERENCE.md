# Chunk Publication Reference

Status: **REFERENCE / QUALIFICATION BOUNDARY**  
Tracker: #126

## Purpose

`LiveChunkCore` is live mutable world state and deliberately does not implement `Clone`. Networking,
persistence, compression, observation, and other asynchronous preparation must not retain mutable
world references or learn the selected section backing representation.

The first publication mechanism is therefore an explicit semantic projection:

```text
live chunk authority
      ↓
BlockSection semantic reads
      ↓
PublishedChunk<S>
      ↓
immutable background preparation
      ↓
install/send only if ChunkPos + ChunkStamp are still current
```

This reference mechanism is intentionally simple. It establishes the correctness and freshness
oracle against which later publication mechanisms compete.

## Published identity

A publication captures one exact:

- `ChunkPos`;
- `ChunkGeneration` + `ChunkRevision` as `ChunkStamp`;
- minimum logical section Y;
- vertical `SectionMasks`;
- contiguous section-major semantic state image.

Inside each section the state array uses the already-admitted observable cell linearization:

```text
(y << 8) | (z << 4) | x
```

The section count is derived from the state-image length rather than stored redundantly.

## Freshness

`ChunkStamp` is meaningful only for an already-identified chunk. A complete publication is current
for a live chunk iff both are equal:

```text
publication.position == live.position
publication.stamp    == live.stamp
```

Therefore:

- same-state writes do not invalidate a publication;
- a real semantic mutation invalidates older publications;
- a new live generation invalidates all publications from the old incarnation even when the
  revision number happens to match;
- a publication from another chunk position cannot be installed accidentally.

This is the same fail-closed rule future encoded chunk packets, persistence snapshots, lighting
results, and other deferred work must obey at installation.

## Reference cost model

`publish_semantic_image()`:

1. allocates one output state array;
2. scans every logical section through the statically dispatched `BlockSection::get` contract;
3. writes each semantic state exactly once into canonical section-major order;
4. captures identity/masks from the same immutable live borrow.

It performs no mutation and adds no per-section revision writes, atomics, locks, `Arc` traffic, or
other work to the ordinary block-mutation HOT path.

## Deliberate non-decisions

This reference does **not** select:

- full-copy publication as the final networking mechanism;
- per-section revisions;
- immutable pages;
- COW;
- `Arc` per section/chunk;
- encoded chunk packet caching;
- compression caching;
- custom allocation;
- unsafe bulk copying.

Likely optimized candidates include reusing unchanged published sections or revision-keyed encoded
components, but each changes memory/coherence/HOT mutation cost and must therefore be measured.

## Permanent qualification

`crucible-world-chunk` tests require:

- every published semantic cell equals live reads;
- canonical section/cell ordering;
- negative/minimum section coordinates;
- exact position/stamp/mask capture;
- same-state freshness preservation;
- mutation/generation/position stale rejection;
- old publication immutability after live mutation;
- identical publication across distinct `BlockSection` implementations;
- long deterministic mutation/publication traces with full-image comparison.

The world chunk crate retains no networking, NBT, persistence, async-runtime, or socket dependency.

## Performance qualification follow-up

Before replacing the reference, measure at least:

- full-copy publication latency and tail;
- publication frequency under realistic visibility/persistence traces;
- bytes copied/allocated;
- retained/RSS cost;
- cache effects;
- stale-result frequency;
- any additional HOT mutation bookkeeping required by a candidate.

An optimized mechanism must preserve the exact immutable semantic projection and freshness law and
show a material whole-cost win beyond noise. Moving work into every mutation to make occasional
publication cheaper is not automatically an optimization.
