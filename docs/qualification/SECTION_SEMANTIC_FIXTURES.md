# Section semantic fixtures — Minecraft Java 26.2

Status: **M0.3C qualification substrate**  
Parent: #18

The committed section fixture records **semantic observations**, never Mojang live-storage objects.
Its target/provenance header is bound to Minecraft 26.2, protocol 776, data version 4903, the pinned
source archive/source qualification, the official server runtime SHA-256, and the generated target
state-data generation digest.

## Fixture principles

- Block fixtures name a target semantic fact signature (`non-air`, counted-fluid, random-block,
  random-fluid bits), not a Java `BlockState` object or a palette slot.
- Qualification deterministically selects the lowest qualified `BlockStateId` carrying that exact
  signature and checks the direct oracle plus every admitted live block candidate.
- Expected counts/gates are written into the fixture rather than inferred from candidate state.
- Zero→one and one→zero count/gate boundaries are explicit fixture cases.
- Biome fixtures qualify the exact 4×4×4 lattice, replacement semantics, storage linearization, and
  the source-backed x-major → y → z resolver call order without inventing a biome palette mechanism.
- Fixture provenance drift fails closed.

The initial committed document is:

`vanilla/fixtures/section/26.2-semantic-fixtures.txt`

## Command

The intended interface for this slice is:

```text
cargo xtask qualify section --vanilla vanilla/fixtures/section/26.2-semantic-fixtures.txt
```

It writes `target/crucible-qualification/section/vanilla-fixture.json` and links evidence to
`SEM-WORLD-SECTION-003`, `004`, `005`–`010`, `012`, `015`, and `016`.

The command currently consumes a normalized semantic fixture. A later #18 slice may add an official
runtime extractor that produces the same format from the local pinned official artifact; the
qualification semantics and evidence schema should not change merely because the producer changes.

Wire/decode rules `SEM-WORLD-SECTION-017` and `018` remain intentionally outside this fixture until
the packet/decode adapter exists. Live CPU storage must not grow packet ownership merely to satisfy a
qualification milestone early.
