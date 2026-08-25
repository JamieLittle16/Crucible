# R1X First Visible World

**Classification:** milestone evidence / historical record  
**Date:** 2026-08-25  
**Minecraft target:** Java Edition 26.2, protocol 776  
**Tested branch revision:** `46f63972e0b8b9041f71dc954c8dd5ca4f8f9d14`  
**Production admitted:** no

## Result

An unmodified Minecraft: Java Edition 26.2 client successfully connected to Crucible, completed the full pre-play route, entered Play, and rendered world data on screen.

The observed successful route was:

```text
TCP
 -> Handshake
 -> Login
 -> Login acknowledgement
 -> Configuration
 -> known-pack negotiation
 -> registry data
 -> tags
 -> Finish Configuration
 -> client Configuration acknowledgement
 -> Play
 -> early Play bootstrap
 -> chunk data accepted
 -> visible world rendered
```

This was the first direct black-box confirmation that Crucible's Rust networking/session spine can carry a stock Mojang client from connection establishment into a rendered Minecraft world.

## Exact smoke profile

The successful manual test used the explicitly experimental R1X target and a compact replay image produced from the pinned source-free 26.2 capture.

Selected replay prefix:

```text
configuration_frames = 34
configuration_bytes  = 44,432
play_frames           = 385
play_bytes            = 560,569
capture_sha256        = 11ead8de74df70b40d7fb045ff9561f06f6e24238765d4141a1d090cab546b57
production_admitted   = false
```

The stock client joined and displayed one chunk. After the finite captured Play prefix was exhausted, the connection eventually timed out because R1X does not yet provide a continuing live Play simulation.

The server reported normal lifecycle endings (`SessionClosed` / `PeerEof`) rather than a protocol, codec, registry, framing, ingress-bound, or state-transition rejection.

## What this proves

This milestone is evidence that:

- Crucible's bounded framing and connection buffers interoperate with the official 26.2 client;
- the implemented Handshake and Login route reaches Configuration correctly;
- the source-admitted Configuration sequence, known-pack branch, registry data, tags, and finish handshake are accepted by the stock client;
- Crucible can transition the client into Play through the same target-neutral bounded publication/I/O architecture used by the earlier slices;
- the client accepts enough early Play state and chunk data to instantiate and render a world;
- the project's strict architecture has not introduced a hidden incompatibility that prevents an actual vanilla client from reaching gameplay state.

## What this does not prove

R1X is deliberately not a production Play implementation.

The 385 Play bodies in this test are a finite captured replay prefix. Therefore this milestone does **not** establish that Crucible independently synthesizes all initial Play packets, owns the rendered chunk, maintains a server tick, performs continuing keepalive/player/chunk tracking, or provides a persistent playable session.

Captured Play traffic remains isolated behind `Target26_2R1x` and is explicitly marked `production_admitted=false`. The production target must continue to fail closed until each semantic Play surface is independently admitted.

No captured replay image is distributed with repository milestone release binaries.

## Architectural significance

The milestone was reached without introducing a Mojang-style runtime object graph or a replay-specific networking stack. R1X uses the existing bounded connection and publication machinery with process-owned immutable replay bodies and compact per-connection state.

That changes the engineering problem materially. The project no longer needs to establish whether the official client can cross Crucible's modern Login/Configuration boundary. There is now a known-working end-to-end trace that can be progressively replaced with Crucible-owned semantic implementations.

The replay fixture is an executable differential specification, not the intended product architecture.

## Next gate: persistent live Play

The next product-facing milestone is a persistent, walkable server with no captured Play replay. The minimum sequence is:

1. source-backed semantic Play entry and player bootstrap;
2. live keepalive and acknowledgement lifecycle so the client remains connected;
3. live position/teleport and movement handling;
4. inventory/menu initialization closure;
5. Crucible-owned chunk and light publication;
6. chunk tracking while the player moves;
7. reconnect and repeatability qualification.

At that point Crucible becomes a primitive but genuine live Minecraft server rather than an end-to-end join experiment.

## Repository marker

This evidence record is intended to accompany the milestone tag:

```text
milestone-r1x-first-visible-world
```

Milestone tags publish an experimental Linux x86_64 `crucible-server` binary and SHA-256 checksum through the tag-triggered release workflow. Those artifacts are snapshots for reproducibility and inspection, not a claim of a playable production release.
