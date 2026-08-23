# Handshake and Status Semantic Contract — Minecraft Java 26.2

Status: **source-backed client-spine contract**  
Target: Minecraft **26.2**, protocol **776**, world/data version **4903**  
Official source archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`

This document freezes the externally observable handshake/status obligations extracted from the
official target source. It does **not** freeze Mojang's Netty pipeline, listener classes, object
layout or allocation strategy.

## Packet-ID derivation rule

`ProtocolInfoBuilder` retains packet codecs in registration order. `IdDispatchCodec.Builder` assigns
the next integer ID from that order, and `IdDispatchCodec` reads/writes that ID as a signed Minecraft
`VarInt` before the packet payload. Therefore the IDs below are source-derived registration indices,
not values copied from a third-party protocol table.

## Exact obligations

- **SEM-NET-HS-001 — outer TCP framing.** Remote packets are framed by a positive `VarInt21` body length. The length prefix is at most three bytes and the body contains packet ID plus packet payload.
- **SEM-NET-HS-002 — zero-frame rejection.** A remote frame body length of zero is invalid before packet dispatch.
- **SEM-NET-HS-003 — frame ceiling.** The largest body representable by the accepted three-byte length prefix is `2^21 - 1 = 2,097,151` bytes.
- **SEM-NET-HS-004 — initial packet ID.** The sole serverbound HANDSHAKING packet is `intention`, network ID `0`.
- **SEM-NET-HS-005 — intention layout.** `intention` payload order is protocol-version `VarInt`, host `Utf8String(255)`, unsigned big-endian 16-bit port, then intention `VarInt`.
- **SEM-NET-HS-006 — intention values.** Status = `1`, login = `2`, transfer = `3`; other values are invalid.
- **SEM-NET-HS-007 — terminal handshake.** `intention` is terminal for the HANDSHAKING protocol and selects the next inbound/outbound protocol.
- **SEM-NET-HS-008 — status version behavior.** The STATUS branch does not reject a handshake solely because its advertised protocol version differs from `776`. Login performs its own version gate later.
- **SEM-NET-HS-009 — status request ID.** STATUS serverbound network ID `0` is `status_request` and has no payload.
- **SEM-NET-HS-010 — ping request ID.** STATUS serverbound network ID `1` is `ping_request` and carries exactly one signed 64-bit big-endian value.
- **SEM-NET-HS-011 — status response ID.** STATUS clientbound network ID `0` is `status_response`.
- **SEM-NET-HS-012 — status response payload.** `status_response` carries one JSON value encoded through `Utf8String(32767)`.
- **SEM-NET-HS-013 — pong response ID.** STATUS clientbound network ID `1` is `pong_response` and carries exactly one signed 64-bit big-endian value.
- **SEM-NET-HS-014 — one status response.** The first status request sends the current `ServerStatus`; a second status request on the same connection disconnects rather than sending another response.
- **SEM-NET-HS-015 — ping echo.** A ping request sends a pong containing the exact 64-bit request value, then disconnects the status connection.
- **SEM-NET-HS-016 — status schema.** The target status codec supports `description`, optional `players`, optional `version`, optional `favicon`, and `enforcesSecureChat` (default false). Version contains `name` and integer `protocol`; players contains `max`, `online`, and optional sample.
- **SEM-NET-HS-017 — complete packet consumption.** After packet decode, extra unread bytes in the frame are a protocol error rather than an ignored suffix.
- **SEM-NET-HS-018 — UTF-16 string limit.** Minecraft string character limits are Java `String.length()` limits, i.e. UTF-16 code units. They are not Rust Unicode-scalar counts.

## Reviewed source evidence

| Obligation area | Official source location |
| --- | --- |
| remote frame width / zero rejection | `net.minecraft.network.Varint21FrameDecoder` |
| remote frame encoding | `net.minecraft.network.Varint21LengthFieldPrepender` |
| packet ID serialization | `net.minecraft.network.codec.IdDispatchCodec` |
| registration-order ID assignment | `net.minecraft.network.protocol.ProtocolInfoBuilder` |
| generic signed VarInt | `net.minecraft.network.VarInt` |
| string bounds | `net.minecraft.network.Utf8String` |
| initial serialization pipeline | `net.minecraft.network.Connection#configureSerialization` |
| handshake registration | `net.minecraft.network.protocol.handshake.HandshakeProtocols` |
| handshake payload | `net.minecraft.network.protocol.handshake.ClientIntentionPacket` |
| intention values | `net.minecraft.network.protocol.handshake.ClientIntent` |
| protocol transition | `net.minecraft.server.network.ServerHandshakePacketListenerImpl` |
| status registration | `net.minecraft.network.protocol.status.StatusProtocols` |
| status request | `net.minecraft.network.protocol.status.ServerboundStatusRequestPacket` |
| status response | `net.minecraft.network.protocol.status.ClientboundStatusResponsePacket` |
| ping request / pong | `net.minecraft.network.protocol.ping.ServerboundPingRequestPacket`, `ClientboundPongResponsePacket` |
| status listener behavior | `net.minecraft.server.network.ServerStatusPacketListenerImpl` |
| status JSON schema | `net.minecraft.network.protocol.status.ServerStatus` |
| exact-consumption failure | `net.minecraft.network.PacketDecoder` |

## Explicitly non-frozen Mojang mechanisms

The following are evidence, not Crucible architecture:

- Netty `ByteBuf` ownership;
- `ChannelPipeline` handler ordering as an implementation object graph;
- listener-class hierarchy;
- `ProtocolInfoBuilder` and `IdDispatchCodec` runtime objects;
- temporary buffers used by Mojang string encoding;
- one-object-per-packet allocation;
- event-loop implementation and channel classes.

Crucible may use generated tables, direct state-machine dispatch, borrowed packet views, buffer pools,
read batching, vectored writes or another reactor. It must preserve the semantic contract above and
qualify any performance mechanism separately.

## Required evidence

The Crucible 26.2 status path must retain:

1. source-derived golden byte fixtures for handshake, status request and ping/pong;
2. frame fragmentation and TCP coalescing tests;
3. exact-consumption checks for every decoded packet;
4. malformed, oversized and unknown-ID rejection tests;
5. deterministic pure state-machine tests, including duplicate status request behavior;
6. a loopback socket test over the executable status path;
7. an unmodified Minecraft 26.2 client as the external status/ping integration oracle.
