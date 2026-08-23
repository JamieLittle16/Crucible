# Protocol Wire Core Qualification

Issue: #78  
Target product line: Minecraft 26.2 / protocol 776

## Scope

`crucible-protocol-core` owns the reusable byte-level laws below versioned packet semantics:

- signed 32-bit Minecraft `VarInt` coding;
- the remote TCP `VarInt21` packet frame;
- Minecraft `Utf8String` length semantics;
- explicit fragmented-input handling.

It does **not** define target-version packet IDs, connection-state transitions, compression,
encryption, authentication or gameplay semantics. Those belong in the source-backed 26.2 packet
layer.

## Source pin and review

The wire laws in this qualification were checked against the official source archive:

- Minecraft: `26.2`
- protocol: `776`
- world/data version: `4903`
- archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`

Reviewed source locations:

- `net.minecraft.network.VarInt`
- `net.minecraft.network.Varint21FrameDecoder`
- `net.minecraft.network.Varint21LengthFieldPrepender`
- `net.minecraft.network.Utf8String`
- `net.minecraft.network.Connection#configureSerialization`

The source archive is an external semantic oracle and is **not** committed or redistributed by
Crucible.

## Core invariants

1. A short byte stream is `Incomplete`, never silently classified as a complete value.
2. A normal signed `VarInt` is bounded to five bytes.
3. The outer remote packet length is a distinct `VarInt21`: at most three encoded bytes.
4. A remote packet frame with body length zero is rejected, matching vanilla framing.
5. The largest vanilla remote frame body is `(1 << 21) - 1` bytes.
6. Frame limits are checked before body access or output mutation.
7. Minecraft string byte lengths are signed-`VarInt` prefixed; negative lengths fail closed.
8. String semantic length is measured in UTF-16 code units, matching Java `String.length()` rather
   than Rust Unicode-scalar count.
9. The encoded-byte ceiling is three bytes per permitted UTF-16 code unit.
10. Decoded frame/string payloads borrow from the input slice; successful reads do not copy them.
11. Encoders validate before appending, so rejected values leave caller output unchanged.
12. Every successful decode reports exact consumption for coalesced TCP streams.
13. No unsafe code or runtime dispatch is required.

### Deliberate hardening

Vanilla's `Utf8String` delegates byte-to-string conversion to Netty. Crucible's wire core explicitly
rejects malformed UTF-8. This is a fail-closed hardening rule for invalid input; valid vanilla-client
streams are unaffected. If later compatibility evidence shows malformed-string behavior is externally
material, that policy must be reviewed explicitly rather than drifting accidentally.

## Permanent tests

The crate-local suite includes:

- canonical positive and negative `VarInt` vectors;
- 200,000 deterministic `VarInt` roundtrips;
- truncation at every `VarInt` terminal boundary;
- rejection of overlong `VarInt`s;
- frame fragmentation at every byte boundary;
- rejection of zero-length and wider-than-`VarInt21` frames;
- frame-limit rejection before body access;
- exact consumption across coalesced frames;
- transactional frame-encoding failures;
- Unicode string roundtrip and fragmentation;
- Java UTF-16 length semantics, including supplementary characters;
- malformed UTF-8 and encoded-byte-limit rejection;
- transactional string-encoding failures and zero-length strings.

CI runs this crate as a named **Protocol wire qualification** step before the full workspace test
sweep.

## Next source-backed gate

The same source review has now identified the handshake/status law. The next PR freezes its packet
IDs, field order, limits and state transitions in VAR/SEM records before an executable server path is
admitted. The first external integration oracle is an unmodified 26.2 client performing server-list
status/ping against Crucible.
