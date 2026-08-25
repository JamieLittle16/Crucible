# R2B Play Bootstrap Core Qualification

Status: **foundation candidate; target Play-entry semantics still source-gated**  
Target milestone: R2B replay-free Play bootstrap  
Predecessors: [`BOUNDED_PUBLICATION_PRIMITIVE.md`](BOUNDED_PUBLICATION_PRIMITIVE.md), [`R2_PLAY_LIVENESS.md`](R2_PLAY_LIVENESS.md)

## Purpose

R2B must replace captured Play bootstrap with Crucible-owned semantic projection without turning the
replacement into a giant per-join packet list or another output queue.

The target-neutral publication requirement is:

```text
caller-owned ordered semantic stages
             +
caller-owned immutable packet bodies for the current target
             +
small per-connection progress
             +
existing ConnectionDriver bounded egress
```

`crucible-publication-core::StagedPublicationCursor` supplies only the progress component.

It deliberately does **not** define Minecraft bootstrap stages, packet identities, codecs, player
state, world state, target-version order, scheduler policy, sharing policy, or publication-image
ownership.

## Why this belongs in publication-core

The reusable mechanism is not Minecraft-specific. It is bounded progression through multiple ordered
immutable publications while preserving the existing candidate/admit/commit law.

A bootstrap-only crate would create a permanent component boundary around a temporary milestone.
Instead:

- `crucible-publication-core` owns generic bounded progression;
- the 26.2 target will own the source-admitted semantic stage sequence and wire projection;
- client/session state owns protocol transactions such as teleport acknowledgement;
- R2C world projection supplies world/chunk/light output through an explicit later stage without
  moving world representation into networking.

## Production shape

```text
StagedPublicationCursor
    stage: usize
    body: PublicationCursor
```

The cursor is exactly two machine words and owns no packet bytes.

One service call performs exactly one of:

1. queue one body from the current stage through existing bounded egress;
2. commit exactly one completed stage boundary and reset the body cursor;
3. report the whole plan complete.

There is no loop over stages. An empty stage consumes one explicit service opportunity. This makes
fairness/work accounting visible to the eventual runtime and prevents a malformed or unexpectedly
large stage plan from creating an unbounded synchronous send loop.

## Transaction law

For current `(stage, body)`:

- if the stage does not exist, return `Complete` and mutate nothing;
- otherwise delegate the current stage body slice to the already-qualified `publish_one` primitive;
- if egress/wire admission rejects a body, change neither staged progress nor existing egress;
- if one body is admitted, advance only the body cursor;
- if the current body cursor is complete, advance exactly one stage, reset body progress to zero,
  and queue no body in that same service opportunity.

The stage increment cannot overflow after a successful stage lookup: `stage < stages.len() <=
usize::MAX` implies `stage != usize::MAX`.

## R2B semantic boundary

This mechanism intentionally does not freeze the source order of Play entry before the finite 26.2
Play-entry source gate is closed.

Existing source review already establishes the broad selected-profile frontier: fresh placement
includes mandatory entry/player state, permission/commands, recipe state, teleport/player position,
player-info and level metadata; selected empty/default branches eliminate some scoreboard/effect work;
world/chunk/light projection remains a later R2C concern.

The eventual target-specific stage plan must be derived from admitted SEM/VAR evidence. Stage labels
are semantic publication boundaries, not packet names and not a runtime packet registry.

## Test qualification

The staged primitive carries tests for:

- exact two-machine-word cursor size;
- stable empty-plan completion;
- no hidden skipping across multiple empty stages;
- body progress reset only after explicit stage completion;
- egress backpressure preserving both staged cursor and pre-existing queued bytes;
- exhaustive ordered publication across every four-stage shape with 0..=3 bodies per stage
  (`4^4 = 256` complete plans), checking every body is observed exactly once and in stage/body order;
- one explicit stage-completion event for every supplied stage, including empty stages.

The underlying `publish_one` tests continue to cover oversized-body rejection and single-publication
queue/cursor rollback.

## Performance claim boundary

No timing benchmark is introduced for the staged cursor itself.

There is no competing mechanism yet: the primitive is two counters plus one bounded call and stage
boundary branch. A hosted microbenchmark would not answer an engineering decision and would invite
false precision.

R2B **will** benchmark mechanism choices when they become real, especially:

- pre-encoded immutable stage images versus on-demand target materialization;
- shared process/composition bodies versus per-connection construction;
- body-at-a-time versus qualified bounded batching/vectored publication;
- static/common bootstrap projection versus player-specific projection;
- cache retention versus reconstruction whole cost.

Any such tournament must include allocations, copied bytes, memory retention, p50/p95/p99 service
cost, backpressure behavior and whole-join cost, not only packet serialization throughput.

## Exit for this slice

This foundation slice is complete when:

1. workspace format/check/strict Clippy/tests/rustdoc are green;
2. existing publication/Configuration qualification remains green unchanged;
3. the new exhaustive staged tests are green;
4. review confirms no target packet identity, Minecraft stage order, world state or second queue has
   entered `crucible-publication-core`.

After that, the next R2B slice closes the finite 26.2 Play-entry semantic/wire frontier and supplies
the first target-owned stage plan. Captured world/chunk/light bytes remain quarantined until R2C.
