# R2 Play Liveness Semantics and Qualification

**Status:** source-admitted and qualified R2A session-liveness contract  
**Target:** Minecraft Java 26.2 / protocol 776  
**Source archive SHA-256:** `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`  
**Semantic mechanism:** `crucible-session-core::LivenessState`  
**26.2 wire/policy binding:** `crucible-target-26-2::play_liveness`  
**Finite protocol contract:** `PROTO-NET-PLAY-LIVENESS-26-2-001`  
**Performance harness:** `liveness_deadline_bench`

## Purpose

A connected Play session needs a liveness mechanism that preserves Minecraft 26.2 behavior without
forcing the server to poll every resident connection on every simulation tick.

The semantic mechanism and its scheduling mechanism are deliberately separate:

```text
source-backed liveness state
        |
        | next_deadline_ms
        v
scheduler-selected deadline frontier
        |
        v
only due sessions are serviced
```

`LivenessState` owns no clock, socket, task, packet identifier or scheduler node. The caller supplies
monotone milliseconds and target timing policy. Minecraft-version-specific packet identity, wire
encoding and timing constants are owned by the 26.2 target rather than product composition.

## Source-admitted 26.2 laws

The final R2A source frontier contains twelve exact fingerprint-pinned bodies:

- the Play registration order in `GameProtocols`;
- each keep-alive packet's `STREAM_CODEC` root;
- each direction's exact `FriendlyByteBuf` decoding constructor;
- each direction's exact writer;
- the common listener constructor, `close`, closed gate, liveness service and reply handler.

The frontier was deliberately expanded from eight bodies after review showed that the two codec roots
proved only delegation. Admission therefore also fingerprints the delegated read/write bodies; the
finalizer rejects the old eight-body shape.

The reviewed registration order derives the exact Play identities:

```text
clientbound keep-alive = 0x2c
serverbound keep-alive = 0x1c
```

The reviewed decoding constructors each consume exactly one `readLong()`, and the reviewed writers
each emit exactly one `writeLong(id)`. There are no additional payload fields. The listener bodies
establish the 15,000 ms interval/timeout, pending-timeout branch priority, challenge timestamp law,
exact response admission, Java-compatible latency smoothing, first-close timestamp and ordinary
dedicated-server invalid-response policy.

No Mojang source text is committed. The source-rich dossier remains ephemeral; committed
`VAR_REVIEWED` records contain only fingerprints, reviewed hazards, semantic links and review notes.

## Canonical semantic rules

The normative rules live in `vanilla/semantics/network/R2_PLAY_LIVENESS_SEMANTICS.md`:

- `SEM-NET-R2-LIVE-001` — explicit caller-supplied monotone time;
- `SEM-NET-R2-LIVE-002` — exact challenge lifecycle;
- `SEM-NET-R2-LIVE-003` — pending timeout priority;
- `SEM-NET-R2-LIVE-004` — exact response admission;
- `SEM-NET-R2-LIVE-005` — Java-compatible latency smoothing;
- `SEM-NET-R2-LIVE-006` — closed-listener lifecycle;
- `SEM-NET-R2-LIVE-007` — target/product disconnect-policy boundary;
- `SEM-NET-R2-LIVE-008` — exact 15,000 ms 26.2 intervals;
- `SEM-NET-R2-LIVE-009` — clientbound `0x2c` + signed 64-bit payload;
- `SEM-NET-R2-LIVE-010` — serverbound `0x1c` + signed 64-bit payload.

The integrated-server-owner exception remains product/target policy and does not weaken exact reply
admission in the reusable state machine.

## Implementation invariants

`LivenessState` is asserted to occupy 32 bytes. It stores only the semantic state required for exact
liveness decisions: keep-alive anchor, pending challenge, close timestamp, smoothed latency and
compact flags.

The state exposes `next_deadline_ms(policy)`. Future production scheduling must therefore be able to
keep inactive sessions out of a simulation-tick scan.

The generic session crate does not hardcode Minecraft's timing values. The 26.2 target exports the
source-admitted `PLAY_LIVENESS_POLICY` and owns finite keep-alive encode/decode functions. Both packet
bodies are stack-sized nine-byte values and require no packet allocation or runtime registry lookup.

Product composition does not own the 26.2 packet IDs, byte order or timing constants. It asks the
target to encode a challenge or decode a finite liveness reply, then applies the target-neutral
semantic state transition.

Challenge publication remains transactional:

```text
copy 32-byte liveness state
        -> service candidate
        -> encode 9-byte target body
        -> bounded egress admission
        -> commit candidate only on success
```

Backpressure therefore cannot create a phantom pending challenge.

## Protocol contract and generated facts

`vanilla/protocol/PROTO-NET-PLAY-LIVENESS-26-2-001.json` is a finite two-packet contract backed by the
twelve reviewed VARs. It carries deterministic golden bodies/frames using challenge
`0x0102030405060708`.

`tools/protocol_codegen.py` generates
`crates/network/crucible-target-26-2/src/generated/play_liveness_26_2.rs` from that contract. The
committed generated file contains compile-time identities only; production builds do not need a
runtime packet registry. Qualification-only golden bytes are behind `cfg(test)`.

`tools/tests/test_target_26_2_generated.py` regenerates the file from the contract and committed VARs,
requires byte-for-byte equality, and pins the generated file SHA-256. Editing packet constants or
golden bytes independently of the admitted evidence therefore fails CI.

## Tests

The permanent semantic suite covers exact deadline boundaries, pending timeout, exact-ID reply,
wrong/unexpected reply rejection without partial mutation, Java-compatible 3:1 latency smoothing,
response timing not resetting the challenge anchor, close idempotence, challenge suppression after
close, pending-timeout priority, invalid time/policy handling, and a deterministic 100,000-event
differential trace against an independently written reference model.

The 26.2 target additionally tests generated golden bodies against its allocation-free stack encoders,
exact serverbound golden-frame decoding, malformed-payload rejection and exact source-admitted timing
policy.

The R1X live integration keeps the existing bounded-driver regression tests, including deliberate
egress backpressure proving candidate state is not committed when challenge publication cannot be
admitted, fragmented replies, wrong/malformed replies and ten consecutive fake-time cycles through
one live driver.

## Performance qualification

The `liveness_deadline_bench` compares a 50 ms resident-session scan against servicing only exact
liveness deadlines over the same semantic state and deterministic acknowledgements. Externally
visible semantic checksums must match before timing is considered.

The benchmark records raw paired timings, scan/deadline service-call counts, acknowledgement work and
hardware/toolchain metadata. Hosted CI asserts semantic equality and structural work reduction but has
no timing threshold.

This benchmark proves the opportunity created by `next_deadline_ms`; it does **not** select the final
deadline container. Heap, timing wheel, bucketed frontier, intrusive queue or another mechanism still
requires a separate whole-cost contest including memory, churn, synchronized bursts, worker topology,
network wakeups and tail latency.

## Real-client evidence

The R2A milestone was exercised with an unmodified Minecraft Java 26.2 client after the finite R1X
visible-world bootstrap. Crucible then owned continuing Play liveness and accepted ten consecutive
live keep-alive replies before normal peer EOF:

```text
LivePeerEof { accepted_keep_alives: 10, latency_ms: 0 }
```

The localhost `0 ms` value is millisecond-resolution measurement, not a claim of zero physical
latency. No keep-alive timeout or invalid-response exit occurred.

## R2A closure

R2A liveness is closed when the exact source-admission/codegen head passes workspace format/check,
strict Clippy, all Rust/Python tests, protocol/network qualifications, benchmark smokes, rustdoc and
the runnable server gate. The overall 26.2 target remains explicitly experimental beyond its admitted
finite surfaces; this contract does not claim general Play or world simulation support.
