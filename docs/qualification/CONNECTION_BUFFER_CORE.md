# Connection Buffer Core

Parent: #78

This document qualifies the version-agnostic byte-buffer boundary between a socket adapter and Crucible's target-version packet state machine.

## Scope

`crucible-connection-core` owns:

- bounded accumulation of arbitrarily fragmented TCP reads;
- borrowed discovery of complete Minecraft frames;
- splitting the frame-body packet-ID `VarInt` from the remaining borrowed payload;
- exact stream-byte consumption;
- bounded encoded egress bytes and partial-write consumption;
- explicit backpressure/fail-closed errors;
- amortized prefix compaction for the transparent reference mechanism.

It deliberately does **not** own:

- target-version packet IDs or field layouts;
- handshake/login/configuration/play transitions;
- authentication, encryption or compression policy;
- sockets, readiness notification or executor choice;
- player/world/gameplay state.

Those remain higher layers of `PROTOCOL_CLIENT_SPINE.md`.

## Resource law

Every connection has three explicit byte limits:

1. maximum decoded frame body;
2. maximum active ingress bytes;
3. maximum unwritten egress bytes.

The configured ingress and egress limits must each be able to hold one maximum-sized encoded frame. Invalid limit sets fail at construction rather than producing a connection that advertises a frame limit it cannot physically buffer.

Appending received bytes first proves:

```text
active_ingress + incoming <= ingress_limit
```

Queueing an encoded frame first proves:

```text
unwritten_egress + encoded_frame_bytes <= egress_limit
```

A failed bound check leaves the logical stream/queue unchanged. No caller can silently clamp or drop overflow bytes.

These are logical buffered-byte bounds. The reference implementation uses Rust `Vec<u8>` and therefore does not claim allocator-capacity bytes equal the logical bound. If physical allocator overhead becomes material, a fixed ring/slab/pool candidate must be measured explicitly rather than smuggled in as an assumption.

## Zero-copy decode path

The ingress buffer stores socket-read bytes once. `peek_frame()` delegates outer framing and packet-ID decoding to `crucible-protocol-core`, then returns:

- packet ID;
- borrowed payload slice;
- exact body byte count;
- exact stream byte count.

No complete-frame or payload copy/allocation is required by decode.

The borrowed `FrameView` naturally prevents mutable consumption while the view exists. Higher layers process the view and then consume exactly `stream_bytes`.

A complete outer frame whose body ends with an incomplete packet-ID `VarInt` is malformed, not merely fragmented: later TCP bytes belong to a later frame and cannot repair it.

## Compaction policy

The transparent reference mechanism keeps one consumed-prefix cursor into a contiguous `Vec<u8>`.

It compacts only when:

- the consumed prefix is at least half the physical buffer; or
- appending without compaction would take physical stored bytes beyond the configured logical bound.

Draining the whole buffer uses `clear()` and retains capacity. The HOT receive/write path never calls `shrink_to_fit`.

This mechanism is intentionally simple enough to audit and benchmark. A ring buffer, segmented buffer, slab/pool, vectored-I/O layout, `Bytes`, io_uring-specific store or custom allocator is a candidate, not doctrine.

## Permanent tests

The crate retains tests for:

- a valid frame arriving one byte at a time;
- two coalesced frames with exact independent consumption;
- complete outer frame + truncated packet ID;
- overlong packet-ID `VarInt` propagation;
- transactional ingress overflow;
- transactional egress overflow;
- partial socket writes;
- compaction preserving active bytes;
- invalid consume counts;
- incoherent limit sets;
- egress-to-ingress loopback through the same framing law.

These tests are target-version independent. Protocol-776 golden packet bytes are added above this layer, not here.

## Performance posture

The current design optimizes the properties already known to matter:

- no per-frame decode allocation;
- no per-frame decode copy;
- contiguous payload access;
- bounded queues;
- no dynamic packet registry;
- no synchronization inside a single connection buffer;
- amortized rather than per-frame compaction.

It deliberately does not yet optimize speculative details such as vectored writes, ring indexing, shared buffer pools or runtime-specific readiness structures.

Before a replacement is admitted, benchmark at minimum:

- fragmented small packets;
- coalesced packet bursts;
- large chunk-like frames;
- partial writes/backpressure;
- bytes copied per useful byte;
- allocations and retained capacity per connection;
- p50/p95/p99 decode and queue cost;
- memory under many idle and active connections.

The replacement must preserve the exact malformed/incomplete/resource-bound behavior.
