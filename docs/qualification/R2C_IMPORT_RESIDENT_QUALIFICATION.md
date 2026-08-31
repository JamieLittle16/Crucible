# R2C Import → Resident Qualification

Status: **implemented hosted-diagnostic qualification; controlled target-hardware baseline remains pending**  
Target: **Minecraft: Java Edition 26.2 / DataVersion 4903**  
Parent importer qualification: [`R2C_PREGENERATED_WORLD_IMPORT_QUALIFICATION.md`](R2C_PREGENERATED_WORLD_IMPORT_QUALIFICATION.md)  
Architecture: [`../architecture/R2C_WORLD_PROJECTION_IMPLEMENTATION.md`](../architecture/R2C_WORLD_PROJECTION_IMPLEMENTATION.md)

## Purpose

R2C needs evidence for the complete cold block-state path, not only isolated parser, section or
residency components. This qualification measures and validates the boundary:

```text
preloaded Anvil region bytes
        ↓
validated RegionView
        ↓
qualified DeflateChunkPayloadDecoder
        ↓
bounded schema-directed NBT decode
        ↓
exact 26.2 persisted-state resolution
        ↓
final qualification section construction
        ↓
install_imported_chunk
        ↓
DimensionInstance residency
```

Filesystem reads happen before the measured CPU path. Region validation remains part of the measured
engine work. This distinction prevents hosted filesystem noise from being mislabeled as parser or
world-install cost.

## Representation boundary

`helve-world-load-qualification` owns a transparent direct 4096-cell section used only for this
qualification. It is not a production R2C.2S section-policy decision.

The qualification builder records exactly:

- uniform final sections;
- non-uniform final sections;
- semantic cells copied from reusable importer scratch for non-uniform sections;
- all semantic cells written into retained direct-section storage.

This makes final-representation materialization visible instead of hiding it behind an abstract
"import" number. Production remains free to select another section mechanism only through the
section qualification programme.

## Measured stages

`r2c_import_resident_bench` records separate distributions for:

1. dimension-directory setup;
2. region validation;
3. decompression + NBT/schema + state resolution + final-section construction;
4. resident installation;
5. whole-chunk import-to-resident handoff;
6. the complete world round.

The artifact currently reports p50, p95, p99 and maximum latency. Hosted values are diagnostic only;
no numerical threshold or mechanism winner may be promoted from GitHub-hosted timing.

The harness also records:

- region-file count and bytes;
- occupied chunk count;
- compressed payload bytes;
- exact generated state-data input/generation digests;
- importer palette/packed-word/state scratch capacities before and after measurement;
- retained decompression output capacity before and after measurement;
- whether either scratch surface grows after warm-up;
- synthesized missing-air-section count;
- final-section construction accounting;
- resident chunk count and a deterministic structural checksum;
- machine/toolchain provenance.

## Genuine official-save gate

The existing `Section Corpus Probe` is intentionally reused so Helve does not pay for a second official
world-generation job. It generates the pinned deterministic Minecraft 26.2 save, then uses the same
save for three independent purposes:

1. transparent Python extraction of every stored block-bearing section;
2. production Rust raw-Anvil differential comparison at every semantic cell;
3. the production import → resident qualification harness.

Minecraft 26.2 standard dimensions live below `dimensions/<namespace>/<dimension>`. The hosted resident
harness therefore receives the exact overworld dimension root:

```text
<save>/dimensions/minecraft/overworld
```

and reads its `region/` directory. Legacy root-level overworld region layout is not silently accepted
by this target-specific gate.

The workflow requires the resident harness to agree with the independently normalized corpus on the
number of block-bearing sections materialized in every measured round. It additionally requires:

- exact target/profile identity;
- exact generated state-data digests;
- the benchmark to observe the actual number of overworld region files;
- at least one resident chunk;
- no importer section-scratch growth after warm-up;
- no decompression-output growth after warm-up;
- dense-copy accounting of exactly 4096 cells per dense section;
- retained-cell accounting of exactly 4096 cells per materialized section;
- one whole-chunk timing sample per occupied chunk per measured round;
- one whole-round timing sample per measured round;
- `mode == hosted-diagnostic`;
- `production_decision_eligible == false`.

The independent Python ↔ Rust differential remains the full-cell semantic oracle. The resident
qualification does not duplicate a second 52-million-cell scan inside the timed harness; #206's
installation seam moves already-constructed sections into the contiguous resident lattice, and its
lifecycle tests independently cover load/unload/reload and stale-generation behavior.

## Warm-up and allocation discipline

One mechanism set is retained across warm-up and measurement:

- one deflate decoder and retained output buffer;
- one target 26.2 block-state resolver;
- one final-section builder;
- one reusable block-section decode scratch object.

Each measured round creates a fresh `DimensionInstance` and loads the complete selected world. The
resident directory is pre-sized to the known occupied chunk count so the benchmark measures the
intended cold import/install mechanism rather than avoidable directory growth noise.

Scratch high-water marks are captured after warm-up and after all measured rounds. Growth during the
measured phase is an explicit gate failure in the official-save workflow.

## Evidence boundary

A green hosted run establishes that:

- the complete block-state import → resident path executes on a genuine pinned 26.2 save;
- every independently observed block-bearing section is materialized in each measured round;
- importer/decompressor scratch reaches steady state before measurement;
- resident installation accepts every occupied chunk under the selected Overworld profile;
- the harness emits stable provenance and stage-separated timing evidence.

It does **not** establish:

- a production section-storage winner;
- a decompression scratch-policy winner;
- target-hardware throughput or latency thresholds;
- biome, heightmap, light or block-entity import parity;
- arbitrary-world support;
- client chunk/light projection readiness.

## Controlled target-hardware follow-up

Before a production performance decision, run the same workload on controlled target hardware with
single-CPU affinity where appropriate and collect multiple independent processes. Decision evidence
must include whole-transaction latency/tails, memory/RSS, retained scratch, allocation/copy evidence,
and direction stability. A candidate only wins if it improves the complete transaction without adding
HOT-path architecture tax elsewhere.

## Requalification triggers

Re-run this qualification when any of the following changes materially:

- Anvil/NBT/decompression/state-resolution logic;
- importer scratch policy;
- final section-construction mechanism;
- `install_imported_chunk` or `DimensionInstance` admission/lifecycle behavior;
- selected dimension lattice;
- generated state identity/facts;
- official-save generator/corpus identity;
- benchmark workload, timing boundary or evidence schema;
- target hardware/toolchain used for a decision-grade baseline.
