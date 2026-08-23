# Network Egress Fusion Laboratory

**Tracking:** #119  
**Scope:** target-neutral outbound packet construction  
**Status:** qualification experiment; no production mechanism selected

## 1. Hypothesis

Crucible's admitted reference outbound path is intentionally simple:

```text
packet fields
    ↓
PacketWriter-owned Vec<u8>
    ↓
EgressBuffer::queue_frame
    ↓
frame prefix + body copy into connection-owned Vec<u8>
```

That path is easy to reason about and already fail-closed, but it creates an intermediate owned
packet body and then copies that body into the final connection queue.

For HOT packet production, particularly many small packets, this may be engine-created work rather
than unavoidable Minecraft work.

P0Q tests one narrower alternative before changing production code:

```text
connection-owned Vec<u8>
    ↓ reserve one frame-prefix byte
encode packet id + fields directly into final storage
    ↓
canonical frame-prefix finalization
```

The experiment asks whether deleting the intermediate packet-body allocation/copy produces a
material whole-cost win without weakening framing, bounds or rollback semantics.

## 2. Reference authority

The reference candidate is not a reimplementation. The benchmark calls the real admitted:

- `crucible_packet_core::PacketWriter`; and
- `crucible_connection_core::EgressBuffer::queue_frame`.

Every fused result must be byte-for-byte identical to that production reference before timing is
considered.

## 3. Fused candidate

The qualification-only `FusedEgress` starts each frame with one placeholder byte. Packet ID and
payload fields are then written directly into the final connection-owned `Vec<u8>` under the same
frame-body and egress bounds.

After the body is complete:

1. compute the canonical Minecraft frame-length VarInt width;
2. if the prefix is one byte, overwrite the placeholder in place — the body does not move;
3. if the prefix is two or three bytes, grow by one or two bytes and move the completed body right
   exactly once;
4. write the canonical prefix bytes;
5. expose the result only after all bounds and finalization checks pass.

This matters because small packets with body length below 128 bytes need **zero final body copy**.
The two- and three-byte prefix regimes still move the body once, so the benchmark must show whether
removing the intermediate allocation remains worthwhile there.

No unsafe code, pool, custom allocator, vectored-I/O mechanism, runtime packet map, trait object or
socket-runtime change belongs to this experiment.

## 4. Transaction law

A failed fused write must leave the logical queued byte stream unchanged.

Permanent tests cover:

- packet field rejection after earlier fields were already written;
- frame/body bound rejection;
- string-law rejection;
- partial drain followed by another packet;
- canonical frame-prefix boundaries around 127/128 and 16383/16384 body bytes;
- tiny ping-like, short-string, mixed metadata and large raw payloads;
- exact equality to the admitted reference stream.

The candidate may compact already-consumed physical prefix storage before beginning a new packet.
That changes only physical layout, never the logical queued stream.

## 5. Benchmark cases

The release harness currently exercises:

- `ping-i64` — tiny fixed-width packet;
- `status-short-string` — small UTF-8/string-shaped packet;
- `prefix-transition-128` — first two-byte frame-prefix regime;
- `metadata-1k` — mixed fixed/string/raw fields;
- `stream-64k` — large payload where finalization itself moves the body.

Both candidates receive the exact same deterministic source values. The connection-owned queue is
retained across repeated packet construction/drain operations; the reference `PacketWriter` remains
per-packet because that is the cost under investigation.

## 6. Evidence artifact

The harness emits deterministic JSON structure containing:

- exact machine/toolchain provenance;
- smoke/full mode and warm-up/round counts;
- case/body identity;
- structural reference body-copy bytes per packet;
- fused finalization-move bytes per packet;
- raw paired reference/fused elapsed times;
- rotating candidate order;
- identical observable checksums;
- p50/p95/p99/max summaries.

Hosted CI timing is **diagnostic only**. CI proves the release harness executes, the reference and
candidate do identical observable work, and the evidence artifact is parseable.

## 7. Target-hardware protocol

A decision run must follow `MICROARCHITECTURE_PERFORMANCE_QUALIFICATION.md`.

At minimum:

1. use a release build from one exact commit;
2. pin/record CPU affinity and topology;
3. record governor/frequency/turbo/SMT/NUMA/cache/microcode state;
4. complete warm-up before measured rounds;
5. retain raw paired rounds rather than only summaries;
6. rotate reference/fused ordering;
7. investigate thermal/frequency migration or large run-order effects rather than averaging them
   away;
8. inspect the packet-size regimes separately;
9. include whole encode + canonical frame-finalization cost;
10. treat retained memory/capacity and bytes moved as part of the decision where measurable.

PMU evidence should be collected when it can distinguish the hypothesis — for example memory-copy
traffic, instructions or branch effects — but PMU availability is not itself required for the
semantic/equivalence gate.

## 8. Admission rule

P0Q does **not** change production networking.

A production fused writer may be proposed only if target-hardware evidence demonstrates a material
whole-cost benefit in the workloads where it would be installed, without an unacceptable regression
in another important regime.

If the candidate wins only for very small packets, the correct result may be a narrowly specialized
small-packet path rather than replacing `PacketWriter` universally.

If the candidate loses, the reference mechanism remains production and the rejected experiment is
still useful evidence: the apparent allocation/copy cost was not worth the added mechanism on the
measured target.

Any production change then requires its own EQUIV/performance-qualified PR. This laboratory is the
permission to decide, not the decision itself.
