# R2B Replay-Free Vanilla Playtest Gate

This gate is the stock-client runtime checkpoint immediately before R2C world projection.

It proves the route Crucible intends to keep:

```text
stock Minecraft Java 26.2 client
        -> Handshake / offline Login
        -> source-admitted Configuration
        -> zero captured Play publication
        -> replay-free R2B semantic bootstrap
        -> same-driver teleport acknowledgement
        -> same-driver keep-alive liveness
        -> WorldProjection boundary
```

It deliberately does **not** publish chunks, light or a fake world. Until R2C exists, a successful
client may remain on the loading-world side after entering Play.

## Cold playtest image

The runtime playtest image is not the old R1X replay format. It has fixed magic `CRR2B001` and contains
only:

1. the 34 source-admitted Configuration packet bodies;
2. one immutable `update_recipes` R2B projection;
3. one immutable `commands` R2B projection;
4. one immutable `server_data` R2B projection.

There is no captured-Play section in this format.

`tools/r2b_pack_playtest.py` first runs the existing full R1B source-free artifact validator. That
validation checks the exact protocol/source/capture commitments, selected player profile, complete
Configuration aggregate/hash and complete captured-Play aggregate/hash. Only after the complete input
is validated does the R2B packer select the three shared immutable projection bodies and write the
smaller replay-free development image.

The runtime loader independently checks magic, protocol/source/capture commitments, finite file/body
bounds, Configuration count/aggregate, trailing data and target-level packet identities. It constructs
`Target26_2R1xContext` with an empty Play vector, so `enter_r2b_play_blocking_transport` retains its
existing fail-before-I/O `captured Play == 0` invariant.

The three selected shared projection bodies are cold development inputs, not semantic authority. The
source-backed R2B rules remain authoritative; the existing selected-profile whole-plan black-box test
cross-checks these shared bytes against the source-backed semantic assembly.

## Temporary post-R2B owner

After `WorldProjectionReady`, the development composition keeps the exact `R2bPlaySession` returned by
R2B. It does not allocate a second connection driver, egress queue or read scratch.

R2B continues to claim:

- teleport acknowledgement;
- keep-alive replies;
- keep-alive deadlines/challenges.

Other complete Play packets currently belong to future R2C/gameplay slices. During this playtest only,
the outer development owner discards them so movement/input cannot head-of-line block a later
keep-alive response. This discard policy is not production gameplay semantics and must disappear as
R2C/gameplay owners claim those packets.

## Build the image

Starting from the same qualified source-free JSON used by the R1X smoke route:

```text
python3 tools/r2b_pack_playtest.py \
  ~/Downloads/r1b-join-replay-image.json \
  --output /tmp/crucible-r2b-playtest.bin
```

The packer must report:

```text
captured_play_frames_written=0
shared_r2b_projection_frames=3
production_admitted=false
```

## Run

```text
cargo run --release --locked \
  --package crucible-server -- \
  --r2b-playtest-image=/tmp/crucible-r2b-playtest.bin
```

Connect an unmodified Minecraft Java **26.2** client to:

```text
localhost:25565
```

The selected qualification profile remains `Stato16` and the capture-qualified fixed connection
session epoch is supplied automatically by CLI option normalization.

## Success criteria

A successful gate requires all of the following:

1. the stock client completes Login and Configuration without protocol rejection;
2. server output reports `R2B WorldProjectionReady`;
3. that report states `captured_play_publication=0`;
4. the initial teleport transaction is acknowledged exactly (`id = 1`);
5. the connection survives long enough to complete at least one R2B-owned keep-alive cycle;
6. ordinary pre-R2C movement/input does not head-of-line block liveness;
7. no replacement connection driver, second egress queue or replacement read scratch exists at the
   Configuration -> R2B -> temporary-world-owner transition.

Client-visible terrain is **not** a success criterion for this gate. Terrain/chunk/light visibility is
R2C's responsibility.

## Failure evidence

For a stock-client failure, retain both:

- the exact Minecraft disconnect/loading-screen behavior;
- the complete `crucible-server` terminal output for that connection.

Do not repair this gate by reintroducing captured Play traffic or synthetic chunk/world packets. A
failure before `WorldProjectionReady` belongs to the R2B runtime composition; a failure caused by the
absence of world projection belongs to R2C.
