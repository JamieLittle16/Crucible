# Configuration Publication Laboratory

Status: **qualification experiment; production publication mechanism not yet admitted**  
Tracking: #146 / #143  
Applies to: Minecraft 26.2 R1B Configuration publication and any later large pre-play control image

## Question

Pinned 26.2 source review shows that Configuration can require a large ordered publication after a
small semantic decision, especially known-pack negotiation followed by registry data and tags.

The existing atomic outbound mechanism is intentionally all-or-nothing: an inbound semantic action
is not consumed until its complete required response batch fits bounded egress. That mechanism
remains correct for naturally bounded responses.

This laboratory asks a different question:

> Can Crucible commit the small semantic decision atomically, then drain a large immutable ordered
> publication through a bounded per-connection cursor without rebuilding/copying the publication per
> client or weakening backpressure/ordering guarantees?

The answer is experimental. No production binder change is admitted by this document alone.

## Non-goals

This lab does not decide:

- Minecraft 26.2 packet IDs or field codecs;
- registry/tag semantic content;
- compression policy;
- socket runtime or executor;
- vectored I/O;
- buffer pooling;
- `Arc`/allocator policy;
- fresh-player Play packet identity;
- whether a fully preframed image ultimately beats prebuilt packet bodies.

Those are separate evidence or performance questions.

## Reference mechanism

The reference is deliberately simple and semantically obvious:

```text
publication packet descriptions
-> construct each packet body for one connection
-> queue through ordinary bounded framing
-> partially drain exactly as the transport reports writes
-> repeat until complete
```

For the target-neutral lab, publication bodies use synthetic identities and deterministic bytes. This
is not a Minecraft packet-law claim.

Reference costs remain inside the experiment: construction, allocation, body copies, framing,
partial-drain compaction and cleanup.

## Candidate mechanism

The candidate model is:

```text
one immutable publication image
+ one tiny per-connection cursor
+ ordinary bounded egress
```

A publication step may expose only the next already-formed immutable packet body. The connection
queues that one body through the existing framing/bounds machinery. The cursor advances only after
queue admission succeeds.

This shape intentionally separates:

- **semantic transaction** — e.g. commit the selected known-pack mode;
- **publication progression** — emit the already-decided ordered output under backpressure.

No hidden unbounded staging queue is allowed.

## Required semantic/resource invariants

Timing is forbidden until all permanent invariants pass.

### Exact ordering and bytes

For every deterministic publication fixture:

- candidate and reference emit byte-identical framed streams;
- packet order is identical;
- each packet is emitted exactly once;
- cursor completion occurs exactly after the final packet;
- empty publication is a no-op.

### Commit/rollback law

For each publication step:

- queue rejection must not advance the cursor;
- frame-body rejection must not advance the cursor;
- existing queued egress must remain unchanged on rejection;
- successful queue admission advances exactly one committed publication position;
- acknowledgement of socket writes advances only egress consumption, not semantic publication
  position beyond frames already admitted.

### Backpressure and boundedness

Permanent tests must include:

- exact-fit egress admission;
- one-byte-over-capacity rejection;
- repeated partial writes of one byte;
- repeated partial writes at deterministic irregular sizes;
- compact/reuse cycles after a consumed prefix;
- publication packet larger than max frame body;
- egress capacity smaller than the next valid frame;
- resumed publication after capacity becomes available;
- no growth beyond configured connection limits.

### Cross-connection isolation

At least two connections must drain the same immutable image under different write schedules.
Tests must prove:

- cursors advance independently;
- one connection's rejection does not affect another;
- immutable publication bytes never mutate;
- interleaved progress produces the same final stream per connection as isolated progress.

### Image lifetime and state footprint

The qualification implementation must make explicit:

- retained publication-image bytes;
- per-connection cursor/state bytes;
- whether any publication bytes are copied into per-connection semantic state;
- whether the candidate needs heap allocation merely to identify the next frame.

The intended candidate should require only cursor/stage state per connection. If the actual Rust
mechanism needs a larger ownership wrapper, the measured and documented cost wins or loses with it.

## Synthetic workload families

The target-neutral experiment uses deterministic shapes rather than guessed Minecraft packet sizes.

### `tiny_control`

Many small frames. Detects excessive per-step machinery and service-loop overhead.

### `mixed_control`

Small and medium frames with deterministic partial drains. Represents a generic control publication.

### `registry_like`

Dozens to hundreds of medium/large immutable bodies with varied lengths. This models the architectural
pressure discovered in R1B without claiming exact Minecraft registry bytes.

### `large_tail`

A publication containing one near-max-frame body among smaller frames. Exercises exact capacity and
resume behavior.

### `multi_connection_fanout`

Many independent cursors consume the same image under staggered drain patterns. Measures memory
amplification and shared-image value.

Once the real 26.2 Configuration contract exists, a separate source-backed fixture family should use
its exact admitted publication image in addition to these synthetic mechanism tests.

## Benchmark candidates

The benchmark should compare at least:

### A — per-connection rebuild/reference

Construct equivalent publication packet bodies for every connection, then queue/drain them normally.

### B — shared immutable bodies + cursor

Build the immutable publication image once outside the per-connection region, then measure complete
per-connection publication including ordinary framing copies and drains.

A later experiment may compare:

### C — shared preframed image / vectored publication

Only if profiling shows framing/copy remains material after B. C is **not** part of the initial
admission and must not bypass the ordinary egress/backpressure law merely because it is faster.

## Whole-cost accounting

The benchmark reports separate regimes rather than hiding costs:

### Preparation

- immutable image construction time;
- retained image bytes;
- any indexing/offset table bytes.

### Per connection

- setup time;
- per-connection owned bytes;
- allocations where measurable;
- bytes copied into intermediate semantic/publication storage;
- bytes copied into final egress;
- complete publication wall time under the declared drain schedule;
- useful payload bytes and final framed bytes.

### Fan-out

For N concurrent logical connections, report:

- total retained bytes;
- bytes attributable to the one shared image;
- bytes attributable per connection;
- total core time to publish the same image N times.

An optimization cannot win by excluding image construction if production rebuilds the image at the
same frequency. Conversely, a version/composition-stable image should not be rebuilt inside every
per-connection timed region if production builds it once.

## Measurement protocol

The benchmark follows `PERFORMANCE_QUALIFICATION_STANDARD.md`.

Production selection evidence must:

- run on controlled target hardware;
- record exact commit/toolchain/profile/machine state;
- separate preparation, warm-up and steady/fan-out regimes;
- use balanced/interleaved candidate order;
- retain raw samples;
- report p50/p95/p99/max where sample count supports them;
- record allocation/memory evidence where practical;
- report any tail caused by refill/compaction/resume boundaries.

Hosted GitHub Actions execute only smoke workloads. Hosted timing is diagnostic and cannot admit the
production mechanism.

## Initial acceptance hypotheses

The candidate is worth production integration only if semantic/resource qualification is completely
green and controlled measurements show a material whole-cost advantage appropriate to the added
mechanism.

Expected reasons to accept shared publication can include:

- elimination of per-client publication reconstruction;
- materially lower allocation count;
- materially lower retained memory at connection fan-out;
- materially lower CPU for repeated joins;
- simpler boundedness than a giant atomic batch;
- improved tail behavior under small egress windows.

A tiny timing win is not required if the candidate is also **simpler in resource semantics** and
removes large memory amplification. The decision record must state the actual reason.

## Production admission constraints

If the candidate graduates from the lab, the production PR must still prove:

1. existing atomic inbound-response semantics are unchanged for ordinary actions;
2. publication has an explicit bounded progress API rather than pretending an inbound frame exists;
3. target state advances only after successful egress admission;
4. one service call has finite declared publication work;
5. no runtime packet registry/target lookup is introduced;
6. no second unbounded outbound queue exists;
7. R0 and R1A behavior remains byte/transaction exact;
8. the 26.2 target publication is generated or otherwise tied to admitted VAR/SEM/contract evidence;
9. a real unmodified 26.2 client reaches the R1B Play/bootstrap endpoint.

## CI role

CI must permanently prove:

- publication model tests pass;
- rollback/order/cross-connection/backpressure cases pass;
- benchmark builds in release mode;
- smoke mode emits a parseable artifact with exact byte/count checksums;
- production remains on the previously admitted mechanism until a separate admission PR explicitly
  changes it.

## Exit

The laboratory is complete when Crucible has enough evidence to either:

- admit a small bounded publication primitive with an independently reviewed production/equivalence
  PR; or
- reject the candidate and retain/rework the simpler reference mechanism.

A failed performance hypothesis is a valid result. Correctness and boundedness are not negotiable.