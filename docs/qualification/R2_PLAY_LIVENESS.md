# R2 Play Liveness Semantics and Qualification

**Status:** implementation/qualification contract for R2A session liveness  
**Target semantic source:** official Minecraft Java 26.2 source archive pinned by Crucible  
**Source archive SHA-256:** `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`  
**Implementation:** `crucible-session-core::LivenessState`  
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

`LivenessState` therefore owns no clock, socket, task, packet identifier or scheduler node. The
caller supplies monotone milliseconds and target timing policy.

## 26.2 source observations

The relevant source frontier is
`net.minecraft.server.network.ServerCommonPacketListenerImpl`.

The pinned 26.2 source establishes the following observable laws:

1. listener construction anchors keep-alive time to the current millisecond time and carries
   latency from the common-listener cookie;
2. the keep-alive interval is exactly 15,000 ms;
3. when the interval is due and a challenge is already pending, the dedicated-server route
   disconnects for timeout;
4. otherwise, if the listener is not closed, the challenge is the current millisecond timestamp,
   that timestamp becomes the new anchor, pending becomes true, and one clientbound keep-alive is
   sent;
5. a response is accepted only when a challenge is pending and its ID is exactly equal to the
   challenge;
6. accepted latency is computed from elapsed milliseconds using Java `int` narrowing followed by
   `(old * 3 + elapsed) / 4`, then pending is cleared;
7. a wrong or unexpected response disconnects the non-singleplayer-owner route;
8. `close()` records only the first close timestamp;
9. the closed-listener timeout is exactly 15,000 ms;
10. the pending-challenge timeout branch has priority over the closed-listener branch;
11. a closed listener never publishes a new keep-alive challenge.

The integrated-server owner exception is target/application policy, not a reason to weaken exact
reply admission in the reusable state machine.

## Crucible semantic rules

### SEM-NET-R2-LIVE-001 — explicit monotone time

Liveness state is deterministic for a sequence of caller-supplied monotone millisecond timestamps.
It performs no hidden wall-clock reads.

### SEM-NET-R2-LIVE-002 — exact challenge lifecycle

At a live keep-alive deadline with no pending challenge:

```text
challenge = now_ms as signed 64-bit
anchor    = now_ms
pending   = true
result    = IssueChallenge(challenge)
```

Before the deadline the result is `Idle`.

### SEM-NET-R2-LIVE-003 — pending timeout priority

At or after the next keep-alive deadline, a still-pending challenge yields
`KeepAliveTimedOut`. This remains true if the listener has subsequently entered closed state.

### SEM-NET-R2-LIVE-004 — exact response admission

A response is accepted iff:

```text
pending && response_id == pending_challenge
```

A rejected response cannot partially mutate the liveness state.

### SEM-NET-R2-LIVE-005 — latency smoothing

For an accepted response:

```text
elapsed_i32 = Java-int-narrow(now_ms - challenge_issue_ms)
latency     = Java-int-wrap(latency * 3 + elapsed_i32) / 4
pending     = false
```

The response time does not replace the keep-alive anchor. The next challenge remains scheduled from
the previous challenge issue time.

### SEM-NET-R2-LIVE-006 — terminal listener

The first close records `closed_since_ms`; repeated close requests are idempotent. No new challenge
may be published after close. With no pending challenge, the next observable terminal action occurs
only when the source keep-alive gate and closed-listener timeout permit the closed timeout branch.

### SEM-NET-R2-LIVE-007 — target policy separation

The generic state machine reports `Rejected`, `KeepAliveTimedOut` and `ClosedTimedOut`. The target
or server layer decides the resulting disconnect behavior and any integrated-server-owner exception.

## Implementation invariants

`LivenessState` is currently asserted to occupy 32 bytes. It stores only the semantic state required
for exact liveness decisions: keep-alive anchor, pending challenge, close timestamp, smoothed latency
and compact flags.

The state exposes `next_deadline_ms(policy)`. This is an architectural contract: future production
scheduling must be able to keep inactive sessions out of the simulation-tick scan.

The generic crate does **not** hardcode Minecraft's 15,000 ms values. `LivenessPolicy` is supplied by
the target binding; this keeps the session core target-neutral while still allowing the 26.2 target
to bind exact source values.

## Tests

The permanent unit suite includes:

- exact before/at-deadline behavior;
- pending challenge timeout;
- exact-ID acknowledgement and wrong/unexpected reply rejection;
- source-compatible 3:1 latency smoothing;
- acknowledgement not resetting the challenge schedule;
- close idempotence and challenge suppression;
- pending timeout priority after close;
- backwards/out-of-range time failure without mutation;
- invalid policy rejection;
- a deterministic 100,000-event differential trace against an independently written reference
  model.

The differential model is intentionally structurally different from `LivenessState`; it exists to
catch state packing or branch-order mistakes rather than simply invoking the production methods
again.

## Performance hypothesis

A naive implementation scans every connected session every server tick:

```text
work ~= connected_sessions * tick_frequency
```

For an idle connection whose liveness state can produce an action only every 15 seconds, almost all
of those calls are known no-ops.

The candidate architecture exposes the exact next deadline. A scheduler can therefore maintain an
active deadline frontier and service only sessions whose liveness state can currently change.
Incoming keep-alive acknowledgements remain necessary work in both designs.

## Benchmark

Run:

```bash
cargo run --release --locked \
  --package crucible-client-spine-qualification \
  --bin liveness_deadline_bench -- \
  --full \
  --output qualification-results/network/liveness-deadline.json
```

The harness compares two scheduling topologies over the same `LivenessState` semantics:

- **scan reference:** every resident session is serviced every 50 ms;
- **deadline candidate proxy:** session service is invoked only at exact liveness deadlines.

Both receive the same deterministic acknowledgements. `Idle` polls are intentionally checksum
neutral because their absence is the optimization under test. Every externally visible liveness
decision and acknowledgement contributes to the semantic checksum, which must match before timing
is accepted.

The artifact records:

- raw paired timing samples;
- semantic checksum;
- scan service-call count;
- deadline service-call count;
- acknowledgement call count;
- hardware/toolchain metadata;
- `scheduler_mechanism_selected=false`.

## What this benchmark does not decide

The deadline candidate is a scheduling-topology proxy. It intentionally excludes the concrete data
structure used to maintain deadlines.

Therefore this benchmark can demonstrate the opportunity created by `next_deadline_ms`, but it
cannot select a production timer heap, timing wheel, intrusive queue, bucketed frontier or other
scheduler mechanism.

Before a deadline container is admitted, a separate whole-cost qualification must include:

- insertion/removal/reschedule cost;
- memory per connection;
- cancellation/disconnect churn;
- synchronized deadline bursts;
- jitter and fairness;
- 1/2/4+ worker ownership topology;
- interaction with network wakeups;
- tail latency under large idle and active populations.

Hosted CI timings remain diagnostic only and cannot select the production scheduler.

## Wire binding status

Independent 26.2 protocol metadata currently corroborates:

```text
clientbound Play keep_alive: 0x2c
serverbound Play keep_alive: 0x1c
payload: one i64
```

These values are **not yet production-admitted by this document**. The production target binding
requires the corresponding pinned official-source registration/codec evidence (or generated facts
whose provenance closes that source chain). Until then the liveness state machine is intentionally
usable without packet-number constants.

## Exit gate for R2A liveness substrate

This substrate may merge when:

1. workspace format/check/strict Clippy are green;
2. all existing protocol/network/world tests remain green;
3. the 100,000-event reference differential test is green;
4. the deadline benchmark executes in release mode with semantic checksum equality;
5. the benchmark becomes a permanent CI smoke with no hosted timing threshold;
6. source/semantic/benchmark documentation remains synchronized with the implementation.

Wire integration is a following gate and must not be smuggled into this merge without exact packet
registration/codec evidence.
