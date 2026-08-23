# Protocol Black-Box Pre-Play Capture

**Status:** qualification/evidence tooling  
**Initial scope:** uncompressed, unencrypted handshake/status traffic

## Purpose

Official source is Crucible's primary white-box implementation oracle, but externally visible
protocol claims should not depend on source reading alone.

`tools/protocol_capture_proxy.py` provides an independent black-box evidence path for the first
client milestone. It transparently proxies one TCP connection between an unmodified Minecraft
client and a controlled vanilla server while reconstructing only the generic length-framed wire
boundary.

It deliberately does **not** interpret packet IDs, packet fields or session semantics.

The intended R0 evidence graph is:

```text
official 26.2 source                      unmodified 26.2 client
        ↓                                          ↓
Atlas / VAR / SEM                         transparent capture proxy
        ↓                                          ↓
source admission gate                       exact framed bytes
        └──────────────────┬───────────────────────┘
                           ↓
                  protocol contract
                           ↓
                  contract validator
                           ↓
                    static Rust adapter
                           ↓
                   Crucible status probe
```

Source-backed and black-box evidence must agree before Crucible claims target compatibility.

## What the proxy records

For each direction independently:

- exact stream byte count and SHA-256;
- canonical frame count;
- frame ordinal and byte offset;
- exact framed bytes;
- exact body bytes;
- per-frame SHA-256.

The artifact also binds the Minecraft version, protocol version, official source archive SHA-256 and
Atlas fingerprint algorithm from `vanilla/vanilla.lock.toml`.

TCP packetization is not evidence. The incremental recorder therefore emits the same artifact
whether one semantic frame arrives in one `recv`, one byte at a time, or coalesced with adjacent
frames.

No peer IP address, client address or wall-clock timestamp is written to the evidence artifact.

## Fail-closed limits

The proxy requires explicit positive limits for:

- frame body bytes;
- total stream bytes per direction;
- frame count per direction;
- socket/accept timeout.

It rejects:

- noncanonical frame-length VarInts;
- more than five frame-length VarInt bytes;
- lengths outside the non-negative i32 range;
- frames exceeding the configured bound before their payload is admitted;
- streams/frame counts exceeding configured bounds;
- EOF in the middle of a frame;
- socket/proxy failures;
- unsafe symlink output.

The proxy never repairs malformed traffic for the purpose of obtaining an artifact.

## Byte-transparency law

Bytes are captured and then forwarded unchanged with `sendall`. Capture parsing has no
transformation step.

Permanent tests use real connected sockets and require the byte stream observed at the far endpoint
to equal the producer stream exactly while the recorder independently reconstructs the same framed
image.

This is evidence tooling, not the production Crucible network loop. Its implementation choices do
not constrain the server runtime.

## Initial protocol-776 workflow

Run a controlled vanilla 26.2 server on localhost, then start the proxy on a different local port:

```bash
python3 tools/protocol_capture_proxy.py \
  --listen-host 127.0.0.1 \
  --listen-port 25566 \
  --upstream-host 127.0.0.1 \
  --upstream-port 25565 \
  --lock vanilla/vanilla.lock.toml \
  --output .crucible/evidence/protocol-776-status-capture.json
```

Point an unmodified 26.2 client at `127.0.0.1:25566` and allow one multiplayer-list status/ping
exchange to complete.

The capture is **evidence for review**, not an automatically admitted protocol contract. Reviewers
still have to map each observed frame to fingerprint-pinned source records and SEM rules before the
finite contract firewall accepts it.

## Compression/encryption boundary

This v1 tool is intentionally limited to ordinary length-prefixed plaintext framing.

It must not be used to claim semantic frame evidence after Minecraft enables transport compression
or encryption. Those transform the wire boundary and require separately qualified capture/decoder
stages.

R0 status/ping occurs before those mechanisms and is therefore the correct first use.

## Qualification

`tools/tests/test_protocol_capture_proxy.py` permanently checks:

- whole-stream, every split point and byte-at-a-time equivalence;
- exact offsets and exact captured bytes;
- noncanonical and oversized frame-length rejection;
- stream and frame-count limits;
- incomplete-EOF rejection;
- deterministic artifact output independent of TCP chunking;
- exact target identity from the vanilla lock;
- real socket byte transparency.

No Minecraft 26.2 packet identity or field layout is embedded in the tests.

## Next gate

Once the first controlled client↔vanilla status capture exists, it can be used alongside the Atlas
source review to construct `PROTO-NET-STATUS-26_2-001` and its golden bodies/frames. The resulting
contract must pass the source gate, protocol-contract validator and static adapter regeneration
before a Crucible R0 status executable is qualified against an unmodified client.
