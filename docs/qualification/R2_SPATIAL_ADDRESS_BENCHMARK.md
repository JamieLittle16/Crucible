# R2 Spatial Address Primitive Qualification

**Status:** diagnostic mechanism qualification for the R2 spatial substrate  
**Parent:** `docs/architecture/R2_R3_PERFORMANCE_SEARCH_PLAN.md`  
**Decision register:** D1/D2 spatial region-cell support; world-routing HOT-path elimination  
**Production-selection authority:** none; hosted timing is diagnostic only

## Question

The first R2 implementation slice introduces two resolved arithmetic mechanisms before the region
container/scheduler exists:

1. `VerticalSectionLattice` resolves one validated contiguous vertical block range at construction,
   then maps block Y to section slot/local Y with range checks + subtraction/bit operations;
2. `RegionCellLayout<SHIFT>` maps exact chunk coordinates to a power-of-two coarse region cell and
   dense local slot with shift/mask arithmetic.

The qualification question is deliberately narrow:

> Do the resolved mechanisms reproduce the reference Euclidean coordinate semantics exactly, and
> what arithmetic cost do they have relative to the transparent `div_euclid`/`rem_euclid`
> baselines?

This benchmark does **not** choose a region-cell size, region container, scheduler, chunk directory,
or production world layout.

## Semantic gate

Timing is forbidden until complete prepared traces agree.

### Vertical reference

For every test Y:

```text
section_y = y.div_euclid(16)
index = section_y - min_section_y
local_y = y.rem_euclid(16)
```

with exact in-range rejection.

### Vertical candidate

`VerticalSectionLattice::section_index_for_block_y` and `local_y_for_block_y` must return the exact
same `(index, local_y)` result.

### Region reference

For the selected diagnostic `SHIFT=3`/8x8 cell arithmetic:

```text
cell = chunk.div_euclid(8)
local = chunk.rem_euclid(8)
slot = local_z * 8 + local_x
```

### Region candidate

`RegionCellLayout<3>::address` must produce exactly the same cell/local/slot identity.

The library tests separately cover multiple shifts, negative coordinates, integer extremes,
round-tripping and overflow rejection. The benchmark trace is additional differential evidence, not
its replacement.

## Measurement

The harness is:

```bash
cargo run --release --locked \
  --package crucible-world-access-qualification \
  --bin spatial_address_bench -- \
  --full \
  --output qualification-results/world-access/spatial-address.json
```

It records machine/toolchain provenance through `crucible-benchmark-support`, alternates reference
and candidate order during warm-up/measured rounds, retains raw nanosecond samples and binds both
paths to the same semantic checksum.

For a controlled decision-bearing experiment, run under the normal performance qualification
standard (including CPU affinity and repeated controlled runs). This primitive benchmark remains
insufficient to choose D1 region-cell granularity because real D1 evidence must include cell
occupancy, merge/split behavior, memory, locality and region workload distribution.

## Interpretation

A faster shift/mask primitive is useful only if the surrounding architecture lets HOT work stay in
resolved coordinates. The larger architectural win is therefore:

```text
resolve sparse/global identity once
        ↓
carry direct/local handle
        ↓
perform repeated dense local work
```

rather than repeatedly invoking a general world directory.

Likewise, the vertical candidate matters because `LiveChunkCore` now retains one validated lattice;
its ordinary vertical access does not need to rediscover the signed section relation for every
operation.

## CI

Normal CI compiles the harness through `cargo check --workspace --all-targets` and strict Clippy,
then runs `spatial_address_bench --smoke` in release mode. The smoke gate rejects any artifact that
does not report exact semantic equivalence for both vertical and region addressing, the expected
schema/mode/diagnostic marker, non-zero semantic checksums, and one positive raw timing sample per
measured round for both reference and candidate paths.

Hosted timing remains diagnostic and cannot freeze a production mechanism or select D1 region-cell
granularity.
