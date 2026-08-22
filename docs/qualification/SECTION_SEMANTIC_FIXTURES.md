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

## Qualification commands

The source-backed semantic fixture path is:

```text
cargo xtask qualify section \
  --vanilla vanilla/fixtures/section/26.2-semantic-fixtures.txt
```

It executes the committed fixture through the independent direct oracle and all admitted live block
candidates, executes the biome semantic cases, and writes commit-bound evidence to:

`target/crucible-qualification/section/vanilla-fixture.json`

Hosted CI executes this command directly in addition to the deterministic quick trace qualification.

To independently bind the block fixture signatures and expected count/gate outcomes to a raw dataset
captured by the pinned official runtime reflection probe, add:

```text
cargo xtask qualify section \
  --vanilla vanilla/fixtures/section/26.2-semantic-fixtures.txt \
  --runtime-data .crucible/vanilla/26.2-block-states.raw.json
```

The runtime verifier is `tools/section_runtime_fixture.py`. It fails closed on target identity,
official-server SHA-256, probe identity, dense global state IDs, block-state fact invariants, missing
fact signatures, or fixture expectations that do not follow from the official runtime facts. Its
evidence is written to:

`target/crucible-qualification/section/runtime-facts-fixture.json`

This creates an intentionally independent chain:

```text
pinned official source / SEM fixture
              ↓
committed semantic fixture
       ↙                ↘
Rust direct oracle       official runtime state-fact dataset
       ↓                          ↓
live candidates          independent Python verifier
       ↘                ↙
       qualification evidence
```

## Evidence boundary

The `--runtime-data` path is **runtime-bound state-fact evidence**, not yet a claim that Crucible has
black-box executed Mojang `LevelChunkSection` for every operation or reproduced its packet bytes.
That stronger runtime/serialization oracle belongs with the packet/decode adapter and
`SEM-WORLD-SECTION-017` / `018`.

The distinction is deliberate: live CPU storage must not acquire packet ownership or Java-shaped
container architecture merely to satisfy a qualification milestone early. The current fixture layer
qualifies `SEM-WORLD-SECTION-003`, `004`, `005`–`010`, `012`, `015`, and `016` at the semantic boundary
that already exists.