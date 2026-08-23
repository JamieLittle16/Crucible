# Atomic Outbound Admission Qualification

**Tracking:** #101  
**Layer:** target-neutral pre-play connection transaction boundary  
**Status:** mechanism + permanent qualification; no Minecraft packet IDs

## Purpose

Login and Configuration actions may require multiple outbound packets. Crucible must never consume
an inbound frame or adopt a candidate session transition and only afterward discover that the
complete required response cannot fit inside the bounded connection egress queue.

The admitted ordering is:

```text
borrow complete inbound frame
        ↓
decode/validate into caller-owned candidate action
        ↓
expose complete outbound frame-body batch
        ↓
validate every body + compute aggregate encoded cost
        ↓
admit entire batch against bounded egress
        ↓
consume exact inbound stream bytes
        ↓
return committed candidate action
        ↓
caller adopts candidate semantic/session state
```

Any failure before the commit boundary leaves the current inbound frame logically unconsumed and
leaves the existing outbound queue unchanged.

## Mechanism

`EgressBuffer::queue_batch` is intentionally a two-pass control-boundary operation:

1. validate every frame body;
2. calculate each encoded frame length with checked arithmetic;
3. calculate the complete batch cost with checked arithmetic;
4. compare `queued + batch` against the exact egress byte limit;
5. compact an already-consumed physical prefix at most once;
6. append all encoded frames;
7. on any unexpected encoder failure, truncate only the newly appended tail.

There is no second queue, hidden unbounded staging buffer, packet registry, trait object or runtime
lookup. The existing one-frame `queue_frame` path remains unchanged.

`ConnectionDriver::process_one_transactional` composes this egress admission with inbound commit. A
handler returns an owned candidate action implementing the statically dispatched `OutboundBatch`
trait. The driver admits that complete batch before consuming the frame. The caller receives the
action only after both connection-side commit steps succeed and can then adopt its candidate session
state.

If the already-peeked ingress frame were unexpectedly impossible to consume after successful egress
admission, the driver restores egress to its exact prior logical length before returning the error.
A double invariant failure is surfaced explicitly as `DriverError::RollbackFailed`.

## Required permanent evidence

Core and cross-layer tests cover:

- empty batch is a no-op;
- valid batch bytes equal repeated one-frame encoding exactly;
- exact-fit aggregate admission;
- aggregate over-capacity rejection;
- existing partially written egress is included in capacity accounting;
- malformed later frame rejects the entire batch;
- rollback removes only newly appended tail bytes after physical compaction;
- incomplete inbound input does not call the semantic handler;
- handler rejection changes neither ingress nor egress;
- outbound-capacity rejection preserves the current inbound frame;
- outbound-capacity rejection preserves pre-existing egress;
- caller-owned candidate `SessionState` is not adopted on rejection;
- successful multi-packet admission occurs before caller session adoption.

## Performance boundary

This mechanism is for cold/control transitions such as Login and Configuration. It deliberately
optimizes for bounded atomicity and obvious correctness rather than speculative queue machinery.

The ordinary single-frame path is unchanged. If future profiling shows batch preflight or compacting
`Vec<u8>` storage to be material in a HOT packet path, alternate rings, slabs, pools, vectored I/O or
reservation structures must compete under the Performance Qualification Standard before replacing
this reference mechanism.

## Scope

This qualification introduces no target Minecraft packet identity, authentication policy,
compression policy, Configuration registry semantics, socket runtime or gameplay behavior.

It closes the generic atomic-response prerequisite needed before source-backed Login/Configuration
handlers may require multiple outbound frames.
