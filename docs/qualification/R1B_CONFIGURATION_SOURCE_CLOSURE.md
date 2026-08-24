# R1B Configuration Source-Closure Gate

Status: **supplemental evidence gate; production Configuration remains blocked**  
Tracker: #146  
Target: Minecraft Java 26.2 / protocol 776

## Why this gate exists

The first R1B review session correctly resolved 25 exact source candidates and established the direct
Configuration control-flow spine. Manual inspection then found that several detailed SEM claims were
supported by source bodies *behind* those first fingerprints.

Examples include:

- packet `STREAM_CODEC` declarations which delegate to private constructors/writers;
- `startConfiguration()` delegating task insertion/progression to helper methods;
- `PrepareSpawnTask.tick()` delegating the actual radius/load transition to the nested `Preparing`
  state;
- the ready spawn path delegating entity readiness and `placeNewPlayer` handoff to nested state;
- registry/tag envelopes delegating packed-entry and nested collection construction.

A wrapper-only fingerprint is insufficient when changing the delegated body could invalidate the
linked SEM while leaving the wrapper fingerprint unchanged.

The Crucible rule is therefore:

> A source gate must fingerprint the body whose change would invalidate the semantic claim, not
> merely a stable caller which happens to reach that body today.

## Evidence split

R1B now uses two narrow gates rather than one oversized gate:

```text
GATE-NET-CONFIG-26_2-001
  direct packet registration / packet envelopes / listener handlers / primary task spine

GATE-NET-CONFIG-CLOSURE-26_2-001
  delegated codec bodies / task helpers / registry-tag construction / prepared-spawn nested state
```

Production Configuration requires both gates. The second gate supplements rather than replaces the
25-entry core review.

The immediate Play bootstrap packet/codecs reached from `PlayerList.placeNewPlayer` remain a separate
R1B Play-entry closure. They must be source-admitted before the real-client R1 exit, but are not
silently pulled into this Configuration dependency gate.

## Closure selector law

`tools/r1b_configuration_source_closure.py` identifies each required source body by:

```text
exact qualified Atlas type
+ exact method name
+ exact parameter count
```

The local preparation step requires that tuple to resolve **exactly once**. It then freezes the
resolved full `qualified_name#signature` into the generated gate. Ambiguous or missing selectors fail
before a review artifact is created.

Nested Java types use Atlas's canonical `$` qualified form, for example:

```text
PrepareSpawnTask$Preparing#tick
PrepareSpawnTask$Ready#spawn
```

This avoids brittle hand-written generic signatures while keeping the final source gate exact.

## Source-text firewall

The prepare command must target a fresh path outside the repository. It writes:

```text
review-dossier.json      source-rich; EPHEMERAL_DO_NOT_COMMIT
review-worksheet.json    source-free
records/                 source-free INDEXED VAR drafts
gate/                    source-free exact-signature gate
manifest.json            session identity
```

`finalize` consumes only the source-free worksheet and INDEXED records. Every candidate requires:

- `source_inspected=true`;
- `accepted=true`;
- non-empty reviewer and review note;
- every Atlas hazard explicitly listed in `hazards_reviewed`;
- at least one SEM link from the candidate's declared allowed set.

It then emits `VAR_REVIEWED` records. Source admission still requires an independent
`tools/vanilla_source_gate.py` run against the pinned Atlas database.

## Generic codec reuse

This closure does not duplicate already-admitted R1A generic evidence merely because R1B uses it.
Existing UTF-8/count/composite evidence remains reusable. R1B adds the collection/list/map and
FriendlyByteBuf helper bodies which are materially part of the Configuration field laws and were not
covered by the earlier gate.

The source distinction between accepted wire count and initial Java collection capacity remains an
important implementation boundary: Crucible must reproduce the wire law without reproducing
vanilla's allocation strategy.

## Prepared-spawn boundary

The closure explicitly fingerprints the nested bodies which make the prepared-spawn capability real:

```text
Preparing.tick
  -> resolve spawn
  -> PLAYER_SPAWN radius-3 ticket/load
  -> wait until ready
  -> Ready

Ready.keepAlive
  -> renew radius-3 PLAYER_SPAWN ticket

Ready.spawn
  -> wait for entities at radius 3
  -> construct player from accepted profile/latest client information
  -> placeNewPlayer
```

Crucible preserves those readiness/order dependencies, not Mojang's Java task/state object graph.
A compact prepared-spawn capability remains the preferred target representation.

## Exit

This closure is complete when:

1. every selector resolves exactly once against the pinned 26.2 Atlas;
2. every source-rich candidate has been manually inspected;
3. every hazard and SEM linkage is explicitly dispositioned in the source-free worksheet;
4. `GATE-NET-CONFIG-CLOSURE-26_2-001` reports `admitted=true` with no failures.

Only after the core Configuration gate, this closure gate, and the minimum Play-entry closure are
green may source-derived packet identities and the selected Configuration state machine enter
`Target26_2` production code.
