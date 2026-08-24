# R1A Login Semantic Contract — Minecraft Java 26.2

Status: **finite Login source law complete; final source-gate rerun and independent capture pending**  
Target: Minecraft **26.2**, protocol **776**, data version **4903**  
Source archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`  
Fingerprint algorithm: `java-token-v2-literal-sensitive`

This contract freezes the selected R1A Login route and its byte-level source law for the pinned
26.2 server. The remaining qualification boundary is no longer protocol discovery: the complete
source law must pass the version-controlled source gate against the real pinned Atlas database, then
an independent unmodified-client capture must converge with the finite contract before production
Login is admitted.

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
  player name encoded through Minecraft UTF-8 with a maximum of 16 Java UTF-16 code units, then a
  UUID using `UUIDUtil.STREAM_CODEC`. Under **SEM-NET-R1A-009**, that UUID is exactly two ByteBuf
  long fields: most-significant bits first, least-significant bits second.
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
  containing the accepted `GameProfile` followed by the server connection session UUID. The packet
  is terminal in the Login clientbound protocol. The profile obeys **SEM-NET-R1A-011** and the
  session UUID obeys **SEM-NET-R1A-009**.
- **SEM-NET-R1A-007 — Login acknowledgement handoff.** A serverbound
  `ServerboundLoginAcknowledgedPacket` is a unit/terminal packet and is accepted only while Login is
  `PROTOCOL_SWITCHING`. Handling it installs the Configuration clientbound protocol, creates the
  initial common-listener cookie from the accepted profile, installs the Configuration
  serverbound listener/protocol, starts Configuration, and marks Login accepted.
- **SEM-NET-R1A-008 — selected first R1 integration profile.** The first Crucible R1 path is a
  source-supported ordinary TCP local-development profile with authentication disabled and
  compression disabled. Under those policy inputs the admitted path is:

  `Handshake(LOGIN, protocol 776) -> ServerboundHello -> offline profile -> verification ->
  ClientboundLoginFinished -> ServerboundLoginAcknowledged -> Configuration`.

  This profile does not claim that encryption or compression are unimplemented Minecraft features;
  it states only that those branches are not traversed under this admitted policy.
- **SEM-NET-R1A-009 — UUID stream primitive.** `UUIDUtil.STREAM_CODEC` delegates decode to
  `FriendlyByteBuf.readUUID(ByteBuf)` and encode to `FriendlyByteBuf.writeUUID(ByteBuf, UUID)`.
  Encoding writes `UUID.getMostSignificantBits()` with `ByteBuf.writeLong`, then
  `UUID.getLeastSignificantBits()` with `ByteBuf.writeLong`; decoding performs the inverse by
  constructing `new UUID(input.readLong(), input.readLong())`. The UUID payload is therefore exactly
  two consecutive 64-bit ByteBuf long fields, MSB half first and LSB half second, with no length,
  presence, or other wrapper.
- **SEM-NET-R1A-010 — offline profile identity.** The source-supported offline branch computes the
  profile UUID by calling Java `UUID.nameUUIDFromBytes` on the UTF-8 bytes of
  `"OfflinePlayer:" + playerName`, then constructs `new GameProfile(id, playerName)`. Crucible's
  independent implementation must be equivalent to Java 25 `UUID.nameUUIDFromBytes` for that exact
  byte input. The JDK algorithm behind `nameUUIDFromBytes` is an external-runtime law and must be
  qualified independently rather than represented as a Mojang-source claim.
- **SEM-NET-R1A-011 — GameProfile stream law.** `ByteBufCodecs.GAME_PROFILE` is composed from, in
  declared component order, `UUIDUtil.STREAM_CODEC` for the profile id, `PLAYER_NAME` for the name,
  and `GAME_PROFILE_PROPERTIES` for properties. `PLAYER_NAME` is Minecraft UTF-8 bounded to 16 Java
  UTF-16 code units. `GAME_PROFILE_PROPERTIES` carries a VarInt count with maximum 16; each property
  then carries a Minecraft UTF-8 name bounded to 64, a value bounded to 32767, and a nullable
  signature. Nullability is one boolean; when present the signature follows as Minecraft UTF-8
  bounded to 1024. Encoding rejects property counts above 16. The fingerprint-pinned three-component
  `StreamCodec.composite` overload decodes codec1, codec2, codec3 in that order and passes the values
  to the constructor in the same order; encoding invokes codec1, codec2, codec3 in that order.
- **SEM-NET-R1A-012 — LoginFinished session identity and component order.**
  `ClientboundLoginFinishedPacket` declares the `GameProfile` component first and the server
  connection session UUID second. The fingerprint-pinned two-component `StreamCodec.composite`
  overload decodes codec1 before codec2 and encodes codec1 before codec2, so the packet byte order is
  exactly GameProfile then session UUID. The session UUID uses the same `UUIDUtil.STREAM_CODEC` as
  **SEM-NET-R1A-009**. It is runtime session state, not a version-pinned constant; capture
  materialization must parse or bind the observed value rather than hardcode one UUID across server
  sessions.

## Existing evidence reused

R1A reuses the R0-reviewed general packet-ID construction law (`SEM-NET-R0-014` and
`SEM-NET-R0-015`) and the already-source-reviewed client-intent value `LOGIN = 2`
(`SEM-NET-R0-002`). R1A does not duplicate or mutate the sealed R0 VAR records.

## Remaining qualification before production Login

The Minecraft-source questions for the selected offline/uncompressed Login path are closed. The
remaining work is qualification rather than source interpretation:

1. run `GATE-NET-LOGIN-WIRE-26_2-001` against the real pinned Atlas database with all required VARs,
   including the two- and three-component `StreamCodec.composite` overloads;
2. independently qualify Crucible's implementation of Java 25 `UUID.nameUUIDFromBytes` for the exact
   `OfflinePlayer:` UTF-8 input law;
3. materialize a finite Login protocol contract from the admitted source law;
4. acquire an independent unmodified-client Login capture under authentication-disabled,
   compression-disabled policy and require source-contract/capture convergence;
5. only then admit the production `Target26_2` Login codec/state machine.

No production `Target26_2` Login codec may bypass those gates.

## Implementation freedom once the finite contract is admitted

The target may model the source states as a compact Rust enum and carry only the data needed by the
selected path. It need not reproduce the Java listener, authentication thread, Netty handlers,
packet classes, or server object graph. Login is a cold path; transparency and bounded behavior
take priority over speculative micro-optimization.
