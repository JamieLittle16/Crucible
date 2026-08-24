# Minecraft 26.2 R1A Login Source Review

Status: **selected offline/uncompressed Login source law complete; final local gate rerun and independent capture pending**  
Tracker: #145 / #143  
Source: pinned official Minecraft 26.2 archive  
Archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`

## Review result

The first R1 Login route does not require encryption or compression if the server policy selects the
corresponding source branches.

`ServerLoginPacketListenerImpl.handleHello` enters the key/encryption branch only when
`usesAuthentication()` is true and the connection is not in-memory. Otherwise it constructs an
offline profile and enters verification directly.

Compression is independent. `verifyLoginAndFinishConnectionSetup` sends the Login compression
packet and installs compression only when the configured threshold is non-negative and the
connection is not in-memory.

Therefore an ordinary TCP development profile with:

```text
authentication = disabled
compression threshold < 0
```

has the source control flow:

```text
LOGIN handshake
  -> Login protocols
  -> ServerboundHello
  -> offline GameProfile
  -> VERIFYING
  -> ClientboundLoginFinished
  -> ServerboundLoginAcknowledged
  -> Configuration protocols/listener
  -> startConfiguration()
```

This is the preferred first R1 integration path. It is source-supported simplification, not a claim
that Crucible may ignore authentication/compression for configurations that require them.

## Login packet registration

`LoginProtocols` registers serverbound packets in this order:

```text
0 hello
1 key
2 custom_query_answer
3 login_acknowledged
4 cookie_response
```

and clientbound packets in this order:

```text
0 login_disconnect
1 hello
2 login_finished
3 login_compression
4 custom_query
5 cookie_request
```

The integer IDs above use the general zero-based insertion-order dispatch mechanism already admitted
for R0; this review adds only the Login-specific registration list.

The selected plaintext local-development route therefore requires Login packet identities:

```text
serverbound hello              0
serverbound login_acknowledged 3
clientbound login_finished     2
```

## State law

The Java listener uses a finer state machine than Crucible's coarse `SessionPhase`:

```text
HELLO
KEY
AUTHENTICATING
NEGOTIATING
VERIFYING
WAITING_FOR_DUPE_DISCONNECT
PROTOCOL_SWITCHING
ACCEPTED
```

The selected offline route does not traverse `KEY` or `AUTHENTICATING`. It moves
`HELLO -> VERIFYING`, optionally waits for a duplicate profile to disconnect, then enters
`PROTOCOL_SWITCHING`. Login acknowledgement is accepted only in `PROTOCOL_SWITCHING`.

Crucible should preserve the observable/order constraints, not reproduce this enum mechanically.
The existing typed `Target26_2State` is the correct ownership boundary.

## Delegated payload review

The identity-bound source read covers `FriendlyByteBuf`, `UUIDUtil`, `ByteBufCodecs`, and the exact
relevant `StreamCodec.composite` overloads from the same pinned 26.2 archive.

### UUID primitive

`UUIDUtil.STREAM_CODEC` delegates to the static `FriendlyByteBuf` UUID helpers. `writeUUID` writes
`getMostSignificantBits()` and then `getLeastSignificantBits()` through two `ByteBuf.writeLong`
calls. `readUUID` performs two `ByteBuf.readLong` calls in the same order and passes them to the
`UUID` constructor. There is no length or optionality wrapper around those 16 bytes.

This law is shared by the profile UUID in `ServerboundHello`, the profile UUID in
`GAME_PROFILE`, and the server session UUID in `ClientboundLoginFinishedPacket`.

### Offline profile

`UUIDUtil.createOfflinePlayerUUID(playerName)` calls Java `UUID.nameUUIDFromBytes` with the UTF-8
bytes of exactly `"OfflinePlayer:" + playerName`. `createOfflineProfile` constructs a
`GameProfile` from that UUID and the same name.

The Minecraft source therefore fixes the input and JDK API contract. It does not itself define the
JDK's internal `nameUUIDFromBytes` algorithm. Crucible must qualify its independent Rust
implementation against Java 25 rather than mislabel a remembered JDK implementation detail as
Mojang-source evidence.

### GameProfile codec

`ByteBufCodecs.GAME_PROFILE` declares these components:

```text
UUIDUtil.STREAM_CODEC
PLAYER_NAME
GAME_PROFILE_PROPERTIES
```

`PLAYER_NAME` is `stringUtf8(16)`.

`GAME_PROFILE_PROPERTIES` uses a VarInt property count bounded to 16. For each property it carries:

```text
name       Minecraft UTF-8, max 64
value      Minecraft UTF-8, max 32767
signature  nullable; boolean presence then UTF-8 max 1024 when present
```

Encoding uses the same fields and rejects a property count above 16. The source helpers establish
that the count is a VarInt and nullable presence is a boolean.

### Composite ordering — closed

The exact `StreamCodec.java` source closes the final generic ordering dependency.

The two-component overload used by `ClientboundLoginFinishedPacket` decodes `codec1`, then `codec2`,
then calls the constructor with `(v1, v2)`; encode invokes `codec1` then `codec2` in that same order.
Therefore the LoginFinished payload is exactly GameProfile followed by session UUID.

The three-component overload used by `ByteBufCodecs.GAME_PROFILE` decodes `codec1`, `codec2`, then
`codec3`, passes `(v1, v2, v3)` to the constructor, and encodes in the same codec1 -> codec2 ->
codec3 order. Therefore the GameProfile payload order is exactly profile UUID, player name,
properties.

Both overloads are now represented by fingerprint-pinned VAR records and required by the R1A wire
gate.

## Evidence-index consequence

`ByteBufCodecs` is a Java interface. Its initialized codec fields are implicitly `public static
final`, even though the source does not spell the `static` modifier on each declaration. The older
declaration-index projection missed those implicit interface class-initialization fields.

That tooling defect was fixed by declaration-index 0.1.2 in #149. The refreshed local declaration
index reports 3,481 synthetic class-initialization nodes, including the corrected interface
initialization evidence.

## Local source-gate result

The first real local `GATE-NET-LOGIN-WIRE-26_2-001` run admitted all eleven UUID/profile/count/string/
nullable primitives with `failures: []` against the pinned Atlas DB. After the composite source read,
the gate was strengthened to require the exact two- and three-component `StreamCodec.composite`
overloads as well. One final local gate rerun must confirm those two reconstructed fingerprints before
this source-admission PR merges.

## Configuration boundary advanced

The same source review materially narrows R1B:

- `SynchronizeRegistriesTask.start` sends `ClientboundSelectKnownPacks` first;
- the client response is compared with the exact requested-pack list;
- an exact match permits known-pack-backed registry element contents to be omitted;
- any different accepted list falls back to an empty known-pack set and therefore sends full
  element contents where required;
- registry data packets are emitted before one `ClientboundUpdateTagsPacket`;
- `JoinWorldTask.start` emits the unit `ClientboundFinishConfigurationPacket`;
- `PrepareSpawnTask` is a server-side readiness task that prepares the eventual player/spawn state
  and must not be collapsed into a packet-only shortcut.

R1B must therefore model known-pack negotiation as semantic state. It may not hardcode an assumption
that the client always knows vanilla registry contents.

## Remaining R1A qualification actions

1. rerun the strengthened wire source gate against the real pinned Atlas DB and require
   `admitted: true` with no failures;
2. independently qualify the Java 25 `UUID.nameUUIDFromBytes` behavior used by the offline profile;
3. materialize the finite Login contract;
4. require an independent unmodified 26.2 client capture under the selected offline/uncompressed
   policy and source-contract/capture convergence;
5. only then admit the production Login implementation.
