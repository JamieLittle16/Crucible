# R2B Preparation Scratch Reservation Sweep

This qualification investigates `OPT-R2B-002`: whether the per-join `PacketWriter` used by replay-free Minecraft 26.2 R2B preparation should reserve scratch capacity before the first dynamic packet is encoded.

No production reservation is selected by this document or by a single hosted CI run. The existing semantic packet-body limit remains 4 KiB regardless of reservation.

## Question

The current runtime creates `PacketWriter::new(4096)`. Its `Vec` begins with zero retained capacity and grows as dynamic bootstrap bodies are encoded. `PacketWriter::with_capacity(4096, N)` can move some or all of that allocator work to writer construction while preserving the same semantic bound and the same reused-scratch ownership model.

The relevant trade-off is not allocation count alone. A useful reservation must reduce preparation cost and/or tail jitter without reserving substantially more transient memory than the workload justifies.

## Candidate sweep

The qualification compares the existing zero-reserve path against these initial reservations:

- 64 bytes;
- 128 bytes;
- 256 bytes;
- 512 bytes;
- 1024 bytes;
- 2048 bytes;
- 4096 bytes.

Each candidate is compared independently with the zero-reserve baseline using balanced ABBA/BAAB blocks so linear runner drift is bracketed rather than mistaken for a candidate effect.

## Workloads

Two semantic workloads are required:

1. `fresh-clear`: the selected fresh route with no existing-player initialization, no optional server-data packet and clear weather.
2. `populated-raining`: 64 existing offline player-info entries, optional server data and the admitted raining branch. This intentionally exercises a much larger dynamic player-info body while remaining below the unchanged 4 KiB packet-body limit.

A reservation that wins only on the selected tiny fixture is not sufficient evidence for production.

## Metrics

For every workload/candidate pair the benchmark records:

- p50, p95, p99 and p99.9 sample service time;
- maximum and arithmetic mean;
- median absolute deviation (MAD) and relative MAD;
- paired candidate/baseline block-ratio distribution;
- epoch-ratio distribution and epoch win rate;
- exact semantic checksum;
- largest encoded body observed by the semantic gate;
- reservation bytes.

Hosted CI runs are diagnostic. The acceptance decision requires stable direction across independent pinned runs and no material tail/jitter regression. A small p50 win does not justify a candidate whose p99/p99.9 or variance is worse.

## Architecture gate

An accepted production change may only replace the writer constructor with the already-existing bounded `PacketWriter::with_capacity` API. It must not add:

- a pool;
- cross-connection scratch ownership;
- synchronization;
- a second queue or buffer owner;
- a larger semantic packet limit;
- fixture-derived correctness assumptions.

If no reservation shows a repeatable, tail-safe win across the workload set, the current zero-reserve constructor remains the production choice and the experiment is recorded as rejected.

## Command

```text
cargo bench --locked --package crucible-target-26-2 --bench r2b_prepare_scratch -- --full --output target/r2b-prepare-scratch.json
```
