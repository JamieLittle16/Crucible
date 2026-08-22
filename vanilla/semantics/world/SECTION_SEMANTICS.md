# World Section Semantic Contract — Minecraft Java 26.2

Status: **source-backed M0 contract**  
Target: Minecraft **26.2**, protocol **776**, world/data version **4903**  
Source fingerprint algorithm: `java-token-v2-literal-sensitive`

This document freezes the semantic obligations extracted from the official target source. It does **not** freeze Mojang's `LevelChunkSection → PalettedContainer → Palette → BitStorage` implementation.

## Exact obligations

- **SEM-WORLD-SECTION-001 — block domain.** A live section exposes exactly 4096 block-state cells arranged as 16×16×16 local coordinates.
- **SEM-WORLD-SECTION-002 — block linearization.** The target section container maps local `(x,y,z)` to `(y << 8) | (z << 4) | x`.
- **SEM-WORLD-SECTION-003 — biome domain.** A section exposes exactly 64 biome cells arranged as a 4×4×4 quart lattice.
- **SEM-WORLD-SECTION-004 — biome linearization.** Biome local `(x,y,z)` maps to `(y << 4) | (z << 2) | x`.
- **SEM-WORLD-SECTION-005 — replace semantics.** A block replacement returns the exact previous semantic state and leaves the requested state at that cell.
- **SEM-WORLD-SECTION-006 — non-air summary.** `non_air_count` is exactly the number of cells whose target `BlockState.isAir()` is false; `hasOnlyAir()` is equivalent to that count being zero.
- **SEM-WORLD-SECTION-007 — fluid summary.** `fluid_count` is exactly the number of non-air cells whose target `BlockState.getFluidState().isEmpty()` is false; `hasFluid()` is equivalent to that count being positive.
- **SEM-WORLD-SECTION-008 — random-block gate.** The random-block summary is true exactly when at least one non-air state is target-random-block-ticking.
- **SEM-WORLD-SECTION-009 — random-fluid gate.** The random-fluid summary is true exactly when at least one counted fluid state is target-random-fluid-ticking.
- **SEM-WORLD-SECTION-010 — combined random gate.** Section random-tick eligibility is the logical OR of the block and fluid random-tick gates.
- **SEM-WORLD-SECTION-011 — RNG consequence.** A false negative in the combined random gate is a semantic failure: `ServerLevel.tickChunk` skips random-position generation for that section, changing RNG consumption and future simulation.
- **SEM-WORLD-SECTION-012 — incremental/recompute equivalence.** Summary state after any legal mutation sequence must equal a full recomputation from the 4096 semantic cells.
- **SEM-WORLD-SECTION-013 — conservative membership.** `maybeHas(P) == false` guarantees that no current semantic state satisfies `P`; `true` may be a false positive. Exact palette compaction is not required.
- **SEM-WORLD-SECTION-014 — copy independence.** A copied live section initially has equal semantic content and summaries but subsequent mutation of either copy must not alter the other.
- **SEM-WORLD-SECTION-015 — biome replacement.** Biome replacement and lookup operate on the exact 64-cell semantic lattice independently of block-state storage representation.
- **SEM-WORLD-SECTION-016 — biome noise fill order.** The target 26.2 source invokes the biome resolver in x-major, then y, then z loop order over the local 4×4×4 lattice. Until black-box qualification proves order unobservable for supported resolvers, the reference preserves it.
- **SEM-WORLD-SECTION-017 — section wire prefix.** The 26.2 section network representation writes signed-short non-air count, then signed-short fluid count, followed by block-state container data and biome container data. Live storage need not resemble this representation.
- **SEM-WORLD-SECTION-018 — read coherence.** After target wire decode, block/biome semantic content and externally supplied count fields form the decoded section image. Simulation installation must not expose a partially decoded mixture.

## Explicitly non-frozen Mojang mechanisms

The following are implementation evidence, not Crucible architecture:

- `PalettedContainer` object ownership;
- `volatile Data<T>` publication;
- per-container `ThreadingDetector`;
- `ZeroBitStorage`/`SimpleBitStorage` as live CPU storage;
- block-state palette widths 0 → 4 → 5 → 6 → 7 → 8 → global;
- biome palette widths 0 → 1 → 2 → 3 → global;
- whole-container O(4096) repacking on palette growth;
- dead palette entries remaining until later packing;
- Java palette/object allocation strategy.

Crucible production storage may use different representations, promotion thresholds, stable local IDs, arenas, or direct state IDs. Acceptance depends on semantic equivalence and measured total cost.

## Required evidence

Production section implementations must pass all of:

1. deterministic differential operation traces against `crucible-world-reference`;
2. recomputation-oracle checks for every exact summary invariant;
3. target-version vanilla fixtures for external projection and RNG-sensitive gates;
4. transition-spike and steady-state CPU benchmarks;
5. resident-memory measurements over realistic section entropy distributions;
6. no mandatory dynamic-dispatch, allocation, lock, or global lookup on ordinary owner-local get/set paths.
