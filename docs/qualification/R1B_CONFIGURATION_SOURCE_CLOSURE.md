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

R1B uses two narrow Configuration gates rather than one oversized gate:

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

## Second-order closure audit

The first real closure preparation resolved and exposed **43 exact pinned bodies**. All 43 source
excerpts matched their recorded hashes and manual inspection found no contradiction with the current
R1B SEM contract. That review also found a smaller second-order invalidation gap: a few of those 43
bodies were themselves wrappers around the body that actually implements the claimed wire law.

Two examples are decisive:

- `ByteBufCodecs.list()` delegates the default maximum to the two-argument `collection(...)`
  overload, while the first closure only fingerprinted the three-argument bounded overload;
- `FriendlyByteBuf.readMap(keyDecoder, valueDecoder)` delegates the actual count/allocation/entry
  loop to the three-argument `readMap(ctor, keyDecoder, valueDecoder)` overload.

The same audit followed the UTF/identifier/optional-Tag chain rather than stopping at the first helper.
The hardened plan therefore contains **55 candidates**. The 12 added candidates bind only the missing
second-order bodies:

- two-argument default `ByteBufCodecs.collection`;
- three-argument `FriendlyByteBuf.readMap`;
- default and bounded `FriendlyByteBuf` UTF delegation;
- `FriendlyByteBuf.writeEnum` for ClientInformation encode symmetry;
- the underlying bounded `Utf8String` reader/writer;
- optional-value codec construction used by packed registry entries;
- `Identifier.STREAM_CODEC` and relevant static `ByteBufCodecs` declarations;
- the network-safe registry subset used by tag serialization.

This is intentionally a bounded closure, not recursive source crawling. Already-admitted R1A
primitive evidence remains reused where it is exactly the same dependency; the plan does not create
duplicate VARs merely to inflate a candidate count.

## Closure selector law

`tools/r1b_configuration_source_closure.py` uses a two-level, fail-closed selector model.

The normal selector is:

```text
exact qualified Atlas type
+ exact method name
+ exact parameter count
```

That compact selector is accepted only when it resolves **exactly once**. It is intentionally not an
"arbitrary first overload" rule.

When the pinned source genuinely contains same-arity overloads, a plan candidate may additionally pin
an `exact_signature`. In that case discovery requires:

```text
exact qualified Atlas type
+ exact Atlas signature
```

and the resolved method name/parameter count must still agree with the candidate declaration. This
keeps overload refinement explicit and reviewable instead of encoding positional or source-line
assumptions.

Before opening the source archive for review excerpts, `prepare` preflights **every** candidate. All
missing or ambiguous selectors are reported together and no source-rich review artifact survives a
failed preflight. This avoids iterative first-error discovery and guarantees that a generated dossier
starts from a complete uniquely bound selector set.

After resolution, the generated gate always freezes the full resolved `qualified_name#signature`.
Thus exact-signature refinement improves discovery precision without weakening final admission.

Nested Java types use Atlas's canonical `$` qualified form, for example:

```text
PrepareSpawnTask$Preparing#tick
PrepareSpawnTask$Ready#spawn
```

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

Finalization is transactional: the complete worksheet, INDEXED record set and gate boundary are
validated before final output exists; reviewed artifacts are written through a sibling staging
directory and atomically renamed only after success. A rejected review leaves no partial final set.

It then emits `VAR_REVIEWED` records. Source admission still requires an independent
`tools/vanilla_source_gate.py` run against the pinned Atlas database.

## Generic codec reuse

This closure does not duplicate already-admitted R1A generic evidence merely because R1B uses it.
The admitted Login wire gate already binds the generic `ByteBufCodecs.readCount`,
`ByteBufCodecs.writeCount`, two-/three-field `StreamCodec.composite`, nullable helpers and UTF-8
factory evidence used again by Configuration. R1B fingerprints the Configuration-specific helper
chain and every new delegated body whose change could invalidate an R1B SEM claim.

The source distinction between accepted wire count and initial Java collection capacity remains an
important implementation boundary: Crucible must reproduce the wire law without reproducing
vanilla's allocation strategy. In particular, vanilla's bounded initial capacity is not a requirement
for Crucible's production representation.

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
