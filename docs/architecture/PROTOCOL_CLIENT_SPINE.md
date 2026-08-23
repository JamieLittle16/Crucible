# Protocol and Client Spine

Crucible's first executable product slice is intentionally narrow: establish a correct, version-pinned path from TCP bytes to an unmodified target client before broad gameplay exists.

## Vertical route

```text
TCP stream
→ wire framing
→ target-version packet law
→ handshake/status
→ login/configuration
→ play entry
→ pregenerated vanilla chunks/light
→ movement/collision
```

The real Minecraft 26.2 client is an early integration oracle, not the source of truth. Source-backed VAR/SEM records define the supported protocol law; differential client/vanilla captures provide additional evidence.

## Layering rule

The protocol stack is split so that generic byte mechanics cannot smuggle version semantics into reusable infrastructure:

1. **Wire core** — VarInt, bounded frame/string decoding, incremental stream boundaries. No packet IDs.
2. **26.2 protocol model** — generated/reviewed packet IDs, fields, limits and connection states pinned to protocol 776.
3. **Connection engine** — socket lifecycle, backpressure, read/write buffering and state-machine execution.
4. **Product adapter** — status identity, player lifecycle, world/chunk/light projection and movement input.

A lower layer must not know about a higher layer's packet types or gameplay objects.

## Performance posture

Start with a transparent reference mechanism and preserve the ability to benchmark replacements. In particular:

- frame decoding should operate on borrowed slices and report exact consumption;
- packet parsing should not require per-field dynamic dispatch;
- connection state should be compact and explicit;
- read/write buffer ownership must be bounded;
- backpressure is mandatory rather than allowing unbounded per-client queues;
- packet batching, vectored I/O, buffer pools and alternate executors require whole-cost evidence before admission.

Network cleverness is not allowed to weaken protocol fidelity or resource bounds.

## Qualification ladder

Every stage is expected to retain tests below it:

- wire-core malformed/fragmented input tests;
- golden target-version packet bytes;
- deterministic captured-stream replay;
- vanilla differential status/login traces where observable;
- real unmodified client integration probe;
- resource/backpressure stress;
- throughput, latency, allocation and memory qualification before replacing reference mechanisms.

## First runnable milestones

### R0 — status server

A Crucible binary listens on localhost and an unmodified 26.2 client can add it to the multiplayer list, receive a valid status response and complete ping/pong. This proves TCP, framing, handshake state and the first source-backed packet table end to end.

### R1 — enter play

A local-development client reaches play through the supported login/configuration path. Authentication/encryption policy must be explicit; no accidental compatibility claims.

### R2 — visible pregenerated world

The client receives a small pregenerated vanilla world, chunks and light without world generation being a prerequisite.

### R3 — walkable server

Movement and collision are authoritative, reconnect is deterministic, and the client can traverse the pregenerated slice. This is the first product-facing vertical slice described by the execution master plan.
