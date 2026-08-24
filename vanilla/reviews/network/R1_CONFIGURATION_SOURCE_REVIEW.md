# Minecraft 26.2 R1B Configuration Source Review

Status: **source control-flow spine established; packet/codec fingerprint closure and final gate pending**  
Tracker: #146 / #143 / #78  
Depends on: `R1_LOGIN_SOURCE_REVIEW.md` and merged R1A target path  
Source: pinned official Minecraft 26.2 archive  
Archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`

## Purpose

R1B closes the first source-supported Minecraft 26.2 route from accepted Login into real Play entry.
This review records the source law which must be frozen before the 26.2 target is extended.

The governing implementation rule is:

> Preserve vanilla ordering, conditions, data dependencies and externally observable state without
> reproducing Mojang's listener/task/packet object graph where Crucible can implement the same law
> more directly.

This document distinguishes **established source law** from facts that are still discovery evidence
and therefore must not yet enter generated packet identity or production target code.

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

The accepted offline profile must survive this boundary. The 26.2 target now retains its UUID and
printable-ASCII name inline rather than rebuilding a Mojang-shaped `GameProfile` object graph.

## Established Configuration control-flow spine

Pinned 26.2 source review establishes the initial Configuration ordering at the semantic level:

```text
Login acknowledgement
-> brand custom payload
-> server links, only when the configured link set is non-empty
-> enabled features
-> SynchronizeRegistriesTask
-> code-of-conduct task, only when conduct text is configured
-> server-resource-pack task, only when a pack is configured
-> PrepareSpawnTask
-> JoinWorldTask
-> ClientboundFinishConfigurationPacket
-> ServerboundFinishConfigurationPacket
-> install Play outbound protocol
-> duplicate/admission checks
-> PrepareSpawnTask.spawnPlayer(...) immediately
```

The source therefore does **not** support a fake R1 exit which merely sets a generic
`Configuration -> Play` phase bit and stops. The minimum source-inseparable player/bootstrap work
performed by `PrepareSpawnTask.spawnPlayer` belongs to the R1B evidence and qualification surface.

## Immediate publication versus semantic tasks

`startConfiguration()` exposes a useful implementation decomposition.

### Immediate publication

The source sends these values before the queued task sequence begins:

- server brand;
- server links only if present;
- enabled features.

These are publication/state projection operations, not evidence that Crucible needs a generic task
object for each packet.

### Required task spine

`SynchronizeRegistriesTask`, `PrepareSpawnTask` and `JoinWorldTask` are always on the selected initial
route. Code-of-conduct and server resource-pack tasks are explicit policy branches.

Crucible must preserve the resulting order and acknowledgement boundaries. It need not reproduce
Mojang's `ConcurrentLinkedQueue<ConfigurationTask>` representation.

## Known-pack negotiation law

The registry synchronization path is stateful and exact.

`SynchronizeRegistriesTask.start` first sends the server's requested `KnownPack` list.

When the client responds:

- an exact list match permits registry packing against that negotiated known-pack set;
- any different accepted response falls back to the empty known-pack set;
- registry data is then emitted under that selected packing law;
- registry data packets precede one update-tags publication;
- the synchronize-registries task is completed only after that response path has been handled.

Therefore R1B must model known-pack negotiation as target-local semantic state. It must not assume a
vanilla client necessarily possesses, selects or retains every server-known registry entry.

## Registry publication consequence

The source semantics imply a potentially multi-packet publication after the known-pack decision.
That publication is mostly version/composition data rather than unique per-player semantic state.

The preferred Crucible decomposition is therefore provisional but strongly motivated:

```text
immutable Configuration publication image
+ per-connection negotiation/stage/cursor
```

rather than:

```text
per-client registry object graph
-> per-client packet object graph
-> one giant outbound transaction
```

The production form must still pass the source/equivalence gate. This review does not pre-admit a
specific storage container, sharing primitive, publication cursor or I/O mechanism.

## Atomic-response boundary

Existing `ATOMIC_OUTBOUND_ADMISSION` remains normative for an inbound semantic action whose complete
required response is naturally one bounded batch. R1B evidence now demonstrates a separate class of
work: large ordered publication which can progress after an already-committed semantic decision.

A future publication mechanism must therefore preserve both laws:

1. semantic decisions and their required atomic responses remain fail-closed transactions;
2. bulk ordered publication may advance incrementally only through explicit committed cursor/state
   transitions, with bounded egress and no hidden unbounded staging queue.

The existing atomic mechanism must not be weakened merely to accommodate registry publication.

## Configuration state direction

The exact enum names are not yet production API, but the source responsibilities currently separate
cleanly into states of this form:

```text
entry / immediate publication
await known-pack response
registry/tag publication
optional policy handshakes
spawn preparation
await finish-configuration acknowledgement
Play/bootstrap handoff
```

Each state should carry only data valid at that point. The accepted Login profile remains available
through the final admission/spawn handoff.

## Play-entry boundary

Source review of `ServerConfigurationPacketListenerImpl.handleConfigurationFinished` establishes that
the server:

1. completes the `JoinWorldTask`;
2. installs the Play clientbound protocol;
3. performs duplicate/admission checks;
4. immediately invokes the prepared spawn path.

`PrepareSpawnTask` is not merely a packet placeholder. It prepares the eventual player/spawn state,
including source-visible world/spawn readiness work, before the client finish acknowledgement.

R1B is complete only when the minimum fresh-player Play/bootstrap surface required by that same
handler is source-admitted and the unmodified 26.2 client reaches the corresponding observable Play
state.

## Server session UUID ownership discovered during R1 work

The `LoginFinished` session UUID is not target-version packet identity and must not be generated by
the packet adapter. Pinned source review establishes it as server connection-population state: the
server lazily creates one random UUID for the current non-empty connection population and clears the
value when that population becomes empty.

Crucible should therefore eventually own this in a small server/product control-plane primitive.
The already-merged target API is correctly decoupled because it only receives the runtime UUID.
Random-source selection remains a separate dependency/security/portability review; it is not part of
this Configuration packet-law document.

## Evidence still required before production Configuration code

The following remain **unadmitted** until fingerprint-pinned records and a green source gate exist:

- complete clientbound Configuration registration order and selected packet IDs;
- complete serverbound Configuration registration order and selected packet IDs;
- exact brand/custom-payload codec and identifier/resource-location law;
- enabled-feature payload codec and collection bounds;
- exact `KnownPack` codec and selected-known-packs collection bound;
- registry-data packet field law, registry key encoding and element payload codec;
- update-tags packet field law, nested collection/count bounds and integer-ID encoding;
- inherited client-information/cookie inputs required on the selected route;
- unit finish-configuration packet law on both directions;
- all ordering/duplicate/wrong-state listener behavior relied upon by the selected path;
- the minimum fresh-player `PrepareSpawnTask -> PlayerList.placeNewPlayer` Play packet surface;
- any initial Play packet whose emission is source-inseparable from the Configuration finish handler.

A registered packet is not evidence that the selected route sends it. Control flow and policy
conditions remain authoritative.

## Required R1B artifacts

Before target implementation merges, R1B must contain:

1. fingerprint-pinned `VAR-NET-R1-CONFIG-*` records for every exact relied-upon declaration/method;
2. reviewed Configuration/Play-entry SEM rules;
3. an explicit hazard review;
4. `GATE-NET-CONFIG-26_2-001` with `admitted: true` and no failures;
5. a finite selected-route Configuration contract/materializer;
6. generated static packet identity from that contract;
7. independent capture/replay evidence wherever the selected route remains plaintext and the
   capture boundary is qualified;
8. target qualification covering happy path, every ordering edge, malformed fields, duplicates,
   negotiation mismatch/fallback and bounded-output failure;
9. a real unmodified Minecraft Java 26.2 probe reaching the admitted Play/bootstrap endpoint.

## Required performance/resource evidence

Configuration is cold/control-path work, so correctness dominates speculative instruction tuning.
It is still large enough for poor architecture to create serious join spikes and per-client memory
amplification.

Any shared/prebuilt publication mechanism must therefore qualify:

- exact output equivalence against a simple reference publication;
- bounded peak per-connection egress;
- no per-client copied registry object graph;
- deterministic partial-drain/resume behavior;
- cross-connection cursor isolation;
- preparation cost and retained shared-image bytes;
- per-connection allocation/copy counts where measurable;
- full publication latency/throughput on controlled hardware before claiming a performance win.

Hosted CI timing is diagnostic only. Permanent correctness/resource tests and benchmark compilation
belong in CI; mechanism selection follows `PERFORMANCE_QUALIFICATION_STANDARD.md`.

## Exit condition

This review becomes source-complete when every production-relevant Configuration and inseparable
Play-entry dependency above is represented by reviewed VAR/SEM evidence and the narrow source gate is
green. Only then should `Target26_2` gain Configuration packet semantics.