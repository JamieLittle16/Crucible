# R2B Performance Evidence Ledger

This document records post-R2B performance changes which alter the replay-free Minecraft 26.2 Play-bootstrap path.

The purpose is not to maximize the number of micro-optimizations. It is to preserve a reproducible record of which changes were accepted, which were rejected, and why the accepted implementation remains correct and architecturally clean.

## Acceptance law

An R2B optimization is accepted only when all of the following remain true:

1. **Protocol and semantic correctness is unchanged.** Source-backed packet order, packet identity, branch law, transactional state changes, teleport acknowledgement, liveness behavior and the `WorldProjection` ownership seam remain qualified.
2. **Ownership remains simple.** No second socket queue, runtime packet registry, hidden object graph, synchronization surface, stale cache fallback or ambiguous lifetime owner may be introduced to save a small amount of work.
3. **Memory accounting remains explicit.** A reduction in allocation count is not automatically a win if it increases long-lived resident memory, introduces pooling, widens connection state, or couples unrelated lifetimes.
4. **The candidate has structural evidence.** Removed work, allocation owners, copied bytes, service calls, or other deterministic costs are recorded where they can be counted.
5. **The candidate has timing evidence.** A paired benchmark must preserve identical semantic output and show an improvement on the measured workload before the optimization is merged. Hosted CI timing is diagnostic only; target-hardware evidence is required before making broad production-performance claims.
6. **The complete gate remains green.** Workspace check, Clippy, Rust tests, target qualification, server qualification and the relevant black-box confirmation must all pass.

A candidate which is faster but makes the architecture materially worse is rejected. A candidate which is cleaner but does not improve the relevant cost is treated as an architecture change rather than a performance optimization and must justify itself separately.

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

**Status:** qualification in progress on PR #192.

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
cargo bench --package crucible-target-26-2 --bench r2b_shared_artifact -- --full --output target/r2b-shared-artifact.json
```

It compares the previous join-time validation behavior with the construction-certified behavior while preserving the same key lookups and semantic checksum. Measurement rounds alternate candidate order.

Do not interpret a hosted CI p50 as a universal hardware claim. It is an acceptance signal for this small change; target-hardware runs remain the authority for production magnitude.

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
- representative paired timing evidence and the hardware/context in which it was collected;
- all invariants which remain unchanged;
- any rejected alternative considered during the same investigation.

If a later architecture change invalidates a benchmark workload or cost model, mark the old evidence historical rather than silently reinterpreting it.