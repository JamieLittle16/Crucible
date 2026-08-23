# Ownership, migration and staged-effect contract

Status: **M0 normative law / production mechanism unresolved**

This document freezes the semantic laws exercised by `crucible-ownership-qualification`. It does not select Crucible's eventual scheduler, queue, region sizing policy, threading runtime or migration implementation.

## Core distinction

> **Authority is semantic state. Worker placement is execution topology.**

Changing the number of workers, their identities, or the legal interleaving in which independent domains execute MUST NOT change the supported gameplay image.

A worker may execute authority. It does not become authoritative merely because work happened to run there.

## Domain authority

Every independently mutable semantic domain has exactly one ordinary mutation authority while active.

Authority identity contains:

```text
domain
owner/executor placement
ownership generation
```

A mutation token is valid only while all three still match the current active authority.

Tokens from previous ownership generations MUST fail closed even if the domain later returns to the same worker.

## Migration

Migration is an explicit quiescent handoff, not a change to a worker-index field while mutations continue.

The admitted sequence is:

```text
active(from, generation G)
        ↓ begin handoff
migrating(from → to, G)
        ↓ commit exact handoff
active(to, generation G+1, revision 0)
```

While `migrating`:

- no ordinary mutation token exists;
- the old owner cannot mutate;
- the new owner cannot mutate early;
- a different/stale handoff cannot steal authority;
- the next semantic stage cannot open.

Migration is currently allowed only at a closed stage boundary. A future mechanism may optimize the physical transfer, but it must present semantics equivalent to this quiescent model.

## Semantic revisions and freshness

Each ownership generation has a monotone semantic revision. Real state changes advance it; no-op changes do not need to.

Deferred work captures a `generation + revision` stamp at preparation time and declares one of two initial M0 freshness contracts:

- `ExactStamp` — install only if both generation and revision still match;
- `SameGeneration` — intervening same-generation semantic changes are permitted, but migration invalidates the work.

Preparation may happen independently of current authority. Installation MUST occur through current authority and satisfy the declared freshness contract.

Duplicate installation of one deferred result is invalid.

The initial two policies are qualification primitives, not permission to create arbitrary ad-hoc freshness modes. New policies require an explicit semantic need and their own evidence.

## Stages and reads

A semantic stage begins by freezing a stage-readable image.

During that stage:

- ordinary local mutation may proceed under each domain's authority;
- a stage-stable read observes the frozen image, not whichever worker happened to mutate first;
- foreign mutation is forbidden;
- cross-domain consequences are emitted as typed effects.

This deliberately prevents scheduler interleaving from becoming accidental gameplay order.

At the stage barrier, emitted effects are applied in a canonical semantic order independent of worker completion timing. The M0 oracle orders them by:

```text
(target domain, source domain, effect identity)
```

A future subsystem may define a more specific semantic order where vanilla requires one, but it may not substitute raw thread timing.

## Cross-domain effects

Foreign state change crosses an explicit typed boundary.

The M0 simulator uses small scalar effect payloads because it is testing authority/order laws rather than Minecraft-specific effect schemas. Production effect types will be subsystem-specific.

An effect:

- has a globally unique identity within the simulation/run;
- identifies source and target domains;
- is emitted only through current source authority;
- does not mutate the target immediately;
- is installed at the defined stage boundary;
- cannot target its own domain as a shortcut around local mutation rules.

## Schedule invariance qualification

The same logical program is executed under:

- multiple logical worker counts;
- different ownership placements;
- randomized legal interleavings of independent domain operations;
- repeated migrations;
- stage-stable cross-domain reads;
- typed staged effects;
- deferred prepare/install with revision changes.

Qualification compares the topology-independent semantic digest after **every completed stage**, not only final state.

The digest intentionally excludes worker identity. It includes semantic values and generation/revision identities.

A worker-count/interleaving change that alters any stage digest is a correctness failure.

## Fail-closed illegal schedules

Permanent adversarial tests cover at least:

- stale authority tokens;
- mutation during migration;
- migration while a stage is open;
- opening a stage during incomplete migration;
- stale deferred installation;
- wrong-domain deferred installation;
- duplicate deferred/effect identities;
- self-targeted foreign effects;
- arithmetic/counter overflow.

The oracle MUST report these as errors rather than silently routing, retrying, wrapping, or repairing them.

## Mechanisms deliberately not selected here

M0.9 does **not** choose:

- Tokio versus another execution runtime;
- work stealing versus fixed worker assignment;
- region/domain size;
- queue implementation;
- lock/atomic layout;
- NUMA placement;
- migration serialization format;
- batching thresholds;
- lock-free or unsafe techniques.

Those mechanisms may compete later. They must reproduce this qualified semantic contract.

## Production handoff

The deterministic simulator remains as an oracle after a production executor exists.

A production scheduling/migration mechanism is not admitted merely because its own tests pass. It must run equivalent randomized programs against this oracle and prove:

```text
same stage images
same accepted/rejected operations
same generation/revision progression
same final semantics
```

Only after semantic equivalence is established do worker scaling, cache locality, queue cost, migration cost, tail latency and machine-level performance decide between mechanisms.
