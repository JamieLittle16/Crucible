# R2 Play Liveness Semantics

**Target:** Minecraft Java Edition 26.2 / protocol 776  
**Status:** canonical semantic contract for R2 Play liveness and its finite 26.2 wire binding

These rules separate Minecraft-observable liveness behavior from Crucible's scheduling mechanism. A production scheduler may use a heap, timing wheel, bucketed frontier, readiness integration, or another qualified mechanism; none of those choices may alter these rules.

## Behavioral semantics

**SEM-NET-R2-LIVE-001 — explicit time input.** Crucible liveness decisions are deterministic for caller-supplied monotone millisecond time. The reusable semantic state performs no hidden wall-clock read.

**SEM-NET-R2-LIVE-002 — challenge lifecycle.** When the live challenge deadline is due with no pending challenge and publication is otherwise permitted, the challenge is the current millisecond timestamp represented as signed 64-bit state, the challenge anchor becomes that timestamp, pending becomes true, and exactly one challenge is requested for publication. Before the deadline there is no liveness action.

**SEM-NET-R2-LIVE-003 — pending timeout priority.** If the next keep-alive deadline is reached while the previous challenge remains pending, the observable result is keep-alive timeout. This branch retains priority if the listener has subsequently entered closed state.

**SEM-NET-R2-LIVE-004 — exact response admission.** A response is accepted iff a challenge is pending and the response ID exactly equals that pending challenge. Rejection does not partially mutate liveness state.

**SEM-NET-R2-LIVE-005 — latency smoothing.** On an accepted response, elapsed time is narrowed with Java `int` semantics and latency becomes `(old_latency * 3 + elapsed) / 4` with Java-compatible integer behavior; pending then clears. A response does not move the challenge anchor.

**SEM-NET-R2-LIVE-006 — closed listener.** The first close records the close timestamp and repeated close requests are idempotent. Closed state suppresses new challenges. With no pending challenge, the closed-listener timeout follows the source gate and timeout policy.

**SEM-NET-R2-LIVE-007 — policy boundary.** The reusable state machine reports rejected response, keep-alive timeout, and closed-listener timeout as semantic decisions. Product/target policy owns the resulting disconnect behavior and the integrated-server-owner exception.

## 26.2 target policy

**SEM-NET-R2-LIVE-008 — 26.2 timeout intervals.** For the ordinary dedicated-server route in Minecraft 26.2, both the keep-alive interval and the closed-listener timeout are exactly `15_000` milliseconds.

## 26.2 Play wire binding

**SEM-NET-R2-LIVE-009 — clientbound keep-alive wire.** In Minecraft 26.2 Play clientbound, keep-alive has packet ID `0x2c`. Its complete packet payload is one signed 64-bit challenge encoded in network byte order, with no additional fields.

**SEM-NET-R2-LIVE-010 — serverbound keep-alive wire.** In Minecraft 26.2 Play serverbound, keep-alive has packet ID `0x1c`. Its complete packet payload is one signed 64-bit response ID encoded in network byte order, with no additional fields.

## Mechanism freedom

These rules do not require Mojang's listener object graph, runtime packet registry, per-tick polling, task-per-connection timers, or any particular scheduler. Crucible may precompute packet identities, expose exact semantic deadlines, batch readiness, and use bounded transactional publication so long as the observable rules above are preserved.
