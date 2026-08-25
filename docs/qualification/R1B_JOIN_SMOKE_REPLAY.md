# R1B Join Smoke Replay

Status: **experimental development aid; not production admission**  
Tracker: #146  
Target: Minecraft Java 26.2 / protocol 776

## Purpose

Crucible already reaches the Configuration boundary with the real development binary. The next
supported milestone remains the source-admitted Configuration -> Play route, but the existing
black-box capture contains a long real 26.2 session after `LoginFinished`. Reusing that capture is a
high-value smoke test for Crucible's own framing, bounded publication and phase orchestration while
the last Play-entry source edge is closed.

This mode must not become a substitute for the final Play gate.

## Capture image

`tools/r1b_join_capture_image.py` accepts only the exact pinned capture with SHA-256
`11ead8de74df70b40d7fb045ff9561f06f6e24238765d4141a1d090cab546b57`.

It validates the committed Configuration witness before exporting anything:

- server ordinal 0 is the already-admitted `LoginFinished` body;
- server ordinals 1..34 are the selected Configuration sequence;
- those 34 bodies must total 44,432 bytes;
- their concatenated body and framed-stream SHA-256 values must equal the committed witness;
- server traffic after ordinal 34 is exported only as `experimental` Play replay data.

The output contains packet bodies and provenance, not official source text. Configuration bytes are
source-admitted by the direct and delegated Configuration gates. Play replay bytes remain black-box
smoke-test evidence until `GATE-NET-PLAY-ENTRY-26_2-001` is admitted.

## Architectural boundary

The intended runtime representation is one immutable image shared by all matching development
connections plus a small per-connection publication cursor. The replay mode must use Crucible's
existing `PrePlayPublisher` / bounded egress path. It must not add:

- a second outbound queue;
- dynamic packet registries;
- per-connection registry/NBT reconstruction;
- Mojang task graphs or packet object graphs to the HOT path.

The selected capture profile is intentionally fixed (`Stato16`, its offline UUID and the captured
server session UUID). General player state belongs to the supported R1 implementation, not to this
smoke fixture.

## Last Play-entry unknown

The 35-body follow-up source review closes every selected-route branch except the exact side effects
of inventory-menu synchronizer installation. `tools/r1b_play_inventory_sync_probe.py` queries the
pinned Atlas for `AbstractContainerMenu` listener/synchronizer methods and nested synchronizer
callbacks without accessing or emitting source text. Its result is a review lead only; exact source
bodies still require the normal source-rich review firewall before final admission.

## Exit criteria

The replay aid is useful when an unmodified 26.2 client can traverse the real Crucible socket path
far enough to exercise Configuration and captured Play traffic. It is removed or kept explicitly
experimental once the supported route can do the same work from SEM-backed state.

The supported R1 milestone still requires:

1. close inventory synchronizer source semantics;
2. admit `GATE-NET-PLAY-ENTRY-26_2-001` independently;
3. implement the finite dynamic Play bootstrap from admitted SEM;
4. run the official client probe without replay dependence.
