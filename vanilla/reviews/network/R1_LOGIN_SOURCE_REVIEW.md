# Minecraft 26.2 R1A Login Source Review

Status: **policy/state/registration review complete; wire-subcodec follow-up required**  
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

## Payload facts already reviewed

`ServerboundHelloPacket` is:

```text
name: Minecraft UTF-8 string, max 16 Java UTF-16 units
profileId: UUID through FriendlyByteBuf UUID primitive
```

`ServerboundLoginAcknowledgedPacket` is a unit packet with no payload and is terminal.

`ClientboundLoginFinishedPacket` is a composite of:

```text
ByteBufCodecs.GAME_PROFILE
UUIDUtil.STREAM_CODEC
```

and is terminal.

## Not yet admitted

The probe intentionally did not include the implementation source for the delegated primitives
needed to materialize those fields. Before a finite Login contract or production codec is allowed,
R1A still needs exact source review of:

- `FriendlyByteBuf.readUUID` and `writeUUID`;
- `UUIDUtil.createOfflineProfile`;
- `UUIDUtil.STREAM_CODEC`;
- `ByteBufCodecs.GAME_PROFILE` and any directly required profile-property subcodec.

These are narrow dependencies. No broader Login architecture question remains.

## Configuration boundary discovered

Login acknowledgement immediately installs Configuration protocols and calls
`ServerConfigurationPacketListenerImpl.startConfiguration()`.

The Configuration source review also establishes that registry synchronization and the subsequent
prepare-spawn / join-world task chain are not optional merely because many Configuration packet
types exist. Code-of-conduct and configured server resource-pack tasks are conditional. That work is
owned by #146 and is not frozen by this R1A review.
