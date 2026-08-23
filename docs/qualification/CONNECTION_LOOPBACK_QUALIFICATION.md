# Connection Loopback Qualification

Issue: #98  
Parent: #78

## Purpose

This qualification proves that Crucible's admitted bounded connection machinery works across a real operating-system TCP stream before Minecraft 26.2 packet semantics are introduced.

It is **not** a production runtime selection. `std::net` appears only in integration-test code. Production networking remains free to compare Tokio, mio, io_uring, platform-specific transports, pooling and vectored I/O later under whole-path evidence.

## Qualified boundary

The loopback test drives:

```text
real TcpStream
    ↓ one-byte client writes
kernel TCP byte stream
    ↓ 7-byte server read scratch
ConnectionDriver bounded ingress
    ↓ borrowed complete FrameView
static handler
    ↓ bounded framed egress
3-byte server write slices
    ↓ exact write acknowledgement
kernel TCP byte stream
    ↓ 2-byte client read scratch
ConnectionDriver bounded ingress
    ↓
exact packet ID + payload comparison
```

The test intentionally uses synthetic packet IDs. It therefore qualifies transport/framing integration only; it makes no protocol-776 packet-law claim.

## Permanent assertions

- real `127.0.0.1` TCP sockets are used;
- both socket directions have finite read/write timeouts so CI cannot hang indefinitely;
- client request bytes are written one byte at a time;
- server socket reads use a seven-byte retained stack scratch buffer;
- server response writes are bounded to at most three bytes per call and every returned kernel write count is acknowledged exactly through `ConnectionDriver`;
- client response reads use a two-byte retained stack scratch buffer;
- empty, text, binary, and 257-byte payloads survive exactly;
- decoded response packet IDs and payload bytes equal the independent expected sequence;
- no production crate gains a socket-runtime dependency or policy decision.

## Performance posture

This is a correctness/integration probe, not a throughput benchmark. Tiny I/O chunks deliberately maximize fragmentation pressure and syscall count. They must never be presented as a production transport recommendation.

When Crucible selects a production network runtime, candidates must preserve this semantic boundary and then compete on whole-connection throughput, allocations, copies, wakeups/syscalls, cache behavior, memory retention, p50/p95/p99 latency, connection-count scaling and interference with simulation work.
