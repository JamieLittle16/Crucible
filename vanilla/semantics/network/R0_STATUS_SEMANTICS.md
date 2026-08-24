# R0 Status Semantic Contract — Minecraft Java 26.2

Status: **source-reviewed, packet-ID derivation pending final `ProtocolCodecBuilder` link**  
Target: Minecraft **26.2**, protocol **776**, data version **4903**  
Source archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`  
Fingerprint algorithm: `java-token-v2-literal-sensitive`

This contract freezes only the client-observable handshake/status/ping obligations needed by the R0
multiplayer-list slice. It does not freeze Mojang object structure, Netty composition, protocol
registry objects, listener allocation, JSON implementation, or socket/runtime architecture.

## Exact obligations

- **SEM-NET-R0-001 — handshake payload.** The serverbound intention payload is, in order:
  protocol version as Minecraft signed `VarInt`, server address as Minecraft UTF-8 string bounded to
  255 Java UTF-16 code units, server port as an unsigned network-order 16-bit integer, then client
  intent decoded from a Minecraft `VarInt`. The packet codec binds that payload to
  `ClientIntentionPacket`.
- **SEM-NET-R0-002 — status intent.** `ClientIntent.STATUS` has target wire value `1`.
  `LOGIN` is `2` and `TRANSFER` is `3`; other intent values are rejected by the target mapping.
- **SEM-NET-R0-003 — status transition.** A valid handshake selecting `STATUS` enters the target
  status protocol/listener route. This R0 contract admits no direct Handshake → Play edge.
- **SEM-NET-R0-004 — handshake registration order.** The target handshake serverbound protocol
  registers `CLIENT_INTENTION` as its only packet codec entry.
- **SEM-NET-R0-005 — status registration order.** In the status protocol, serverbound registration
  order is status request followed by ping request; clientbound registration order is status
  response followed by pong response. This rule records source order only; the final claim that the
  zero-based codec position is the wire packet ID is completed by the separate
  `ProtocolCodecBuilder` review.
- **SEM-NET-R0-006 — status request payload.** `ServerboundStatusRequestPacket` is a unit packet:
  after the packet ID it carries no payload bytes.
- **SEM-NET-R0-007 — status response envelope.** `ClientboundStatusResponsePacket` carries one
  `ServerStatus` value through the target lenient JSON stream codec with a maximum string length of
  32767 Java UTF-16 code units.
- **SEM-NET-R0-008 — outer status JSON law.** `ServerStatus` contains `description` with default
  empty component, optional `players`, optional `version`, optional `favicon`, and
  `enforcesSecureChat` defaulting to `false`. These are JSON codec semantics, not a requirement that
  Crucible allocate an equivalent record graph.
- **SEM-NET-R0-009 — admitted status subrecords.** When emitted, `players` contains required integer
  `max` and `online` fields and optional `sample` defaulting to an empty list. `version` contains
  required string `name` and integer `protocol`. R0 does not require Crucible to emit a favicon.
- **SEM-NET-R0-010 — status request lifecycle.** The first valid status request sends one status
  response. A repeated status request on the same target status listener is rejected/disconnected;
  it does not produce a second status response.
- **SEM-NET-R0-011 — ping request payload.** The status ping request carries exactly one signed
  network-order 64-bit integer.
- **SEM-NET-R0-012 — pong echo.** The pong response carries exactly the same signed 64-bit value,
  encoded network-order, with no semantic transformation.
- **SEM-NET-R0-013 — ping terminal behavior.** Handling a valid ping request sends the corresponding
  pong response and then closes/disconnects the status connection.
- **SEM-NET-R0-014 — protocol-builder order preservation.** `ProtocolInfoBuilder.addPacket` appends
  codec entries to one ordered list; packet-codec construction iterates that list in order and feeds
  each entry to `ProtocolCodecBuilder`; `Details.listPackets` reports each entry with its zero-based
  list index. This rule deliberately stops one link short of asserting that the
  `ProtocolCodecBuilder` integer is the on-wire packet ID.

## Pending final source link

Before `PROTO-NET-STATUS-26_2-001` may freeze numeric packet IDs, the exact target
`ProtocolCodecBuilder` source must be reviewed and fingerprint-pinned to prove how the integer used
by its encoder/decoder is assigned from insertion order. Black-box capture is an independent
convergence gate, not a substitute for this source link.

## Crucible implementation freedom

The R0 target may therefore:

- statically match packet IDs once the finite contract is admitted;
- decode directly from borrowed packet payload bytes;
- prebuild/cache the selected status-response JSON/frame outside the per-connection path;
- use compact enums/structs rather than Mojang packet/listener object graphs;
- use the already-qualified bounded connection/session machinery.

It must not add a dynamic packet registry, target lookup table, second framing layer, per-packet heap
object requirement, or runtime-service abstraction merely to resemble Mojang.
