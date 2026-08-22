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

## Current qualification path

The committed fixture is compiled and executed by the `crucible-section-qualification` integration
tests under the normal workspace `cargo check`, Clippy, test, and rustfmt gates. The initial fixture
contains ten semantic cases: eight block cases executed against all four admitted live block
candidates and the direct oracle, plus two independent biome cases.

The target command remains:

```text
cargo xtask qualify section --vanilla vanilla/fixtures/section/26.2-semantic-fixtures.txt
```

That CLI wrapper and its `vanilla-fixture` evidence writer are intentionally deferred to the next
#18 slice together with the official-runtime fixture producer. Until then, do not treat `--vanilla`
as implemented merely because the normalized fixture engine is qualified.

The runtime producer will emit or verify this same semantic format from the local pinned official
artifact. The qualification semantics should not change merely because the fixture producer changes.

Wire/decode rules `SEM-WORLD-SECTION-017` and `018` remain intentionally outside this fixture until
the packet/decode adapter exists. Live CPU storage must not grow packet ownership merely to satisfy a
qualification milestone early.
