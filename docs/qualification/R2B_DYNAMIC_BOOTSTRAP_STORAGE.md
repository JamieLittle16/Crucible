# R2B Dynamic Bootstrap Storage Qualification

**Status:** mechanism tournament; no lower-bound shortcut is production-admitted  
**Target:** Minecraft Java 26.2 R2B replay-free Play entry  
**Normative performance policy:** [`PERFORMANCE_QUALIFICATION_STANDARD.md`](PERFORMANCE_QUALIFICATION_STANDARD.md)  
**Semantic predecessor:** [`R2B_PLAY_BOOTSTRAP_CORE.md`](R2B_PLAY_BOOTSTRAP_CORE.md)

## Purpose

R2B has two very different output classes and must not force them into one ownership model.

Composition-stable publications such as the admitted command tree and synchronized recipes are
process/composition-owned immutable artifacts. Matching connections borrow those exact bodies; they
are not rebuilt per join.

The remaining selected-profile Play bootstrap bodies depend on player/session/level state and must be
materialized per connection. The storage mechanism for those bodies must satisfy all of:

- exact source-admitted 26.2 wire semantics;
- no runtime packet registry or Mojang-shaped packet object graph;
- no second outbound queue;
- bounded/fail-closed construction;
- cheap immutable borrowing during staged publication;
- minimal allocation, copying and metadata;
- explicit handoff to `WorldProjection` rather than network ownership of world/chunk/light state.

This document qualifies the storage mechanism only. It does not admit packet identities, field laws,
stage ordering or world semantics.

## Current preferred candidate

`DynamicBootstrapArena<N>` is a target-local contiguous owner:

```text
one reusable bounded PacketWriter scratch allocation
                    |
                    | serialize one dynamic packet
                    v
              [ scratch body ]
                    |
                    | one exact seal copy
                    v
+--------------------------------------------------+
| contiguous arena bytes: body0 body1 ... bodyN-1 |
+--------------------------------------------------+
       ^          ^                         ^
       |          |                         |
    u32 span   u32 span                  u32 span
```

The packet spans are inline in the arena object. After construction, publication borrows slices from
the arena and never allocates, copies or resolves packet identities.

The scratch writer is reused across the complete finite batch. `PacketWriter::with_capacity` allows
the target to reserve a tight known scratch capacity once while retaining the independent semantic
packet-size bound. `reset()` clears only the logical body length and retains the allocation.

### Structural cost

For a finite `N`-body bootstrap, the intended construction has:

- one long-lived arena byte buffer owner;
- one temporary/reusable scratch byte buffer owner;
- no per-body heap owner;
- one body-to-final-owner copy per dynamic body;
- two `u32` values per possible body span;
- one body-count word;
- zero storage-owned ordering state;
- zero storage-owned socket/backpressure state.

The arena is deliberately not a queue. Publication progress belongs to
`crucible-publication-core::StagedPublicationCursor`; egress bytes belong to the existing connection
driver.

## Why this is not declared optimal by inspection

The seal copy is real work. A direct-to-arena writer could theoretically reduce the construction to
one byte-buffer owner and no seal copy.

That does **not** make direct writing automatically superior. A production direct writer would also
need to preserve the existing `PacketWriter` properties:

- every field checks its packet budget before mutation;
- semantic field rejection leaves bytes unchanged;
- a failed packet can roll back without corrupting previously sealed bodies;
- packet body boundaries remain explicit;
- target codecs do not duplicate target-neutral Minecraft wire primitives;
- no runtime dispatch/trait-object tax appears in the hot path.

A direct implementation that duplicates codecs, weakens rollback, adds lifetime-heavy plumbing or
moves complexity into every target serializer may cost more than one small contiguous copy.

Therefore the copy is a measured hypothesis, not an assumed defect.

## Tournament candidates

`r2b_bootstrap_arena_bench` compares three mechanisms over byte-identical synthetic finite joins.

### A. Per-body owned reference

Each dynamic packet gets its own exactly-sized `PacketWriter`/`Vec<u8>` and the join owns a
`Vec<Vec<u8>>`.

This is the simple packet-object-shaped reference. It has no final seal copy, but pays one byte-buffer
owner per dynamic body plus the outer vector and carries per-vector metadata/allocator lifetime.

This candidate exists to quantify what Crucible avoids by collapsing ephemeral packet ownership.

### B. Reused scratch + contiguous arena

One pre-sized `PacketWriter` is reused across all bodies and one exactly pre-sized arena stores the
final bytes. Every body is copied once at seal time.

This is the current preferred production candidate because it combines simple target codecs,
transactional `PacketWriter` semantics, compact final ownership and allocation reuse.

### C. Optimistic direct-flat floor

The benchmark also emits an intentionally stronger lower bound: one pre-sized flat `Vec<u8>` receives
packet-id and payload bytes directly with no seal copy.

**This is not a production candidate and cannot be selected by the benchmark.** It deliberately omits
the complete transactional writer API and exists only to estimate the maximum plausible value of
removing the scratch/seal boundary. If the arena is already close to this floor, a direct writer is
not worth architectural complexity.

## Equivalence gate

Timing is recorded only after all three candidates agree exactly on:

- number of bodies;
- body boundaries;
- total body bytes;
- every body byte, represented by a boundary-sensitive checksum.

Measured rounds also recheck semantic equality. Any mismatch aborts the benchmark.

Synthetic payload bytes are intentionally simple: this tournament isolates ownership/allocation/copy
mechanics rather than pretending to benchmark the final semantic codecs before those codecs exist.
The tournament must be rerun with the exact admitted R2B body-size corpus once the complete dynamic
bootstrap builder is frozen.

## Workload regimes

The harness currently carries three construction regimes:

1. `tiny-control` — worst case for per-body allocation/metadata relative to useful bytes;
2. `selected-profile-like` — mixed small/medium dynamic bodies resembling the finite R2B join shape;
3. `wide-metadata` — larger metadata bodies where the arena seal copy becomes more visible.

Smoke mode exists for CI correctness and catastrophic-regression detection. Full mode increases
fanout, warmup and balanced measured rounds.

Candidate order rotates every round so one mechanism is not systematically measured with warmer
allocator/cache/frequency state.

## Allocation and copy accounting

The JSON evidence records structural owner counts separately from elapsed time.

For the benchmark's 18-body finite workload:

- per-body reference: 19 heap buffer owners (18 bodies + outer vector);
- arena: 2 heap buffer owners (scratch + final arena);
- direct floor: 1 heap buffer owner;
- arena seal-copy bytes per join: exact sum of dynamic body bytes;
- per-body reference vector metadata: `18 * size_of::<Vec<u8>>()`;
- arena inline handle/span footprint: `size_of::<DynamicBootstrapArena<18>>()`.

"Heap buffer owner" is an ownership/layout count, not a claim about allocator-internal calls. The
benchmark pre-sizes buffers to avoid accidental geometric growth obscuring the mechanism comparison.
Allocator-event and fragmentation evidence can be added on controlled hardware with external tools
when it becomes decision-relevant.

## Measurement and decision law

GitHub-hosted timing is diagnostic only, per the Performance Qualification Standard. CI may require:

- benchmark compilation/execution;
- byte equivalence;
- nonzero semantic checksum;
- expected structural owner counts;
- complete paired/rotating sample arrays.

CI timing may not select the production mechanism.

Before replacing the scratch+arena candidate with a direct writer, controlled target-hardware
qualification must show a material whole-cost win. Default reopening threshold:

- roughly >=5% improvement in dynamic-bootstrap construction or a demonstrated material allocation /
  tail-latency improvement;
- and a meaningful contribution at whole-join/server level;
- with no semantic, backpressure, memory, code-size or maintainability regression.

The comparison must include cold construction, warmed burst joins, p50/p95/p99 where sample count is
sufficient, copied bytes, retained memory and allocator evidence where available.

A tiny microbenchmark win does not justify duplicating packet-wire logic.

## Production invariants

Whichever storage candidate wins, the R2B production path must preserve these invariants:

1. command and synchronized-recipe bodies remain composition-owned immutable shared projections;
2. dynamic bodies are materialized exactly once for the selected connection state;
3. publication borrows already-materialized bodies and performs no per-body allocation;
4. `StagedPublicationCursor` remains the sole bootstrap progress owner;
5. connection-driver egress remains the sole outbound queue/buffer;
6. body admission/backpressure never advances publication state on failure;
7. target packet IDs are compile-time generated facts, never runtime registry lookups;
8. `WorldProjection` remains an ownership seam for R2C;
9. no benchmark result can weaken source/semantic qualification.

## Reopen triggers

Re-run this tournament when any of the following materially changes:

- selected R2B dynamic body count or body-size distribution;
- target codecs or their maximum packet bounds;
- `PacketWriter` storage model;
- arena span representation;
- allocator policy;
- compiler/codegen policy;
- target CPU class;
- publication begins consuming bodies in a way that changes retention/lifetime.

## Exit criteria

This storage slice is ready for production integration when:

- workspace format/check/Clippy/tests/rustdoc are green;
- the arena rollback/overflow/reuse tests are green;
- the benchmark smoke gate is green and byte-equivalent;
- the exact R2B dynamic builder uses a pre-sized reusable scratch writer and exact-capacity arena;
- controlled full benchmark evidence either supports the arena or justifies a measured replacement;
- qualification docs record the chosen mechanism and its reopen trigger.
