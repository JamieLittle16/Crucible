# Pre-Play I/O Adapter Contract

Issue: #129  
Parent: #78  
Depends on: production binder #123

## Purpose

`crucible-preplay-io` is the target-neutral production boundary between a byte transport and `PrePlayConnection<T>`.

It promotes the already-qualified socket-pumping law into reusable production code without selecting a Minecraft target version or a permanent server runtime.

## Production data path

```text
transport Read/Write
    ↓ retained fixed read scratch
bounded PrePlayConnection<T> ingress
    ↓ borrowed framed packet view
statically bound target T
    ↓ atomic outbound batch + session commit
contiguous bounded egress
    ↓ direct borrowed write slice
transport Read/Write
    ↓ exact write acknowledgement
PrePlayConnection<T>
```

The adapter adds no per-frame payload allocation and no second outbound byte queue. The read scratch allocation is retained for the lifetime of the adapter.

## Fairness law

Every semantic processing/service call receives a positive `ActionBudget`.

`service_once` performs at most:

- two transport write attempts;
- one transport read attempt;
- the caller-declared number of semantic actions.

A coalesced or hostile connection therefore cannot implicitly monopolize an executor turn by draining an unbounded packet stream.

## Transaction and failure law

The adapter preserves `PrePlayConnection<T>` transactionality and adds explicit transport behavior:

- a target error does not consume the current frame or advance the session;
- exact partial-write counts are acknowledged back into the bounded egress queue;
- a successful zero-byte write while egress is non-empty is an error, never a spin loop;
- `WouldBlock`/`Interrupted` are nonfatal scheduling outcomes;
- non-transient I/O errors retain operation + `ErrorKind` provenance;
- clean EOF is accepted only when no incomplete inbound frame remains;
- EOF with buffered partial framing fails closed;
- terminal session closure stops further target processing;
- accounting overflow is explicit rather than wrapping.

## Deliberate non-decisions

This crate contains no:

- Minecraft 26.2 / protocol 776 packet identities or layouts;
- packet registry or runtime target lookup;
- Tokio, mio, io_uring, epoll, kqueue, listener, accept-loop or executor policy;
- `Arc`, mutex, mandatory trait object or service locator;
- compression, encryption or authentication policy;
- unsafe code, ring buffer, slab, pool or custom allocator.

A localhost R0 executable may instantiate this boundary with `TcpStream`; doing so does **not** select `std::net` as Crucible's eventual high-scale networking runtime.

## Permanent qualification

The crate's own tests cover:

- all two-fragment split points for the synthetic handshake;
- coalesced actions under an explicit action budget;
- exact one-byte partial-write acknowledgement;
- zero-write rejection;
- truncated EOF rejection;
- target-error transactionality;
- terminal-close behavior with trailing input;
- the independently callable bounded processing primitive;
- 10,000 status actions through one retained adapter under hostile small reads/writes;
- one real localhost TCP roundtrip through the production adapter;
- direct partial-write progress without a second outbound queue.

CI owns these through the named `Pre-play I/O adapter qualification` gate in addition to full workspace check/Clippy/tests/rustdoc.

## Performance posture

This boundary is designed to avoid obvious per-frame overhead: static target dispatch, borrowed frame/payload access, retained scratch storage, contiguous egress and exact acknowledgement.

That is not a claim that the generic `Read + Write` mechanism is the final runtime. Event-loop/runtime candidates must preserve this semantic boundary and then compete under controlled whole-path evidence for throughput, allocations, copies, syscalls/wakeups, cache behavior, retained memory, p50/p95/p99 latency, connection-count scaling and interference with simulation work.

## R0 consequence

After this gate, generic product I/O is no longer an R0 architecture blocker. The remaining first-client status path is:

```text
admitted 26.2 status source + capture evidence
    ↓
generated/static Target26_2 status adapter
    ↓
PrePlayIo<Target26_2>
    ↓
small localhost listener/executable
    ↓
unmodified 26.2 client status + ping probe
```

No target-specific implementation may bypass the evidence admission gate tracked by #107 / #124.
