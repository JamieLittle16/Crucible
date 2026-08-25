# R2B Play-Entry Projection Architecture

Status: **PROVISIONAL mechanism split; source-backed selected-profile admission in progress**  
Target: Minecraft Java Edition 26.2, protocol 776  
Parent gate: `GATE-NET-PLAY-ENTRY-26_2-001`

## 1. Purpose

R2B must remove the non-world portion of the finite R1X Play replay without turning Crucible into a runtime clone of Mojang's bootstrap object graph and serializer registries.

The final 67-body control-flow review plus the 117-body wire-closure review show that the selected fresh/default route naturally separates into three publication classes:

```text
A. composition-stable immutable projection
B. small semantic runtime codec
C. selected-profile specialization
```

The selected mechanism for each surface is allowed to differ. Vanilla defines the client-observable result and ordering; it does not require Crucible to reproduce Mojang's runtime construction path.

## 2. Class A — composition-stable immutable projections

### Command tree

The reviewed source establishes all of the following:

- permission filtering constructs a client-visible command graph;
- the clientbound command packet serializes a stable node envelope (flags, children, optional redirect, node kind, literal/argument payload and optional suggestion identifier);
- argument node serialization writes the registered argument-type identity and delegates the argument-specific payload to `ArgumentTypeInfo.serializeToNetwork(...)`;
- the exact result therefore depends on command composition, enabled feature/registry composition, suggestion-provider identities and the permission profile.

For the admitted fresh/default profile Crucible MUST NOT re-run this graph construction and generic serializer dispatch for every joining player.

The production candidate is an immutable `CommandProjectionArtifact` keyed by at least:

```text
Minecraft target
protocol contract revision
command composition revision
registry/argument-type composition revision
enabled-feature profile
permission profile
```

The artifact is generated/qualified outside the per-connection hot path and shared by every connection with the exact same key.

A key mismatch is not permission to reuse the artifact. It is a cache miss / unsupported profile until an equivalent projection exists.

The source review remains authoritative for the semantic construction/invalidation law. Exact bytes for a fixed admitted key may additionally be established by a pinned stock-server oracle and golden comparison. Replay ordering or bytes alone are never semantic authority.

### Synchronized recipes

The selected synchronized item-property and stonecutter publication is composition-stable for a fixed server data/recipe composition. It should likewise become an immutable shared projection, keyed by target plus recipe/data-pack composition revision.

Ordinary joins should not rebuild or reserialize identical recipe publication for each observer.

## 3. Class B — small semantic runtime codecs

These surfaces carry genuinely mutable semantic values and remain explicit target codecs.

### Recipe-book settings

The wire law is four `TypeSettings` values, each containing `open` and `filtering` booleans. The fresh/default profile starts with all values false, but the semantic representation remains a compact player-owned value rather than an opaque static byte blob so later persisted settings do not require an architectural rewrite.

### Clock synchronization

The time packet contains mutable game-time / clock network state. Registry identity for a world clock may be compact/static, but total ticks, partial tick, rate and paused/effective-rate state are semantic runtime values. These values must be projected from Crucible world/dimension time state, not frozen into a process-wide artifact.

### Default spawn / respawn data

Dimension identity, block position, yaw and pitch are semantic world/player bootstrap facts and are encoded directly from a compact snapshot.

### Dimension bootstrap facts

Dimension type is immutable for the lifetime/revision of a loaded dimension definition but is not a universal process constant: custom data-pack dimensions must remain possible. R2B therefore consumes an immutable dimension-bootstrap fact/image supplied by the dimension seam. R2C will replace the initial reference provider with `DimensionInstance` ownership.

## 4. Class C — selected-profile specialization

### Fresh empty inventory

The reviewed `ItemStack` encoder has an explicit empty fast path:

```text
empty ItemStack -> VarInt(0)
```

Only a non-empty stack enters `Item.STREAM_CODEC` and `DataComponentPatch` serialization.

The admitted R2B profile is a fresh player whose initial inventory/carried stacks are empty. Therefore the non-empty item/registry/component serializer graph is unreachable for this profile and MUST NOT block this gate or be pulled into R2B merely for genericity.

The first implementation exposes this truth explicitly as a fresh-empty inventory snapshot/projection. Persisted/non-empty player inventory is a later profile expansion and requires its own ItemStack wire admission before use.

Fail closed: a non-empty stack reaching the fresh-only encoder is an error, not a request to invent a partial ItemStack codec.

## 5. Projection image model

R2B should converge on a compact target-owned image such as:

```text
PlayBootstrapImage26_2
  shared:
    command_projection
    synchronized_recipe_projection
    composition-stable packet bodies
  per dimension/profile:
    dimension_bootstrap_projection
  per connection/player:
    login/player identity fields
    difficulty/abilities/held slot
    recipe-book settings
    initial empty inventory state
    teleport transaction
    spawn facts
    clock snapshot
    player-info facts
```

`StagedPublicationCursor` owns only bounded progression over the image. It does not own Minecraft packet identities, composition caches or world state.

## 6. Publication and backpressure law

For all three classes:

```text
prepare candidate body/artifact reference
        -> bounded egress admission
        -> commit cursor/state only after admission succeeds
```

No second outbound queue is introduced. Shared artifacts are immutable and byte-bounded by the composition/bootstrap cache policy. Slow clients may not pin unbounded historical artifacts.

## 7. Oracle artifacts

A composition artifact may use a stock-server reference run as black-box confirmation when:

1. the exact target/source/archive and profile are pinned;
2. source VAR/SEM evidence proves the semantic packet/stage is mandatory for that profile;
3. artifact identity includes every source-backed input that may alter the bytes;
4. its exact body/frame SHA is committed source-free;
5. a deterministic extraction/regeneration tool exists;
6. CI compares the generated/embedded artifact against the committed golden;
7. capture bytes are never used to infer semantic ordering or branch law by themselves.

This is deliberately different from R1X replay. R1X replays a finite observed stream. A qualified composition projection is a named immutable result of a source-defined semantic function for one explicit composition key.

## 8. Evidence-to-gate pipeline

The production projection is gated by a deliberately separate evidence pipeline:

```text
exact source reviews + pinned full capture
        -> final bounded evidence collection
        -> explicit final-seam human review
        -> ADMISSION_INPUTS_VERIFIED
        -> canonical source-free VAR materialization
        -> candidate GATE-NET-PLAY-ENTRY-26_2-001
        -> independent Atlas source-gate evaluation
        -> admitted target contract
        -> PlayBootstrapImage26_2 implementation
```

`tools/r2b_play_entry_collect_evidence.py` is the only combined local collector. It preserves two trust classes in one external `EPHEMERAL_DO_NOT_COMMIT` directory: source-rich final-seam evidence and source-free black-box composition artifacts.

`tools/r2b_play_entry_finalize.py` verifies exact 67-body and 117-body dossier commitments, the completed final-seam worksheet, the 15-rule R2B semantic contract and both oracle bodies. It intentionally emits `production_admitted=false` even when every input is valid.

`tools/r2b_play_entry_gate_materialize.py` converts those reviewed fingerprints into canonical source-free `VAR-NET-R2B-PLAY-*` records and a candidate source gate. Its semantic mapping is explicit for the full historical 67-body review plan, all seven 117-body families and the two final-seam groups. Unknown evidence fails closed.

The ordinary `tools/vanilla_source_gate.py` remains the independent admission authority. It must resolve every generated exact `type#signature` against the pinned Atlas database, recheck fingerprints, hazard coverage and SEM linkage, and report `admitted=true` before target/product code may rely on the candidate gate.

This split prevents the final source-rich evidence run from implicitly promoting itself into production truth and ensures official source excerpts are not needed after canonicalization.

## 9. Performance hypothesis

The important contest is not `Vec<u8>` versus another byte container. It is:

```text
per-join graph/filter/registry/codec reconstruction
        versus
resolve immutable projection key once + share exact bytes
```

Qualification must measure at least:

- CPU per join and clustered joins;
- allocations per join;
- copied/encoded bytes;
- retained shared memory;
- p50/p95/p99 join publication latency;
- backpressure behavior;
- composition-change regeneration cost.

The shared mechanism is admitted as a production winner only if whole-cost evidence supports it. The reference semantic encoder/artifact builder remains available for differential qualification.

## 10. Scope boundary

This design does not admit:

- non-default permission profiles without a matching command projection;
- runtime/plugin command mutations without projection invalidation/regeneration;
- non-empty/persisted inventory;
- custom dimensions without corresponding dimension facts;
- chunk/light/world streaming;
- movement or interaction semantics.

Those are explicit later expansions rather than accidental generic fallbacks.
