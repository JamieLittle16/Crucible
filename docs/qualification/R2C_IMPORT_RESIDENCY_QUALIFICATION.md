# R2C Import → Residency Qualification

This qualification measures and structurally audits the cold path from persisted chunk bytes to a live Helve resident chunk.

## Scope

The harness exercises the production chain:

```text
Anvil region framing
→ bounded zlib decompression
→ bounded schema-directed NBT decode
→ exact 26.2 block-state resolution
→ final semantic block sections
→ helve-world-load structural composition
→ DimensionInstance residency
→ unload
```

It deliberately uses `DirectBlockSection` as a transparent correctness/reference storage mechanism. The result therefore does **not** select the eventual production section representation and does not supersede M0.3D section-policy qualification.

## Fixture

The committed fixture is one sector-aligned in-memory Anvil region containing one zlib-compressed Minecraft 26.2 chunk at `(0, 0)`.

The decompressed NBT is 244 bytes with SHA-256:

```text
002818fa4ac587b42097b8033a4768c28134e0d3556ea235a6a5e6edfaa83323
```

It contains exactly three uniform logical sections:

- section `-1`: `minecraft:stone`;
- section `0`: `minecraft:air`;
- section `1`: `minecraft:stone`.

The loaded dimension lattice is exactly those three sections, so the semantic witness covers 12,288 block cells.

## Permanent correctness / structure gates

Before timing, the harness verifies every resident semantic cell against the expected section image and checks resident summary masks against recomputation. Every timed successful load is subsequently unloaded, and the final resident count must be zero.

The install seam is structurally required to preserve its current ownership law:

- decoded block-bearing section objects are moved into the resident column;
- no 4096-cell copy is introduced by `install_imported_chunk`;
- the final resident section vector has exact dimension-lattice capacity;
- successful runtime admission pays one authoritative sparse-directory probe;
- generation identity advances monotonically across unload/reload cycles.

The benchmark reports the install seam separately from decoding so reference-section construction cost cannot be misattributed to residency.

## Timing surfaces

Two distributions are recorded with p50/p95/p99/p99.9/max and raw samples:

1. `whole_decode_install_unload_ns`: region validation + decompression + NBT/state decode + section construction + resident installation + resolve witness + unload;
2. `install_unload_ns`: resident installation + unload from an already-decoded `ImportedStoredChunk`.

The reusable `DeflateChunkPayloadDecoder` is initialized once at the selected 64 KiB decompressed bound and retained across rounds. Its retained bytes/capacity are emitted explicitly.

## Evidence policy

GitHub-hosted timing is diagnostic only. The artifact always states:

```text
hosted_ci_is_diagnostic_only = true
performance_admitted = false
reference_section_storage_only = true
```

A production performance decision requires repeated `--full --require-single-cpu` runs on controlled target hardware, with stable toolchain/hardware identity and cross-process review. This harness establishes the workload and evidence shape; it does not set a timing threshold or declare the import mechanism production-optimal.

## Commands

Hosted smoke:

```bash
cargo run --release --locked \
  --package helve-world-load \
  --example import_residency_bench -- \
  --smoke \
  --output target/r2c-import-residency-smoke.json
```

Controlled target run:

```bash
taskset -c <cpu> cargo run --release --locked \
  --package helve-world-load \
  --example import_residency_bench -- \
  --full \
  --require-single-cpu \
  --output <fresh-evidence-path>.json
```

The benchmark records Git commit, Rust compiler identity, CPU model, CPU affinity and kernel metadata in each artifact.
