# Pre-play Proactive I/O Service Law

Status: **production networking composition primitive**  
Scope: target-neutral bounded I/O scheduling for `PrePlayPublisher` targets

## Purpose

`PrePlayConnection::service_publication` establishes the ownership/commit law for one proactive
publication step. It intentionally does not decide when a transport adapter should attempt that work.

The ordinary `PrePlayIo::service_once` path remains the correct path for inbound-driven Status/Login
traffic and is unchanged. `service_once_with_publication` is the opt-in I/O law for targets which also
implement `PrePlayPublisher`.

## Ordering law

One publication-aware service call has the following deterministic order:

```text
write already-queued egress once
        |
process buffered inbound actions under ActionBudget
        |
        +-- action budget/closed --> final write --> return
        |
     Incomplete
        |
try exactly one proactive publication step
        |
        +-- Progress --> account one action --> final write --> yield
        |
       Idle
        |
read transport at most once
        |
process newly completed inbound actions under remaining ActionBudget
        |
     Incomplete
        |
try exactly one proactive publication step
        |
        +-- Progress --> account one action --> final write --> yield
        |
       Idle
        |
final write once --> return
```

The adapter never performs two proactive publication commits in one service call, even when the
caller supplied a larger action budget.

## Why inbound work has priority

Already-buffered complete inbound packets have already consumed socket/ingress resources and may be
required to unlock the next source-faithful publication stage (for example, a Configuration
negotiation response). They therefore retain the existing `process_limit` priority.

Proactive work is attempted only when that processing reaches `Incomplete`. This also means the
adapter never performs another transport read merely to discover work when the target already knows
it has server-initiated output ready.

## Budget law

Every successful publication state commit consumes one `ActionBudget` unit:

- `PublicationStep::Queued` consumes one action and admits exactly one outbound frame;
- `PublicationStep::Complete` consumes one action and admits zero frames.

Counting `Complete` is mandatory. A target may use completion to leave one publication stage and
enter another without emitting bytes; if that transition did not consume budget, malformed target
state could create an unbounded zero-byte loop.

If the publication commit consumes the last available action, the stop reason is
`ActionBudgetExhausted`. Otherwise the adapter returns `PublicationProgress`, explicitly recording
that it yielded after one proactive commit despite spare budget.

## Backpressure law

Existing queued egress always receives the first write opportunity. If it remains queued, the adapter
returns `OutputPending` before target processing or publication.

After a new publication frame is admitted, the existing final write opportunity is used. A partial or
pending write changes the stop reason to `OutputPending`. No later publication step can commit until
a future service call has drained the prior egress far enough for the normal service order to reach
target work again.

This preserves the invariant:

```text
bounded egress admission != socket completion
```

Publication state advances when the frame enters the already-qualified bounded egress, not when the
kernel later accepts every byte. Partial writes therefore cannot duplicate a publication body.

## Resource and dispatch shape

The publication-aware scheduler adds no publication-owned heap state. Its production path contains no
`Vec`, `Box`, `Arc`, `Rc`, `String`, trait object, runtime target registry, or second outbound queue.
Publication bytes remain borrowed through `PrePlayPublisher`; the only per-call scheduling state is
stack-local counters and small `Copy` report/stop values.

The target and publication capability are statically bound generic parameters. Calls are ordinary
monomorphised Rust dispatch, not virtual dispatch. The existing inbound-only `service_once` function
is unchanged and contains no publication readiness branch, so R0/R1A do not pay for a capability they
cannot use.

The only packet-body data movement introduced by publication is the already-qualified
`ConnectionDriver` frame admission into its existing bounded egress. This layer does not copy bodies
into an intermediate publication queue or staging image.

These are architectural/resource invariants, **not a throughput claim**. CI benchmark smoke verifies
that benchmark machinery and the existing Configuration publication laboratory remain runnable; a
performance winner for the eventual 26.2 Configuration image representation still requires the
controlled target-hardware qualification defined by the Performance Qualification Standard.

## Why the service laws remain separate

There is intentional structural overlap between the inbound-only and publication-aware service
choreography. A generic runtime scheduler abstraction was rejected here because it would enlarge the
ordinary R0/R1A proof and maintenance surface for a capability those paths never possess.

The duplication is bounded to one explicit child module, while the lower-level read, write,
transactional inbound processing, publication admission, buffer accounting and target state machines
remain shared production primitives. If later evidence shows the two service laws evolve in lockstep,
a private statically-specialised common engine can be qualified then; this PR does not introduce that
abstraction speculatively.

## Separate report type

The ordinary `ServiceReport` / `ServiceStop` contract is intentionally unchanged. R0/R1A cannot
produce proactive publication and should not be forced to acknowledge a stop state they can never
observe.

The opt-in path uses `PublicationServiceReport` / `PublicationServiceStop`, adding only
`PublicationProgress` to the existing transport/budget/terminal vocabulary.

This is deliberate compatibility isolation rather than a generic reporting abstraction.

## Qualification

Permanent integration tests use the real `ConnectionDriver` framing and a synthetic target which
enters Configuration through admitted generic session transitions. They prove:

- newly activated publication runs after inbound Configuration entry and before a second transport
  read;
- a large action budget still permits at most one proactive step per service call;
- inbound actions can exhaust the budget and defer publication to the next call;
- zero-byte `Complete` consumes budget and cannot spin;
- existing egress/partial-write backpressure prevents a second publication commit; and
- queued publication bytes are emitted through the same connection egress as ordinary responses.

## R1B boundary

This primitive contains no Minecraft 26.2 packet identity, Configuration field law, registry/tag
content, publication ownership choice, spawn state, or Play semantics.

Only after the Configuration source gate and finite protocol contract are green may `Target26_2`
implement `PrePlayPublisher` and may product composition choose this service path for R1B.
