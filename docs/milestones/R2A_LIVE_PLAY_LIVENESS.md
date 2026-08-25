# R2A Live Play Liveness Evidence

**Status:** achieved experimental R2A integration proof  
**Target:** Minecraft Java Edition 26.2, protocol 776  
**Qualified branch head:** `d4fefbef7b71e33148442aa3e9feb443f9b7c211`  
**Production admission:** **false**

## Result

An unmodified Minecraft Java 26.2 client completed the existing Crucible Login and Configuration
route, received the finite R1X visible-world bootstrap, crossed the explicit empty-queue handoff into
Crucible-owned live Play liveness, and then acknowledged ten consecutive live keep-alive challenges.
The live connection ended by peer EOF rather than timeout or invalid-response rejection.

Observed server result:

```text
R1X connection ... completed: LivePeerEof { accepted_keep_alives: 10, latency_ms: 0 }
```

The same run also contained short `SessionClosed` connections. Those did not enter the live-Play
liveness route and are not failures of the qualified live connection.

`latency_ms: 0` is valid for this localhost test. The source-compatible latency calculation operates
at millisecond resolution, so a sufficiently short local round trip may narrow to zero.

## What this proves

R2A establishes that Crucible now owns a continuing part of Play rather than relying entirely on a
finite capture:

```text
real Login
    |
real Configuration
    |
finite R1X visible-world bootstrap
    |
empty user-space ingress + egress handoff
    |
Crucible live Play controller
    |
semantic keep-alive deadlines
    |
Crucible-generated challenge
    |
stock-client exact reply
    |
transactional acknowledgement + latency update
    |
repeat
```

The proof covers ten successful repetitions through the real TCP transport and stock client. No
`LiveKeepAliveTimedOut` or `LiveInvalidKeepAlive` result occurred.

## Architectural properties exercised

The live integration retains the permanent R2 architecture:

- liveness semantic state is the compact target-neutral `LivenessState`;
- no simulation-tick scan is required by the semantic API;
- the next semantic wake is exposed by `next_deadline_ms`;
- the blocking R1X adapter uses socket-read timeout only as a development deadline wake mechanism;
- the adapter does not select the future fleet deadline container or scheduler;
- live ingress and egress remain bounded by `ConnectionDriver`;
- the finite pre-play/R1X driver is dropped only after Play is reached, replay is complete, and both
  user-space queues are empty;
- only one Crucible connection queue is alive at a time;
- challenge state uses candidate -> bounded-egress-admit -> commit, so failed egress admission
  cannot create a phantom pending challenge;
- keep-alive packet bodies are stack values rather than heap packet objects;
- malformed, unexpected, or wrong-ID replies fail closed without partial liveness-state mutation.

## Automated qualification at the proven head

GitHub CI for `d4fefbef7b71e33148442aa3e9feb443f9b7c211` completed green, including:

- repository guard;
- workspace format;
- `cargo check --workspace --all-targets --all-features --locked`;
- strict workspace Clippy with `-D warnings`;
- protocol wire and packet-body qualification;
- connection buffer and driver qualification;
- bounded publication and pre-play qualification;
- session-state qualification;
- Minecraft 26.2 target qualification;
- client-spine integration qualification;
- fused outbound, Configuration publication, and liveness-deadline benchmark smokes;
- all Rust tests;
- composition HOT, executor, world-access, and spatial-address benchmark smokes;
- section semantic/source-backed qualification;
- Python tooling checks;
- rustdoc;
- the dedicated runnable server gate.

The live-server unit suite additionally covers exact keep-alive body/framing bytes, fragmented
responses, ten deterministic fake-time cycles, exact reply admission, latency smoothing, malformed
and wrong replies, and the transactional egress-backpressure invariant.

## Source and wire-admission boundary

The liveness semantics themselves are source-backed by the pinned Minecraft 26.2
`ServerCommonPacketListenerImpl` frontier and the R2 liveness qualification contract.

The experimental live route currently binds:

```text
clientbound Play keep_alive = 0x2c
serverbound Play keep_alive = 0x1c
payload = one signed 64-bit integer
```

The reviewed `GameProtocols#<clinit>()` source fingerprint establishes the authoritative Play
registration-order surface used to derive packet identities, and independent 26.2 protocol/capture
evidence corroborates these two values. However, the dedicated source-derived Play-liveness protocol
contract and generated target facts are not yet committed.

Therefore these wire identities remain **R1X experimental bindings** and `production_admitted=false`
remains correct. The next source-admission slice must commit the relevant VAR records, finite protocol
contract, generated Rust identities, golden bytes, and codegen drift check before these constants may
become permanent `Target26_2` Play facts.

## R2A conclusion

The experimental R2A criterion is met:

> A stock Minecraft 26.2 client can enter Play through Crucible and remain alive across repeated,
> genuinely Crucible-owned keep-alive transactions instead of dying when the finite replay ends.

This closes the live-liveness integration proof. It does **not** close replay-free Play bootstrap,
teleport/movement authority, Crucible-owned chunks/light, or production Play admission; those remain
the following R2 slices.
