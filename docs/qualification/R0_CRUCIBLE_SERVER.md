# Crucible R0 Localhost Server

Status: **R0 EXTERNAL-PROBE CANDIDATE**  
Tracker: #124  
Protocol contract: `PROTO-NET-STATUS-26_2-001`  
Admission session: `fb57c003d0e96c467dad55c209237dd23478ff287caea51943823cc62848cea0`

## Purpose

`crucible-server` is the first runnable product composition above the source-admitted Minecraft Java
26.2 target. R0 intentionally proves only:

```text
Handshake -> Status -> Status response -> Ping/Pong
```

It does **not** claim Login, Configuration, Join or Play compatibility.

The crate composes the already-qualified `PrePlayIo<Target26_2>` path with a small blocking
`std::net::TcpListener` policy. This is a compatibility probe boundary, not a commitment to the
long-term server runtime or executor architecture.

## Evidence boundary

The runnable server consumes no remembered/community packet identities. Packet IDs and finite R0
field semantics remain owned by `crucible-target-26-2`, whose generated constants are tied to the
reviewed source/capture contract and admission session above.

The first external Crucible probe deliberately returns the same deterministic status JSON used by
the admitted vanilla oracle capture:

```json
{"description":"Crucible R0 Oracle","players":{"max":20,"online":0},"version":{"name":"26.2","protocol":776},"enforcesSecureChat":true}
```

This keeps the constrained Crucible exchange directly comparable with the admitted oracle session.
Product-configurable presentation belongs after R0.

## Permanent qualification

`crucible-server` tests the production composition rather than a second protocol implementation.
Qualification includes:

- the complete admitted client stream coalesced into one transport read;
- every split point of the admitted client stream;
- clean EOF after a status-only request;
- a real loopback TCP exchange fragmented one byte at a time;
- exact byte equality with the admitted status response and pong frames.

The dedicated `R0 Server Gate` additionally requires rustfmt, locked compilation, Clippy with
`-D warnings`, and the localhost black-box tests.

## Running the probe candidate

From a clean checkout of the candidate commit:

```bash
cargo run --release --locked --package crucible-server -- --once 127.0.0.1:25565
```

Without `--once`, the R0 process accepts sequential status connections until terminated:

```bash
cargo run --release --locked --package crucible-server -- 127.0.0.1:25565
```

R0 deliberately uses a blocking sequential listener, a 15-second per-connection read/write timeout,
`TCP_NODELAY`, bounded ingress/egress, retained read scratch, and an explicit four-action service
budget. None of these choices selects the final high-concurrency runtime.

## Final unmodified-client probe

The final probe deliberately keeps the already-admitted oracle endpoint shape. The capture proxy
listens on `127.0.0.1:25566` and forwards to Crucible on `127.0.0.1:25565`. The admitted handshake
golden bytes already encode host `127.0.0.1` and port `25566`, so this topology requires no contract
change.

First prove the checkout is current and clean:

```bash
cd ~/Crucible
git fetch origin
git switch main
git pull --ff-only origin main
git status --short
git rev-parse HEAD
```

`git status --short` must print nothing. Keep every evidence output below outside the repository so
the checkout remains clean.

### Terminal A — Crucible

```bash
cd ~/Crucible
cargo run --release --locked --package crucible-server -- \
  --once 127.0.0.1:25565
```

### Terminal B — independent framed capture proxy

```bash
cd ~/Crucible
python3 tools/protocol_capture_proxy.py \
  --listen-host 127.0.0.1 \
  --listen-port 25566 \
  --upstream-host 127.0.0.1 \
  --upstream-port 25565 \
  --lock vanilla/vanilla.lock.toml \
  --output "$HOME/Downloads/crucible-r0-client-capture.json" \
  --max-frame-bytes 4096 \
  --max-stream-bytes 16384 \
  --max-frames 8 \
  --timeout-seconds 300
```

### Unmodified Minecraft Java 26.2 client

Use the official, unmodified Minecraft Java 26.2 client. In Multiplayer, add or direct-connect the
probe endpoint:

```text
127.0.0.1:25566
```

The server-list entry must visibly render the `Crucible R0 Oracle` status and complete its latency
ping without a protocol error. Retain a screenshot of that rendered entry as the UI evidence file.
A clean temporary multiplayer list is recommended if the normal list contains information that
should not be retained in the evidence image.

The capture proxy and `--once` Crucible process should both terminate after the status/ping
connection closes.

### Explicit operator observation

TCP bytes cannot prove that the GUI was an official unmodified client or that the status was visibly
rendered. Record those facts explicitly rather than pretending the capture inferred them:

```bash
cat > "$HOME/Downloads/crucible-r0-client-observation.json" <<'JSON'
{"schema":1,"kind":"r0-unmodified-client-observation-v1","minecraft":"26.2","client_distribution":"official","modified":false,"endpoint":"127.0.0.1:25566","server_list_visible":true,"status_rendered":true,"ping_completed_without_protocol_error":true}
JSON
```

Only set those booleans to `true` after directly observing them. A failed observation is evidence of
a real R0 failure and must not be edited into success.

### Final fail-closed admission

Set `UI_EVIDENCE` to the retained screenshot path and run:

```bash
cd ~/Crucible
UI_EVIDENCE="/path/to/the/retained-minecraft-screenshot.png"

python3 tools/r0_external_probe_admission.py \
  --repo-root . \
  --capture "$HOME/Downloads/crucible-r0-client-capture.json" \
  --observation "$HOME/Downloads/crucible-r0-client-observation.json" \
  --ui-evidence "$UI_EVIDENCE" \
  --output "$HOME/Downloads/crucible-r0-external-probe.json"
```

The admission tool independently requires:

- a clean Git worktree and canonical exact `HEAD`;
- the server's R0 admission-session constant to equal the sealed P0M report;
- the checked-in generated 26.2 target bytes to equal the sealed generated-Rust digest;
- the new client↔Crucible capture to pass `protocol_capture_admission.py` against
  `PROTO-NET-STATUS-26_2-001`;
- exactly three client→server and two server→client golden frames;
- official Minecraft Java 26.2, unmodified, through `127.0.0.1:25566`;
- server-list visibility, rendered status and ping completion all explicitly observed; and
- a retained non-empty bounded UI evidence file whose SHA-256 is bound into the report.

The output report is canonical JSON and carries its own deterministic `report_sha256`. Local paths,
timestamps and operator identity are deliberately excluded from the report identity.

## External exit gate

Do not close #124 or claim **R0 status compatibility** merely because repository CI is green.
A real unmodified Minecraft Java 26.2 client must still be run against this localhost process and
retain evidence that:

1. Crucible appears in the multiplayer server list;
2. the status response is accepted/rendered;
3. ping/pong completes without a protocol error;
4. the constrained Crucible exchange matches `PROTO-NET-STATUS-26_2-001`.

Only a green `r0_external_probe_admission.py` report permits the R0 compatibility claim and closure
of #124.
