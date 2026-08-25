# R2B Play-Entry Source Gate

Status: **open; closure probe in qualification**  
Target: Minecraft Java Edition 26.2, protocol 776  
Gate: `GATE-NET-PLAY-ENTRY-26_2-001` (reserved; not yet admitted)

## Purpose

R2B replaces the non-world portion of the finite R1X Play replay with Crucible-owned semantic
bootstrap state and target projection. The source gate exists to ensure that this replacement follows
observable vanilla semantics without inheriting Mojang's runtime object graph, packet registry,
flushing architecture, or world representation.

The gate is intentionally finite. It covers only the selected fresh/default player entry profile.
Chunk/light publication and ongoing world tracking belong to R2C/R2D.

## Required evidence chain

```text
pinned Minecraft 26.2 source
        |
existing fresh-player placement review
        |
hardened delegate/codec closure probe
        |
one bounded exact-body source review
        |
VAR_REVIEWED records
        |
SEM-NET-R2B-PLAY-* rules
        |
GATE-NET-PLAY-ENTRY-26_2-001
        |
target-owned semantic stage plan + codecs
```

No packet identity, field order, default branch or mandatory stage may be admitted from replay
position alone.

## Existing reviewed facts carried forward as evidence leads

The historical first-pass and follow-up reviews already establish the selected-profile shape strongly
enough to bound the final review:

- fresh placement has a mandatory Play-entry control-flow spine;
- login/entry state, difficulty, abilities, selected slot and synchronized recipe state are emitted;
- permission/command state and initial recipe-book state are part of fresh-player publication;
- initial player-info state and an initial teleport/player-position transaction are client-visible;
- level metadata includes border/clock/default-spawn/load-start/tick state, with weather conditional;
- empty scoreboard/effect branches may produce no selected-profile traffic only when their default
  assumptions are explicitly bound;
- inventory-menu initialization is client-visible only through delegated synchronization and remains
  unclosed until that delegate path is reviewed;
- world/chunk/light streaming is not part of this gate.

These observations are not themselves the final R2B admission records.

## Why the old 27/35-body frontier is not directly canonicalized

Two evidence hazards remain:

1. `ServerPlayer::initInventoryMenu` delegates into menu synchronization. The actual synchronizer
   implementation and initial-data packet path must be identified and reviewed before inventory
   packet presence/payload can be frozen.
2. several packet `STREAM_CODEC` declarations may delegate outbound wire law to packet constructors
   or writers. R2A demonstrated that a codec root is not proof of a delegated `read`/`write` body.

The R2B closure probe therefore reports exact Atlas signatures for:

- inventory synchronizer setup, initial-data and change callbacks;
- mandatory selected-profile bootstrap packet classes and their available methods.

The final source-rich review must use exact-signature selectors wherever overloads exist and must
include every material outbound delegate needed to prove the selected wire law.

## Gate profile boundary

The first admitted profile is deliberately narrow:

- fresh player;
- ordinary non-transfer entry;
- offline/no remote chat-session branch unless later evidence/profile changes require otherwise;
- empty scoreboard;
- no active effects at entry;
- default/fresh inventory state as established by source;
- no world/chunk/light publication inside this gate.

Any richer persisted player, scoreboard, effect, transfer, online-chat or custom composition behavior
requires an explicit later SEM/profile expansion. It must not be smuggled into R2B through a generic
Mojang-shaped bootstrap object.

## Architectural admission rules

The final target implementation must preserve all of the following:

- semantic stage labels are not packet names and are not a runtime packet registry;
- `crucible-target-26-2` owns version-specific packet identities and wire codecs;
- `crucible-publication-core` remains target-neutral and owns only bounded progression;
- per-connection bootstrap progress remains compact and allocation-free in the ordinary path;
- immutable composition-stable bodies may be shared/precomputed when source semantics permit;
- player-specific bodies are projected from compact semantic state rather than reconstructed from a
  Mojang object graph;
- one service opportunity remains bounded; egress rejection cannot advance bootstrap progress;
- no second outbound queue is introduced;
- world/chunk/light state crosses an explicit later `WorldProjection` seam supplied by R2C.

## Required final review outputs

Before `GATE-NET-PLAY-ENTRY-26_2-001` may be marked admitted, the repository must contain only
source-free artifacts proving:

1. exact source fingerprints for every material selected-profile control-flow and outbound-codec
   body;
2. explicit hazard dispositions;
3. no unresolved delegate dependencies;
4. semantic rules for mandatory order, conditional/default-empty branches, teleport transaction and
   inventory bootstrap;
5. a finite target packet/codec contract or equivalent generated facts with codegen drift checks;
6. exact selected-profile stage ordering derived from those SEMs rather than from the capture.

Source-rich dossiers remain external and `EPHEMERAL_DO_NOT_COMMIT`.

## Test and performance requirements after gate closure

The first target implementation must carry:

- golden codec tests for every newly admitted packet surface;
- differential/reference tests for stage selection and conditional/default-empty branches;
- exhaustive/stress progression tests through `StagedPublicationCursor` under small egress limits;
- transactional tests proving backpressure does not skip/duplicate stages;
- teleport-ack state-machine tests once the initial teleport becomes live;
- real stock-client comparison against the R1X oracle while replay is progressively removed.

Timing benchmarks are required only where there is an actual mechanism choice. Likely first R2B
contests are shared immutable bootstrap images versus per-connection materialization and one-body
publication versus qualified bounded batching/vectored output. Whole-cost evidence must include
allocations, copied bytes, retained memory, tails and join latency.

## Exit

This source-gate phase exits only when the finite selected-profile source frontier has no unresolved
material delegate and the target stage plan can be generated or implemented without relying on
capture ordering or server-owned 26.2 packet constants.
