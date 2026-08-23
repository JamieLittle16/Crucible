# Protocol Wire Core Qualification

Issue: #78
Target product line: Minecraft 26.2 / protocol 776

## Scope

`crucible-protocol-core` is deliberately version-agnostic. It implements only reusable byte-level laws required by the later 26.2 packet layer:

- signed 32-bit Minecraft-style VarInt coding;
- bounded length-delimited frames;
- bounded UTF-8 strings;
- explicit fragmented-input handling.

It does **not** define packet IDs, connection-state transitions, compression, encryption, authentication or gameplay semantics. Those become admissible only after source-backed 26.2 VAR/SEM work.

## Core invariants

1. A short byte stream is `Incomplete`, never silently malformed.
2. Five VarInt continuation bytes fail closed.
3. Negative lengths fail before payload access.
4. Caller-supplied byte limits fail before payload access or output mutation.
5. Decoded frame/string payloads borrow from the input slice; the decoder does not allocate or copy them.
6. Encoders validate before appending, so rejected values leave the caller's output unchanged.
7. Every successful decode reports exact consumption so coalesced TCP data can be advanced without rescanning.
8. No unsafe code or runtime dispatch is required.

## Permanent tests

The crate-local tests include:

- canonical positive and negative VarInt vectors;
- 200,000 deterministic VarInt roundtrips;
- truncation at every VarInt terminal boundary;
- rejection of overlong VarInts;
- frame truncation at every byte boundary;
- negative and over-limit frame rejection;
- coalesced-frame exact consumption and zero-length frames;
- transactional frame encoding failures;
- Unicode string roundtrip and fragmentation;
- invalid UTF-8, byte-limit and character-limit rejection;
- transactional string encoding failures and zero-length strings.

CI runs this crate as a named **Protocol wire qualification** step before the full workspace test sweep.

## Next source-backed gate

The next PR must excavate the pinned official 26.2 source and commit reviewed protocol records for the handshake/status path. It must not infer packet IDs or field order from memory. The first external integration oracle is an unmodified 26.2 client performing server-list status/ping against Crucible.
