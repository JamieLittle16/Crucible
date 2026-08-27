# R2B Performance Evidence Ledger

This document records post-R2B performance changes which alter the replay-free Minecraft 26.2 Play-bootstrap path.

The purpose is not to maximize the number of micro-optimizations. It is to preserve a reproducible record of which changes were accepted, which were rejected, and why the accepted implementation remains correct and architecturally clean.

## Performance objective

Crucible does not optimize only for arithmetic mean throughput. A server which is slightly faster on average but periodically stalls join/bootstrap work is not necessarily better.

For R2B and later runtime work, performance evidence therefore has four distinct dimensions:

1. **Central service cost.** Median/p50 and aggregate throughput describe the common path.
2. **Tail latency.** p95, p99 and p99.9 expose the slow path experienced by a minority of joins. Maximum latency is recorded as a diagnostic, but is not interpreted alone because a single operating-system interruption can dominate it.
3. **Jitter / variance.** Median absolute deviation (MAD), interquartile range (IQR), p99-to-p50 and p99.9-to-p50 amplification, and means of the slowest 1% and 0.1% expose unstable service time which a mean can hide.
4. **Direction stability.** Paired measurements are divided into epochs and repeated in independent benchmark processes. A candidate whose measured direction repeatedly flips is **inconclusive**, not a performance win.

For player-facing latency paths, a central-case improvement does not excuse a material, repeatable tail regression. If a proposed change creates such a trade-off, it needs explicit higher-level evidence showing that the trade is beneficial; it is not accepted as a routine micro-optimization.

The R2B shared-artifact microbenchmark measures fixed-size **micro-batch service time**, not literal per-join latency. Timing each extremely small lookup individually would make clock-read overhead a substantial part of the measurement. Fixed-size micro-batches retain enough work per sample for useful timing while providing thousands of samples from which tail and jitter statistics can be estimated.

A later server-level arrival benchmark must additionally measure admission-to-ready latency under steady arrivals and bursts, including completion-gap jitter, queue occupancy/backpressure, p95/p99/p99.9, and maximum stalls. That is the authority for user-visible arrival smoothness; the target-local microbenchmarks isolate CPU mechanisms underneath it.

## Acceptance law

An R2B optimization is accepted only when all of the following remain true:

1. **Protocol and semantic correctness is unchanged.** Source-backed packet order, packet identity, branch law, transactional state changes, teleport acknowledgement, liveness behavior and the `WorldProjection` ownership seam remain qualified.
2. **Ownership remains simple.** No second socket queue, runtime packet registry, hidden object graph, synchronization surface, stale cache fallback or ambiguous lifetime owner may be introduced to save a small amount of work.
3. **Memory accounting remains explicit.** A reduction in allocation count is not automatically a win if it increases long-lived resident memory, introduces pooling, widens connection state, or couples unrelated lifetimes.
4. **The candidate has structural evidence.** Removed work, allocation owners, copied bytes, service calls, or other deterministic costs are recorded where they can be counted.
5. **The candidate has timing evidence.** A paired benchmark must preserve identical semantic output and show a repeatable improvement on the measured workload before the optimization is merged. Hosted CI timing is diagnostic only; target-hardware evidence is required before making broad production-performance claims.
6. **Tail behavior is not materially worse.** p99, p99.9, slowest-tail means and robust jitter metrics are compared alongside p50. A repeatable tail regression larger than the measured noise envelope blocks a routine performance merge even if the mean or p50 improves.
7. **The measured direction is stable.** Full qualification should use multiple independent runs on a pinned, otherwise quiet target CPU. A change whose paired/epoch direction alternates between faster and slower remains inconclusive until the measurement or the implementation explains the disagreement.
8. **The complete gate remains green.** Workspace check, Clippy, Rust tests, target qualification, server qualification and the relevant black-box confirmation must all pass.

A candidate which is faster but makes the architecture materially worse is rejected. A candidate which is cleaner but does not improve the relevant cost is treated as an architecture change rather than a performance optimization and must justify itself separately.

### Benchmark discipline

R2B paired microbenchmarks use balanced **ABBA / BAAB** measurement blocks. Each block measures each candidate twice and alternates the bracketing order between blocks. This reduces first/second-order bias and cancels much of a linear frequency or host-load drift which a simple AB comparison can mistake for a candidate effect.

The measured blocks are divided into epochs. Evidence records both per-block and per-epoch candidate/reference ratios in parts per million (ppm), plus the fraction of blocks and epochs in which the candidate wins. Independent processes are required because within-process samples alone cannot reveal process-start, frequency-state or host-placement variance.

For target-hardware acceptance, prefer at least **five independent full runs** pinned to the same physical CPU under a quiet system state. The expected pattern for a real small optimization is:

- the paired median ratio remains below `1_000_000 ppm` in a strong majority of runs;
- epoch direction is predominantly the same rather than approximately 50/50;
- the improvement is large enough to distinguish from paired-ratio MAD and run-to-run spread;
- p99 and p99.9 do not show a repeatable material regression;
- slowest-1%/0.1% means and relative MAD do not reveal a new jitter problem.

Do not manufacture a fixed percentage win threshold for every mechanism. A 0.5% change in a very hot path can matter while a 5% microbenchmark win in an irrelevant path may not. The evidence must be interpreted in terms of frequency, end-to-end contribution and measurement noise.

## Baseline

The replay-free R2B vertical was frozen by the squash merge:

- `7e0aac0decc880a7628d2b8639d4f7948c27f8cc` — `R2B: source-admit replay-free Play bootstrap`

That baseline already provides:

- one bounded connection driver across the Configuration -> Play handoff;
- transfer of the existing read scratch instead of handoff reallocation;
- one contiguous dynamic bootstrap arena rather than one allocation per dynamic body;
- one reused bounded `PacketWriter` during dynamic preparation;
- process/composition-owned command and recipe projections;
- target-owned compile-time packet identities;
- transactional teleport and keep-alive state;
- an explicit `WorldProjection` ownership seam.

Post-baseline optimization work must therefore demonstrate a win against an already compact design rather than against the earlier replay scaffold.

## OPT-R2B-001 — certify immutable shared packet kinds once

**Status:** qualification in progress on PR #192; current timing evidence is intentionally classified as inconclusive until the strengthened distribution benchmark is stable.

### Previous cost

Every admitted join borrowed the immutable commands, update-recipes and optional server-data bodies, then reparsed the canonical packet-ID `VarInt` and compared it with the target-owned packet identity.

Those bytes are process/composition-owned and immutable. Revalidating packet kind for every connection repeated work which could not change between joins.

### Candidate

Packet kind is certified when the immutable artifact is constructed. The public construction surface uses semantic artifact names:

- `CommandProjectionArtifact`;
- `RecipeProjectionArtifact`;
- `ServerDataProjectionArtifact`.

Packet numbers remain crate-private compile-time Minecraft 26.2 facts. Empty bodies, malformed or non-canonical packet-ID `VarInt`s, and wrong packet kinds fail before an artifact can be shared with a connection.

The join path retains exact revision-key checks, so stale composition/status state still fails closed.

### Structural effect

For the admitted route:

- packet-ID decodes/comparisons per join for shared artifacts: **3 -> 0** when server data is present;
- new allocations per join: **0**;
- new synchronization operations: **0**;
- new connection/session fields: **0**;
- runtime packet registries/lookups: **0**.

### Timing probe

The target-local paired benchmark is:

```text
cargo bench --locked --package crucible-target-26-2 --bench r2b_shared_artifact -- --full --output target/r2b-shared-artifact.json
```

The schema-2 benchmark compares the previous join-time validation behavior with the construction-certified behavior while preserving the same key lookups and semantic checksum. It uses 1,024-join micro-batches, balanced ABBA/BAAB blocks, many thousands of service-time samples, epoch ratios, and robust tail/jitter summaries.

It records:

- raw service-time samples for both candidates;
- p50/p90/p95/p99/p99.9/max;
- mean, MAD and IQR;
- slowest-1% and slowest-0.1% means;
- p99/p99.9/max amplification over p50;
- paired block ratios and their spread;
- per-epoch ratios and win rate;
- structural packet-decode/allocation counts;
- the semantic checksum.

The dedicated CI workflow pins the process to one allowed CPU and executes three independent smoke runs. Hosted CI remains diagnostic: it is useful for detecting gross regressions and unstable measurement direction, but it is not the authority for production magnitude.

The earlier coarse benchmark produced opposite directions on two hosted runs. Those observations are retained as the reason schema 1 was rejected as insufficient evidence; neither run is treated as proof that OPT-R2B-001 is faster or slower.

## OPT-R2B-002 — preparation scratch reservation

**Status:** investigation only; no production change admitted.

The production server currently creates a bounded preparation `PacketWriter` with a 4 KiB semantic maximum and reuses it for every dynamic R2B body. The writer starts with an empty `Vec`, so the first dynamic bodies may grow its retained allocation incrementally.

`PacketWriter::with_capacity(maximum, initial_capacity)` already exists and can reserve a known useful amount while retaining the same semantic packet bound.

This is a plausible optimization only if a defensible reservation can be derived without encoding one captured player's body sizes as a semantic bound. Candidate reservations must be benchmarked against the existing `PacketWriter::new(4096)` path. Oversized per-join reservation which merely trades allocator calls for wasted resident bytes is not an automatic win.

## Dynamic arena allocation — currently retained

The contiguous dynamic arena is currently considered **architecturally justified**, not an obvious allocation-removal target.

Prepared dynamic bodies must remain stable for staged publication after the reusable scratch writer has been cleared and reused. Removing the arena allocation would therefore require another stable owner: lifetime coupling to unrelated connection buffers, a pool, a larger long-lived session allocation, or direct-final-storage encoding with more complex transactional bookkeeping.

The existing qualification benchmark is:

```text
cargo run --release --locked --package crucible-client-spine-qualification --bin r2b_bootstrap_arena_bench -- --full --output target/r2b-bootstrap-arena.json
```

It compares per-body ownership, the one-allocation arena, and a qualification-only direct-storage floor while checking byte-equivalent semantics and structural memory ownership.

The direct floor is deliberately not a production candidate by itself. A future direct-final-storage design must first demonstrate equally clean transactional ownership and a material end-to-end win.

## Evidence recording policy

For every accepted optimization, update this ledger with:

- the merge commit;
- the exact benchmark command and benchmark schema/name;
- structural before/after counts;
- representative paired central, tail and jitter evidence and the hardware/context in which it was collected;
- independent-run and epoch direction stability;
- all invariants which remain unchanged;
- any rejected alternative considered during the same investigation.

If a later architecture change invalidates a benchmark workload or cost model, mark the old evidence historical rather than silently reinterpreting it.
