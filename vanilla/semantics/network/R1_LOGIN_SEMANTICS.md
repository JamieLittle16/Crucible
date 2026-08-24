# R1A Login Policy Semantic Contract — Minecraft Java 26.2

Status: **source-reviewed policy/state/registration layer; payload subcodecs still incomplete**  
Target: Minecraft **26.2**, protocol **776**, data version **4903**  
Source archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`  
Fingerprint algorithm: `java-token-v2-literal-sensitive`

This contract freezes only the Login control-flow and packet-registration facts already established
from the pinned 26.2 source for R1A. It deliberately does **not** yet admit a finite Login wire
contract: the inner `GameProfile` codec, UUID stream codec and offline-profile UUID construction
remain explicit source dependencies.

## Exact obligations

- **SEM-NET-R1A-001 — Login handshake entry.** Once the handshake has selected `LOGIN`, the server
  installs the Login clientbound protocol. Login accepts only the current protocol version; for the
  pinned target that is protocol 776. A matching client enters the Login serverbound protocol with a
  fresh Login listener.
- **SEM-NET-R1A-002 — Login registration order and selected IDs.** Serverbound Login registration
  order is `hello`, `key`, `custom_query_answer`, `login_acknowledged`, `cookie_response`.
  Clientbound Login registration order is `login_disconnect`, `hello`, `login_finished`,
  `login_compression`, `custom_query`, `cookie_request`. Combined with the already-admitted
  zero-based insertion-order dispatch law from R0, the R1A local-development path uses:
  serverbound `hello = 0`, serverbound `login_acknowledged = 3`, clientbound
  `login_finished = 2`.
- **SEM-NET-R1A-003 — serverbound hello envelope.** `ServerboundHelloPacket` carries, in order, a
  player name read/written as Minecraft UTF-8 bounded to 16 Java UTF-16 code units, then a UUID via
  `FriendlyByteBuf.readUUID` / `writeUUID`. The exact byte projection of that UUID primitive remains
  a required subcodec review before finite contract admission.
- **SEM-NET-R1A-004 — authentication branch.** `handleHello` is valid only in the Login `HELLO`
  state and rejects invalid player names. If the requested name matches the singleplayer profile,
  that profile proceeds directly to verification. Otherwise, encryption/key exchange is entered
  only when `server.usesAuthentication()` is true **and** the connection is not an in-memory
  connection. If that condition is false, no `ClientboundHelloPacket`/`ServerboundKeyPacket`
  exchange occurs and verification begins with `UUIDUtil.createOfflineProfile(requestedUsername)`.
- **SEM-NET-R1A-005 — compression branch.** After profile/login checks, Login compression is
  negotiated only when `server.getCompressionThreshold() >= 0` and the connection is not in-memory.
  The server first sends `ClientboundLoginCompressionPacket(threshold)` and installs compression
  after that send completes. Therefore a normal TCP local-development profile with a negative
  compression threshold remains on ordinary uncompressed framing.
- **SEM-NET-R1A-006 — Login completion.** After verification and duplicate-profile handling,
  successful Login enters `PROTOCOL_SWITCHING` and sends exactly one `ClientboundLoginFinishedPacket`
  containing the accepted `GameProfile` and the server connection session UUID. The packet is
  terminal in the Login clientbound protocol. Exact `GameProfile` and UUID subcodec byte law remains
  pending.
- **SEM-NET-R1A-007 — Login acknowledgement handoff.** A serverbound
  `ServerboundLoginAcknowledgedPacket` is a unit/terminal packet and is accepted only while Login is
  `PROTOCOL_SWITCHING`. Handling it installs the Configuration clientbound protocol, creates the
  initial common-listener cookie from the authenticated profile, installs the Configuration
  serverbound listener/protocol, starts Configuration, and marks Login accepted.
- **SEM-NET-R1A-008 — selected first R1 integration profile.** The first Crucible R1 path is a
  source-supported ordinary TCP local-development profile with authentication disabled and
  compression disabled. Under those policy inputs the admitted path is:

  `Handshake(LOGIN, protocol 776) -> ServerboundHello -> offline profile -> verification ->
  ClientboundLoginFinished -> ServerboundLoginAcknowledged -> Configuration`.

  This profile does not claim that encryption or compression are unimplemented Minecraft features;
  it states only that those branches are not traversed under this admitted policy.

## Existing evidence reused

R1A reuses the R0-reviewed general packet-ID construction law (`SEM-NET-R0-014` and
`SEM-NET-R0-015`) and the already-source-reviewed client-intent value `LOGIN = 2`
(`SEM-NET-R0-002`). R1A does not duplicate or mutate the sealed R0 VAR records.

## Explicit remaining source holes before finite Login contract

1. `FriendlyByteBuf.readUUID` / `writeUUID` exact byte law.
2. `UUIDUtil.createOfflineProfile(String)` exact deterministic UUID construction.
3. `ByteBufCodecs.GAME_PROFILE` exact profile field/property encoding.
4. `UUIDUtil.STREAM_CODEC` exact session UUID encoding used by `ClientboundLoginFinishedPacket`.

No production `Target26_2` Login encoder/decoder may guess these from memory or protocol tables.

## Implementation freedom once the finite contract is admitted

The target may model the source states as a compact Rust enum and carry only the data needed by the
selected path. It need not reproduce the Java listener, authentication thread, Netty handlers,
packet classes, or server object graph. Login is a cold path; transparency and bounded behavior
take priority over speculative micro-optimization.
