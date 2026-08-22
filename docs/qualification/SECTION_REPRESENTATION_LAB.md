# Section Representation Laboratory

Status: M0.3 active. No production default has been selected.

## Candidate direction

The first deliberately non-Mojang adaptive candidate is:

`Uniform → Local4 → Local8 → Direct`

- `Uniform` owns no 4096-cell backing.
- `Local4` stores 4096 stable local IDs in 2048 bytes and at most 16 palette values.
- `Local8` stores one byte per cell and at most 256 stable palette values.
- `Direct` stores semantic state values directly.
- Palette entries are not removed during ordinary mutation. This makes membership queries conservative and prevents tiny mutations from causing representation-wide compaction.
- Promotion is one-way in this first candidate. Thermal/demotion policy is deferred until traces demonstrate enough benefit to justify hysteresis and extra transitions.

The current backing uses boxes to keep the adaptive enum small; it must later be compared with owner/domain-local size-class arenas. The candidate deliberately avoids embedding a direct 4096-cell array in the enum, which would make every uniform section reserve the largest variant's size.

## Qualification matrix

Correctness:
- differential traces against `DirectBlockSection`;
- exact summary equality after every mutation;
- full 4096-cell equality at trace boundaries;
- conservative-membership no-false-negative checks;
- transition-boundary tests at 2, 17, and 257 observed semantic values.

Performance experiments must separate:
- uniform reads;
- low-entropy reads and writes;
- repeated writes to already-known local values;
- insertion of new local values;
- `Uniform→Local4`, `Local4→Local8`, and `Local8→Direct` transition spikes;
- high-entropy direct access;
- palette membership queries;
- clone/publication-shaped copies;
- resident memory over realistic loaded-section distributions.

## Important unresolved experiment

`Local8` currently uses linear palette lookup as a minimal baseline. That may be too expensive for hot mutation at large palette cardinality. Competing lookup structures should be added as independent candidates rather than silently complicating the baseline. The winner must be chosen by total CPU+memory measurements.

## Target-data gate

Synthetic IDs are sufficient for representation algorithm equivalence tests but **not** for production qualification. Before selection, M0.3 must produce a pinned Minecraft 26.2 state-universe/fact artifact and prove the narrowest safe packed direct width. The public semantic state identity must remain independent of the physical width chosen by a representation.
