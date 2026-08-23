# Pre-Play Target Binding

**Status:** Client P0 production boundary  
**Scope:** target-neutral Handshake / Status / Login / Configuration transaction adoption

## Purpose

Crucible's connection machinery is deliberately split into layers:

```text
socket/runtime adapter
        ↓
bounded connection bytes
        ↓
framed packet view
        ↓
source-backed target decoder
        ↓
candidate semantic action
        ↓
transactional outbound admission
        ↓
live session-state adoption
```

`crucible-preplay-core` owns the final three generic coordination steps. It contains no target packet
IDs, Minecraft-version field layouts, authentication policy, registry payloads or socket runtime.

The reason for making this a production boundary is simple: target packet handlers must not each
reimplement the subtle rule that inbound consumption, required outbound capacity and session-state
transition are one commit decision.

## Core law

A target decoder does **not** mutate the live session.

It receives:

- immutable target/runtime context;
- the current `SessionState` by value;
- one borrowed `FrameView`.

It returns an owned `PrePlayAction` containing:

- the complete outbound frame-body batch required by the action;
- a candidate `SessionState`.

The binder then validates the candidate and calls the already-qualified transactional connection
driver. Only after the complete outbound batch has been admitted and the exact inbound frame has
been consumed does the binder replace the live session with the candidate.

Therefore:

```text
TARGET DECODE
    ≠
LIVE STATE COMMIT
```

and:

```text
outbound capacity failure
→ no inbound consumption
→ no session transition
```

## Candidate transition validation

The target cannot use `SessionState` as a bypass around the lifecycle law.

For one committed packet action, the candidate must be exactly one of:

1. the unchanged current session;
2. one legal forward edge already admitted by `crucible-session-core`;
3. terminal closure.

The binder rejects skipped, backward or reconstructed session states before outbound admission.
Closed sessions are terminal and no further packet action is processed.

This matters because same-phase packets are normal—for example several Status packets can be
processed without changing phase—while lifecycle packets must still obey the exact one-way graph.

## Static target binding

`PrePlayConnection<T>` is generic over the target decoder type `T`.

There is no required:

- runtime packet registry;
- `dyn` handler dispatch;
- service locator;
- target map lookup;
- `Arc` / mutex / async dependency in this boundary.

Package/version selection can therefore remain a cold composition concern while packet handling is
statically specialized.

## Allocation and copying

Inbound packet payloads remain borrowed from the bounded ingress buffer. This binder does not copy
them.

Outbound ownership is deliberately delegated to the target action. A cold target can use ordinary
owned buffers. A HOT target may later use a more specialized/fused mechanism only after its
performance experiment wins. The binder does not force an intermediate representation.

## Failure semantics

The binder is fail-closed for:

- malformed generic wire input;
- target semantic/codec rejection;
- invalid candidate session transitions;
- bounded egress exhaustion;
- impossible ingress/egress rollback failures;
- terminal sessions.

No failure path adopts candidate session state.

## Permanent qualification

The crate's own tests use synthetic packet identities only and cover:

- every handshake frame split point;
- same-phase Status actions;
- exact response bytes;
- malformed/trailing payload rollback;
- invalid candidate-session rollback;
- all-or-nothing multi-packet response batches under egress pressure;
- the complete synthetic Login -> Configuration -> Play path;
- terminal closure;
- a 10,000-action fragmented Status reuse trace.

These tests are a permanent production contract, not temporary scaffolding for R0.

## R0 relationship

This boundary intentionally stops one step before Minecraft 26.2 facts.

The R0 path is:

```text
pinned 26.2 source + black-box capture
        ↓
reviewed VAR / SEM
        ↓
admitted status contract + golden bytes
        ↓
generated static packet IDs
        ↓
small target-specific decoder / encoder
        ↓
PrePlayConnection<Target26_2>
        ↓
bounded localhost socket adapter
        ↓
unmodified-client status + ping probe
```

No target constant may be introduced here merely to make that final integration arrive sooner.
