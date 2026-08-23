# Protocol Packet-Body Codec Qualification

Issue: #83

## Purpose

`crucible-packet-core` is the target-version-agnostic field codec between framed Minecraft packet payload bytes and source-backed packet semantics.

It deliberately owns **no** packet IDs, protocol-version constants, connection-state transitions, authentication policy, socket/runtime selection, compression policy or gameplay semantics.

## Reader laws

`PacketReader` operates on one already-complete frame payload.

- variable-width strings and raw remainder bytes borrow from the caller payload;
- no field read allocates heap storage;
- successful reads advance by exactly the consumed wire width;
- malformed or truncated reads leave the cursor unchanged;
- field truncation is an error here, not stream-level `Incomplete`, because the enclosing packet body is already complete;
- booleans accept only canonical bytes `0` and `1`;
- fixed-width integers use network byte order;
- `finish()` rejects any trailing payload byte.

## Writer laws

`PacketWriter` owns one bounded packet body under an explicit byte ceiling.

- every fixed-size or variable-size field checks the packet byte budget before mutation;
- semantic string rejection leaves the writer unchanged;
- packet-budget rejection leaves the writer unchanged;
- booleans emit canonical `0`/`1` bytes;
- no dynamic dispatch or unsafe code is used.

The writer intentionally uses a normal `Vec<u8>` reference mechanism. Buffer pooling, slabs or fixed-capacity packet arenas are future mechanisms that must prove whole-cost benefit before replacing this boundary.

## Permanent qualification

The crate-local suite covers:

1. network-order fixed-width scalar encoding/decoding;
2. positive and negative Minecraft `VarInt` roundtrips;
3. Java UTF-16-unit string bounds and borrowed string decoding;
4. malformed boolean rejection without cursor movement;
5. every truncation boundary for variable/fixed-width fields;
6. exact cursor rollback after failed reads;
7. exact trailing-byte rejection;
8. writer rollback at the configured packet byte limit;
9. a 20,000-record deterministic mixed-field roundtrip;
10. pointer identity for borrowed trailing raw bytes.

Ordinary CI runs both the dedicated packet-body test gate and the complete workspace gate under strict Clippy and rustdoc warnings-as-errors.

## Admission boundary

Passing this qualification does **not** prove any Minecraft 26.2 packet layout. Target packet IDs, field order, legal state transitions and handshake/status semantics must be frozen separately from the pinned official 26.2 source frontier before they are implemented above this crate.
