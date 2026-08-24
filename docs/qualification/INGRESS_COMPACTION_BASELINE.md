# Ingress Compaction Baseline

Issue: #132  
Parent: #78

## Question

Does Crucible's admitted compacting-`Vec<u8>` ingress mechanism move enough live data under realistic fragmentation and fairness regimes to justify evaluating a more complicated ring/slab/pool representation?

This record defines the measurement gate. It does **not** authorize a production representation change.

## Existing mechanism

`IngressBuffer` retains received bytes in one bounded `Vec<u8>` plus a logical start offset. Complete frame bodies and payloads are borrowed directly from the active suffix. Consuming a frame advances only the start offset. Before an append, active bytes are compacted with `copy_within` only when:

- the physical append would cross the configured ingress bound; or
- at least half of the current physical bytes have been consumed.

When every physical byte is consumed, the vector is cleared and the start offset returns to zero.

## Harness

`ingress_compaction_bench` executes the **real** `IngressBuffer` and independently shadows only the public algorithm's physical-length/start arithmetic to count:

- append calls;
- modeled compaction count;
- modeled active bytes moved;
- peak logical buffered bytes;
- peak physical vector length (not allocator capacity).

No counter fields are added to the production buffer.

Every case uses a pre-generated framed byte stream. The semantic checksum, decoded frame count, stream byte count and payload byte count must remain identical across warm-up and measured rounds.

## Fragmentation/fairness matrix

The initial matrix includes:

- 1-byte, 3-byte, 7-byte and 17-byte hostile fragments;
- 1460-byte MTU-like fragments;
- 64 KiB coalesced fragments;
- unlimited drain after each logical turn;
- one-action and four-action budgets that leave complete frames buffered across turns;
- a separate near-maximum-frame/MTU case.

Small/mixed streams combine tiny status-like bodies, medium bodies and multi-kilobyte bodies. The near-maximum case uses bodies close to the admitted 65,536-byte frame-body bound.

## Timing

The release harness retains raw rounds plus p50/p95/p99/max wall time and the standard Crucible machine/microarchitecture provenance.

Hosted CI timing is diagnostic only. Controlled target-hardware runs must follow `MICROARCHITECTURE_PERFORMANCE_QUALIFICATION.md` before any mechanism decision.

## Interpretation

A high compaction-byte count does not by itself justify a ring buffer. A candidate must beat the existing mechanism on whole cost while preserving exact framing semantics. In particular, a ring may reduce copies while increasing:

- split-slice handling;
- indexing and branches;
- cache footprint;
- retained memory;
- parser complexity;
- syscall/read integration complexity.

Therefore any later candidate must compare allocations, copies, retained memory, p50/p95/p99 timing, throughput and relevant PMU/cache evidence under the same streams.

## Default outcome

If the baseline does not expose a material cost on target hardware, Crucible keeps the compacting `Vec<u8>`. Simpler code is the preferred production mechanism when more complex machinery does not buy measured performance.
