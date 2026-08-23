# Packet Body Core Qualification

**Parent:** #83 / #78  
**Layer:** version-agnostic complete-packet field mechanics

## Boundary

`crucible-packet-core` sits between outer framed packet views and Minecraft-version packet semantics:

```text
TCP fragments
  ↓
crucible-connection-core
  ↓ FrameView { packet_id, payload }
crucible-packet-core
  ↓ typed field reads/writes
26.2 protocol model
```

It deliberately contains no packet IDs, connection-state transitions, protocol-776 constants, authentication policy, compression policy, socket runtime, or gameplay types.

## Reader law

`PacketReader` borrows one already-complete packet payload.

- fixed-width numbers use network byte order;
- VarInts and strings reuse the admitted wire-core law;
- strings retain Java UTF-16-unit limits;
- boolean bytes are canonical `0` / `1` only;
- successful fields advance by their exact encoded width;
- incomplete fields are malformed *packet bodies*, not incomplete TCP streams;
- every failed field read leaves the cursor unchanged;
- `finish()` rejects unclaimed trailing bytes;
- `take_remaining()` exists only for packet laws that explicitly define an opaque trailing field.

No read allocates or copies field payload data.

## Writer law

`PacketWriter` appends to caller-owned storage under an explicit per-payload byte budget.

Every field determines and validates its complete encoded size before mutation. Crossing the budget fails transactionally. String validation is performed before the existing transactional wire-string encoder is invoked.

This is a transparent reference mechanism. Direct encoding into a connection egress buffer may later remove an intermediate payload allocation, but only after a whole-path benchmark demonstrates that the assembly allocation is material.

## Permanent qualification

Tests cover:

- fixed-width big-endian values;
- signed VarInt roundtrip;
- bounded Unicode strings;
- canonical and malformed booleans;
- every fixed-field truncation width;
- incomplete and overlong VarInts;
- truncated and invalid strings;
- cursor rollback after failures;
- exact trailing-byte rejection;
- writer byte-bound rollback;
- rejected string rollback;
- 10,000 deterministic mixed-field records.

Ordinary CI runs `cargo test --package crucible-packet-core --locked` as a named **Packet body qualification** gate in addition to workspace check/Clippy/tests/rustdoc.

## Next gate

The next layer must use fingerprint-pinned official 26.2 VAR/SEM records to define handshake and status packet identities, fields and state transitions. This crate is not evidence for any target-version packet ID.
