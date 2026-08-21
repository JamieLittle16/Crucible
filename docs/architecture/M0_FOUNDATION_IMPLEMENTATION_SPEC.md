# M0 — Foundation and World Kernel Qualification

M0 proves the substrate rather than producing a playable server.

## M0.0 Repository

- Rust workspace and pinned toolchain.
- formatting/lint baseline.
- architecture guard.
- deterministic test/benchmark harness.
- CI.

## M0.1 Component/profile resolver

- package manifests;
- capability IDs and versions;
- provider cardinality/conflicts;
- trust/fidelity policy;
- static profile resolution;
- exact `Crucible.lock`;
- generated composition wiring;
- benchmark against direct hand wiring to prove no meaningful HOT dispatch tax.

## M0.2 Vanilla Atlas MVP

- source pin and hashes;
- Java symbol/method index;
- normalized fingerprints;
- world-kernel dependency tracking;
- VAR resolution/staleness;
- populate initial world/section VARs from the official target source.

## M0.3 Generated target data

- block-state universe;
- narrowest safe `BlockStateId` with capacity assertion;
- initial SoA hot metadata;
- provenance manifest.

## M0.4–M0.5 Section laboratory

Reference direct section plus candidate Uniform, packed, stable-local-index, direct, and adaptive representations where justified. Differential/property tests and entropy/read/mutation/memory benchmarks are mandatory.

## M0.6 LiveChunkCore

Only section slots, vertical masks/summaries, revision metadata, and identity/lifecycle needed by experiments. No packet/NBT/worldgen/entity baggage.

## M0.7 Mutation facts

Authoritative block mutation emits old/new facts and incrementally updates summaries. A full recomputation oracle validates every maintained invariant.

## M0.8 Bulk world views

Compare conventional global point lookup with a resolved `DomainChunkWindow`/volume path using collision- and pathfinding-shaped traces.

## M0.9 Ownership simulator

Singular mutation tokens, migration, typed cross-domain effects, stage-stable reads, randomized legal schedules, and 1/2/4/many-worker runs.

## M0 exit

M0 passes only when correctness, schedule invariance, reproducible benchmarks, source tracking, composition, and architecture guards are operational. Attractive hypotheses that lose are rejected rather than preserved out of pride.
