# Vanilla Source Review — R0 Handshake / Status / Ping (26.2)

Target: Minecraft Java **26.2**, protocol **776**, data version **4903**.  
Pinned source archive SHA-256:
`1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`.

This review records client-observable source facts for the first multiplayer-list milestone and
separates target semantics from Mojang mechanism. The numeric R0 packet-ID derivation is now closed
against the pinned source through `ProtocolCodecBuilder` and `IdDispatchCodec`.

## Handshake

`ClientIntentionPacket` decodes its payload in the target order:

1. protocol version through the Minecraft `VarInt` primitive;
2. server address through the target UTF-8 string primitive with bound 255;
3. server port through an unsigned 16-bit network-order read;
4. intent through a `VarInt` followed by `ClientIntent.byId`.

`ClientIntent` maps STATUS/LOGIN/TRANSFER to 1/2/3 respectively and rejects unknown values.
`HandshakeProtocols` registers only the intention packet for the serverbound handshake protocol.

`ServerHandshakePacketListenerImpl.handleIntention` is the semantic transition point reviewed for
the R0 STATUS branch. Login/transfer behavior is outside this slice.

## Status registration and codecs

The target status protocol source declares ordered serverbound registrations:

1. status request;
2. ping request.

Its ordered clientbound registrations are:

1. status response;
2. pong response.

`ServerboundStatusRequestPacket` uses a unit codec and therefore has no payload after its packet ID.

`ClientboundStatusResponsePacket` binds a `ServerStatus` value to the target lenient JSON stream
codec with string bound 32767. The outer `ServerStatus` codec defines optional/default JSON fields:
description (default empty component), players, version, favicon, and `enforcesSecureChat` (default
false). The R0 review additionally binds the `Players` and `Version` nested codecs because those are
the only optional subrecords expected to be needed by the deterministic multiplayer-list fixture.
Favicon semantics remain outside the finite R0 response unless the fixture actually emits one.

## Status listener behavior

The reviewed target status listener is one-shot for status information. The first status request
sends the prepared status response; a duplicate status request disconnects instead of sending a
second response.

A ping request decodes exactly one signed 64-bit network-order integer. Pong encoding writes the same
64-bit representation. The listener constructs the pong from the request value, sends it, then
disconnects.

This lifecycle is important: R0 must not turn the status connection into a reusable generic request
loop simply because that would be a convenient abstraction.

## Packet-number derivation

`ProtocolInfoBuilder.addPacket` appends to an `ArrayList`. Both `buildPacketCodec` and
`buildDetails` consume a copied ordered codec list. `buildPacketCodec` visits entries in list order;
`CodecEntry.addToBuilder` forwards each type/codec to `ProtocolCodecBuilder.add`.
`Details.listPackets` exposes the same list position `i` as the reported network ID.

`ProtocolCodecBuilder.add` validates packet flow and forwards the packet type/serializer to one
`IdDispatchCodec.Builder`; `build` returns that builder's codec. The dispatch builder appends entries
in registration order. During `build`, it walks those entries in the same order and assigns each
unique type `id = toId.size()`, so the first registration receives 0, the second 1, and so on.
Duplicate type registration is rejected.

`IdDispatchCodec.decode` reads a Minecraft `VarInt` and uses that integer directly as the index into
the ordered entry list, rejecting negative or out-of-range IDs. `encode` obtains the type's assigned
integer from the same map, writes it with Minecraft `VarInt`, and chooses the serializer at that
same list index.

Combining those rules with the reviewed registration declarations closes the R0 identities:

- handshake serverbound `CLIENT_INTENTION` = 0;
- status serverbound `STATUS_REQUEST` = 0;
- status serverbound `PING_REQUEST` = 1;
- status clientbound `STATUS_RESPONSE` = 0;
- status clientbound `PONG_RESPONSE` = 1.

These are source-derived protocol-776 facts. The independent black-box capture remains a byte-level
convergence test; it is not the source of packet meaning or identity.

## Mechanisms not frozen

Crucible does **not** inherit:

- `ProtocolInfoBuilder`/`ProtocolCodecBuilder` object layout;
- Netty `ByteBuf` ownership or pipeline composition;
- Mojang packet/listener allocation patterns;
- a runtime packet registry;
- Mojang's JSON object graph;
- per-request recomputation of an otherwise static server-list response.

For R0 the preferred production shape is a tiny statically specialized target decoder over borrowed
frame payloads, with bounded output and a response image prepared outside the connection hot path.
