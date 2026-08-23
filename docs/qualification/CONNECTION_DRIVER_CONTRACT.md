# Connection Driver Contract

Parent: #91 / client spine #78

## Purpose

`crucible-connection-driver` is the runtime-neutral processing boundary above the admitted bounded connection buffers and below target-version Minecraft packet semantics.

It exists so socket/runtime experiments do not duplicate or weaken stream progression, fairness, backpressure, and malformed-input behavior.

## HOT-path law

For one connection poll:

```text
socket fragment
  -> bounded ingress append
  -> borrowed complete FrameView
  -> statically dispatched target/session handler
  -> consume exact stream bytes only after handler success
  -> repeat until incomplete / explicit yield / frame budget
```

The driver performs no per-frame payload allocation or copy. `FrameView` continues to borrow directly from the admitted ingress storage.

`FrameBudget` is positive by construction. Every processing call therefore has an explicit finite fairness boundary chosen by its caller. One connection cannot implicitly drain an unbounded coalesced stream in a single executor turn.

## Transaction boundary

The driver guarantees **stream progression** transactionality:

- malformed framing returns an error before consumption;
- a target/session handler error leaves the current frame logically unconsumed;
- a successful handler result commits exactly that frame's `stream_bytes`;
- handler-requested yield commits the current frame, then returns control;
- budget exhaustion never partially consumes the next frame.

The driver cannot roll back arbitrary external state mutated inside user handler code. Target/session handlers must therefore validate before committing their own semantic state, or provide their own transaction mechanism. This distinction is explicit rather than hidden behind a callback abstraction.

## Egress law

The driver reuses `EgressBuffer` unchanged:

- callers queue already-formed packet bodies;
- complete frame size is validated before mutation;
- unwritten bytes remain bounded;
- partial socket writes expose a contiguous borrowed slice;
- write acknowledgement consumes exactly the count reported by the adapter;
- impossible counts fail instead of clamping.

## Deliberate non-decisions

This layer contains no:

- Minecraft 26.2 / protocol 776 packet IDs;
- handshake/status/login/configuration semantics;
- session phase state;
- authentication, encryption, or compression policy;
- Tokio, mio, io_uring, epoll, kqueue, or blocking-runtime selection;
- `Arc`, mutex, service locator, mandatory trait object, unsafe code, buffer pool, ring buffer, slab, or custom allocator.

Those are separate mechanisms and must preserve this law. Runtime/buffer alternatives require whole-cost evidence before replacing the simple admitted mechanism.

## Permanent qualification

Tests in the core crate cover:

- byte-at-a-time fragmentation;
- coalesced frames;
- explicit per-poll fairness budgets;
- handler error with exact ingress rollback;
- handler-requested yield;
- borrowed payload pointer stability across a rejected/retried frame;
- egress overflow rollback;
- exact partial-write accounting;
- impossible write-count rejection;
- a deterministic 20,000-frame stream compared across radically different fragmentation and frame-budget schedules.

The long trace requires identical frame count and content checksum independent of transport fragmentation or scheduler budget.

## Next boundary

After the source-backed 26.2 handshake/status VAR+SEM is frozen, target packet logic can be layered above this driver. A small localhost socket adapter can then exercise status/ping against an unmodified client without making that adapter the permanent production runtime decision.
