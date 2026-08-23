# Fused Outbound Construction Laboratory

Status: **qualification experiment; production path unchanged**

Parent: [Client P0 protocol spine](../architecture/PROTOCOL_CLIENT_SPINE.md)  
Tracking issue: #119

## Question

Crucible's admitted outbound path is deliberately simple:

```text
semantic packet fields
        ↓
PacketWriter
        ↓ owned packet-body Vec<u8>
EgressBuffer::queue_frame
        ↓ frame-prefix append + body copy
bounded final egress Vec<u8>
```

That path is easy to reason about, fail-closed and already qualified. It does, however, construct one
intermediate owned packet-body vector and then copy those bytes into the final bounded egress queue.
For sufficiently frequent or sufficiently large outbound packets, that allocation/copy may be worth
removing.

This laboratory asks only:

> Does constructing validated packet fields directly in the final bounded egress storage produce a
> material **whole-cost** improvement without weakening any packet/framing/resource invariant?

The answer is experimental. The repository must not assume that the fused mechanism wins.

## Compared mechanisms

### Reference

The benchmark uses the real production mechanism:

```text
PacketWriter
→ EgressBuffer::queue_frame
```

The intermediate packet-body allocation, field validation, frame preflight, frame prefix, body copy,
buffer compaction and partial drains are all inside the measured mechanism.

### Fused candidate

The candidate exists only in `crucible-client-spine-qualification`.

It:

1. computes and validates the exact encoded packet-body length;
2. validates the same frame-body and bounded-egress limits;
3. compacts consumed final-buffer bytes under the same policy shape;
4. reserves final frame bytes;
5. appends the frame-length `VarInt` directly to final storage;
6. appends the packet ID and fields directly after it; and
7. truncates to the exact pre-operation tail on any subsequent encoding failure.

It does **not** use a runtime packet registry, dynamic dispatch, unsafe code, an unbounded staging
queue, a buffer pool, a custom allocator, vectored I/O or a runtime-specific socket mechanism.

## Semantic gate

Timing is forbidden until byte equivalence succeeds.

The benchmark compares the complete active egress image after every packet and every deterministic
partial drain. Permanent tests additionally cover:

- frame-body `VarInt` prefix boundaries at 127/128 and 16383/16384 bytes;
- repeated partial-drain and compaction/reuse traces;
- body-limit rejection without mutation;
- invalid-string rejection without mutation; and
- bounded-egress rejection without mutation.

The reference and candidate must also report identical useful body-byte counts, final queued-byte
counts and semantic checksums in timed runs.

A semantic mismatch is a failed experiment, never a performance result.

## Workload families

The current deterministic families are deliberately target-neutral:

- `tiny_ping` — one packet ID plus an eight-byte scalar;
- `status_string` — one bounded UTF-8/UTF-16 string;
- `medium_metadata` — mixed VarInt, fixed-width, boolean and bounded string fields;
- `large_blob` — large byte payload representative of later streaming pressure; and
- `coalesced_mix` — mixed small/medium/large packets under repeated partial drains.

These shapes are **not Minecraft 26.2 packet-law claims**. Target packet identities remain gated by
the source/VAR/SEM/protocol-contract pipeline.

## Measurement protocol

Run the full experiment only on controlled target hardware:

```bash
cargo run --release --locked \
  --package crucible-client-spine-qualification \
  --bin fused_outbound_bench -- \
  --full \
  --output target/fused-outbound-full.json
```

The artifact retains:

- shared CPU/cache/affinity/SMT/frequency/NUMA provenance;
- alternating candidate order;
- warm-up rounds;
- every raw paired timing;
- p50/p95/p99/max for both paths;
- exact useful body bytes and final queued bytes; and
- the fused/reference p50 ratio for each workload family.

Hosted CI executes only `--smoke`. Hosted-runner timing is diagnostic and **must never select the
production mechanism**.

## Whole-cost rule

Do not move work outside the timer to manufacture a win.

Construction, validation, framing, allocation/reservation behavior, copies, compaction and the
configured partial-drain/reuse pattern belong to the mechanism being compared. Memory-retention or
allocation effects observed during target qualification must be considered alongside latency and
throughput.

If later profiling identifies a separate bottleneck—allocator behavior, writev/vectored I/O, buffer
pooling, socket runtime, compression or batching—it receives its own hypothesis and experiment.

## Admission rule

The fused candidate may proceed to a **separate production/equivalence PR** only when all of the
following hold:

1. byte equivalence and rollback qualification are completely green;
2. controlled target-hardware measurements are stable under the project microarchitecture standard;
3. the candidate shows a material whole-cost improvement on the workload families that justify its
   additional complexity;
4. the improvement is not obtained by excessive retained capacity or hidden setup; and
5. the production design can preserve the current bounded transactional semantics without adding a
   second source of packet-law truth.

If those conditions do not hold, the experiment records the result and Crucible retains the simpler
`PacketWriter → EgressBuffer` path.

## CI role

CI proves only that:

- the benchmark builds in release mode;
- the permanent byte-equivalence/rollback tests pass;
- the smoke artifact is parseable and complete; and
- production remains on the reference path.

A green CI run is **not** evidence that the fused mechanism is faster.
