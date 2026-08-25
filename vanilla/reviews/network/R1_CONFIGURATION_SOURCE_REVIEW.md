# Minecraft 26.2 R1B Configuration Source Review

Status: **direct Configuration spine reviewed; delegated closure tooling merged; local source-gate execution and minimum Play-entry admission pending**  
Tracker: #146 / #143 / #78  
Depends on: `R1_LOGIN_SOURCE_REVIEW.md` and merged R1A target path  
Source: pinned official Minecraft 26.2 archive  
Archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`

## Purpose

R1B closes the first source-supported Minecraft 26.2 route from accepted Login into genuine Play
entry. Crucible preserves vanilla-observable ordering, conditions, data dependencies and state while
remaining free to implement them without Mojang's listener/task/packet object graph.

The evidence boundary is deliberately split so a stable wrapper fingerprint cannot silently bless a
changed delegated body:

```text
GATE-NET-CONFIG-26_2-001
    direct Configuration spine
              +
GATE-NET-CONFIG-CLOSURE-26_2-001
    delegated codec/task/registry/spawn dependencies
              +
GATE-NET-PLAY-ENTRY-26_2-001
    minimum source-inseparable fresh-player Play bootstrap
              ↓
source-admitted Target26_2 Configuration -> Play entry
```

The first two gate definitions/tooling exist. Neither may be treated as admitted until the reviewed
source-free records pass `tools/vanilla_source_gate.py` against the pinned local Atlas. The Play-entry
gate is still a planned boundary being discovered and reviewed.

## Entry boundary from R1A

The selected local-development Login route is already admitted as:

```text
Handshake(LOGIN, protocol 776)
-> offline ServerboundHello
-> LoginFinished
-> LoginAcknowledged
-> Configuration protocols/listener
-> startConfiguration()
```

The accepted offline profile survives this boundary. Crucible retains the required UUID/name state
compactly rather than reconstructing a Mojang-shaped `GameProfile` graph merely for architectural
similarity.

## Reviewed Configuration control-flow spine

Exact pinned-source review of the 25 direct R1B candidates found no contradiction with the current
Configuration SEM contract. The selected initial route is:

```text
Login acknowledgement
-> brand custom payload
-> server links, only when configured and non-empty
-> enabled features
-> SynchronizeRegistriesTask
-> code-of-conduct task, only when configured
-> server-resource-pack task, only when configured
-> PrepareSpawnTask
-> JoinWorldTask
-> ClientboundFinishConfigurationPacket
-> ServerboundFinishConfigurationPacket
-> install Play outbound protocol
-> duplicate/admission checks
-> prepared spawn path immediately
```

The selected Crucible profile has no server links, code-of-conduct or server resource pack. Those
branches remain source-visible policy alternatives, not selected-route traffic.

A fake R1 exit that merely changes a generic `Configuration -> Play` phase bit is therefore invalid.
The source-inseparable prepared-spawn and fresh-player bootstrap work belongs to the R1B evidence and
qualification surface.

## Why a supplemental Configuration closure gate exists

The 25 direct candidates resolved uniquely and their bodies were reviewed, but the review exposed a
change-detection weakness: several direct declarations delegate the detailed behavior relied upon by
SEM rules. Examples include `Packet.codec(write, constructor)` wrappers, outer prepared-spawn state
delegation, registry/tag builders and Configuration task helpers.

A direct wrapper VAR is valid evidence for that wrapper. It is not enough to make every delegated
semantic claim stale if a deeper method changes.

PR #169 therefore added `GATE-NET-CONFIG-CLOSURE-26_2-001` and a 43-candidate source-closure plan
covering the selected delegated packet read/write bodies, Configuration task progression, registry
packing/tag construction, nested prepared-spawn readiness/spawn logic and the generic collection /
identifier helpers actually used by the selected field laws.

Closure discovery must resolve each selector exactly once. The generated gate freezes the fully
resolved Atlas signature. Finalization is fail-closed and transactional: the entire worksheet,
INDEXED record set and gate boundary are validated before final output is created, staged output is
atomically renamed only after success, and a rejected review leaves no partial reviewed record set.

The 43 closure bodies are **not yet claimed reviewed/admitted** merely because the tooling exists.
They still require execution against the pinned local source/Atlas, exact-body review, hazard
resolution, SEM linkage and an independent green source gate.

## Immediate publication versus semantic tasks

`startConfiguration()` supports a compact implementation split.

Immediate publication occurs before queued task progression:

- server brand;
- server links only if present;
- enabled features.

Required selected-route semantic work then includes registry synchronization, spawn preparation and
join-world/finish progression. Crucible must preserve the resulting order and acknowledgement
boundaries but does not need Mojang's `ConcurrentLinkedQueue<ConfigurationTask>` representation.

## Known-pack negotiation law

Registry synchronization is stateful and exact:

1. the server sends its requested `KnownPack` list;
2. an exact client response selects that requested pack set;
3. any other accepted response falls back to the empty set;
4. registry data is packed/emitted under the selected set;
5. zero or more registry-data packets precede exactly one update-tags publication;
6. task completion follows that response path.

Known-pack negotiation therefore belongs in small target-local semantic state. Crucible must not
assume that a client possesses or accepts every server-known registry entry.

## Capture witness

The committed 26.2 Configuration-prefix witness independently records the selected route:

- server brand `vanilla`;
- enabled feature `minecraft:vanilla`;
- requested/selected known pack `minecraft:core@26.2`;
- 29 clientbound `registry_data` frames;
- one `update_tags` frame;
- one clientbound finish packet and one serverbound finish acknowledgement;
- 44,432 clientbound Configuration packet-body bytes in the captured route.

These bytes/IDs remain black-box evidence rather than source authority until the corresponding VAR
and source gates admit the underlying registration/codec law.

## Publication architecture direction

Configuration publication is large but mostly immutable/version-composition data. The current
preferred decomposition is therefore:

```text
shared immutable publication bodies/image
+ tiny per-connection negotiation/stage/cursor state
```

not a per-client registry/packet object graph or a second outbound queue.

The merged publication binder and bounded pre-play I/O path provide a transactional primitive:
publication cursor/state advances only after the existing bounded egress accepts the frame. The
current one-publication-step service quantum is a conservative fairness/backpressure baseline, not a
frozen throughput optimum. Bounded batching, preframed shared images or vectored I/O remain eligible
if controlled measurements show a whole-path win while preserving the same semantics.

## Prepared-spawn law

Pinned source review establishes that prepared spawn is real readiness work, not a packet marker.
The selected path uses a player-spawn region radius of 3 and waits for the corresponding load/readiness
conditions before the finish handoff may spawn the player. The supplemental closure gate exists in
part to bind the nested state bodies that contain this logic, rather than only outer delegation
methods.

## Play-entry boundary

`ServerConfigurationPacketListenerImpl.handleConfigurationFinished` completes the JoinWorld task,
installs the Play clientbound protocol, performs duplicate/admission checks and immediately invokes
the already-prepared spawn path.

The exact reviewed `PlayerList.placeNewPlayer` body then performs a substantial initial Play
bootstrap. Directly visible packet/helper surfaces include login/bootstrap state, difficulty,
abilities, held slot, recipes, permissions/stats/recipe book, scoreboard state, teleport/player-info,
level information, effects and inventory/menu initialization.

This proves that Play entry is source-inseparable from Configuration completion, but it does not yet
admit every subordinate packet ID/codec or prove every helper emits traffic on the selected default
profile.

`vanilla/frontiers/r1b-play-entry-selected.json` is therefore a discovery-only frontier rooted in the
reviewed `placeNewPlayer` body. Chunks, light, movement and general gameplay breadth are deliberately
excluded. The next Play-entry review must distinguish:

1. unconditional packet constructions in `placeNewPlayer`;
2. helper-driven network publication;
3. conditional/default-empty branches;
4. server-internal setup that is not client-observable packet law.

Only the minimum source-inseparable fresh-player surface should enter
`GATE-NET-PLAY-ENTRY-26_2-001`.

## Evidence still required before production Configuration code

The remaining source-admission work is now concrete:

- finalize the already-reviewed 25 direct records and require
  `GATE-NET-CONFIG-26_2-001` to report `admitted: true` with no failures against the pinned Atlas;
- execute the 43-entry delegated closure plan against the same source/Atlas, review exact bodies and
  hazards, finalize the source-free records, and require
  `GATE-NET-CONFIG-CLOSURE-26_2-001` to report `admitted: true`;
- use the Play-entry discovery frontier to select and review the minimum fresh-player packet/helper
  surface and require `GATE-NET-PLAY-ENTRY-26_2-001` to be independently green;
- close any subordinate codec dependency discovered by those reviews rather than implementing it from
  memory or community tables;
- freeze a finite selected-route Configuration/Play-entry contract only after those source gates are
  satisfied.

A registered packet is never evidence that the selected route sends it. Control flow, policy state
and capture evidence remain authoritative for route selection.

## Required R1B implementation/qualification artifacts

Before the target implementation merges, R1B must contain:

1. reviewed fingerprint-pinned VAR records for every relied-upon direct/delegated Configuration and
   minimum Play-entry method;
2. reviewed SEM rules with explicit hazards resolved;
3. all three narrow source-admission surfaces green against the pinned Atlas;
4. a finite selected-route Configuration/Play-entry reference contract/materializer;
5. generated static packet identity derived only from admitted source evidence;
6. reference/parity tests for codecs and publication ordering;
7. target qualification covering happy path, ordering edges, malformed fields, duplicates,
   known-pack mismatch/fallback, wrong-state input and bounded-output failure;
8. independent capture/replay evidence where applicable;
9. a real unmodified Minecraft Java 26.2 client probe reaching the admitted Play/bootstrap endpoint.

## Required performance/resource evidence

Configuration and initial Play bootstrap are cold/control-path work, so correctness dominates
speculative instruction tuning. They are still large enough for poor architecture to create join
spikes and per-client memory amplification.

The production path must therefore qualify:

- exact output equivalence against the simple source-backed reference;
- bounded peak per-connection egress;
- no per-client copied registry object graph;
- deterministic partial-drain/resume and cross-connection cursor isolation;
- shared-image preparation cost and retained bytes where used;
- per-connection allocation/copy counts where measurable;
- fresh-player publication latency and throughput on controlled hardware before claiming a
  performance advantage.

Hosted CI benchmark smokes are regression diagnostics only. Mechanism selection follows the project
performance qualification standard and representative controlled measurements.

## Exit condition

R1B source review is complete only when the direct Configuration gate, delegated Configuration
closure gate and minimum Play-entry gate are all backed by reviewed VAR/SEM evidence and independently
green against the pinned 26.2 Atlas. R1B implementation is complete only when the resulting finite
contract is implemented, parity/equivalence-qualified and an unmodified 26.2 client reaches the
admitted Play endpoint. Chunk/light publication follows as the next milestone rather than being folded
into this gate.
