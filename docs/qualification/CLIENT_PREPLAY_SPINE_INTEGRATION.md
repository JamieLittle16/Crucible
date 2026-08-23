# Client Pre-Play Spine Integration Qualification

Issue: #92

## Purpose

This qualification proves that Crucible's target-version-agnostic networking layers compose transactionally before any Minecraft 26.2 packet identity is admitted.

The exercised path is:

```text
framed stream
    ↓
bounded ingress / borrowed FrameView
    ↓
runtime-neutral ConnectionDriver
    ↓
allocation-free PacketReader
    ↓
validated synthetic semantic action
    ↓
fail-closed SessionState transition
```

The packet identities in this laboratory are deliberately synthetic and must never be interpreted as Minecraft protocol constants.

## Transaction law

For one complete frame, semantic state and stream position commit together only after the entire action has succeeded.

A handler therefore:

1. receives a borrowed `FrameView`;
2. decodes every required field through `PacketReader`;
3. calls `PacketReader::finish()` so trailing bytes cannot be ignored;
4. validates phase-specific semantic conditions;
5. validates the requested lifecycle transition on a copy of `SessionState`;
6. returns success to the driver;
7. only after driver success does caller-owned state adopt the validated candidate state and any bounded response become visible.

If field decoding, exact-payload validation, phase validation or transition validation fails, the current frame remains logically unconsumed and external session state remains unchanged.

## Synthetic protocol

The suite intentionally uses unmistakably non-Minecraft packet identities for:

- selecting the status route;
- selecting the login route;
- completing synthetic login;
- completing synthetic configuration;
- issuing a synthetic status query/reply;
- terminal closure.

Synthetic sentinel strings and scalar proofs make accidental treatment as target protocol law difficult.

## Permanent qualification

The crate-local suite requires:

- whole-frame and byte-at-a-time input to produce the same login/configuration lifecycle result;
- every split point of route-selection frames to leave state unchanged until the complete frame exists;
- coalesced status frames to preserve exact order and response bytes;
- malformed complete payloads with trailing bytes to leave phase, ingress and egress unchanged;
- truncated semantic fields inside a complete wire frame to leave phase and ingress unchanged;
- packets illegal for the current phase to consume nothing and queue no response;
- one successful action to consume exactly its own frame while leaving the next coalesced frame intact;
- a 10,000-run deterministic fragmentation corpus to remain semantically stable.

Ordinary CI owns this through the named `Client spine integration qualification` step in addition to the complete workspace tests, strict Clippy and rustdoc.

## Deliberate non-decisions

This gate contains no:

- Minecraft 26.2 / protocol 776 packet ID;
- Mojang source body;
- authentication, encryption or compression policy;
- socket/runtime selection;
- gameplay packet or world code;
- dynamic dispatch or unsafe code.

Those concerns remain above or below this transaction boundary.

## Exit and next layer

Once this gate is green, the target-specific handshake/status implementation must preserve the same transaction law using fingerprint-pinned VAR/SEM evidence and golden fixtures from the official 26.2 source review. Target packet constants must not enter production Rust before that source gate is complete.
