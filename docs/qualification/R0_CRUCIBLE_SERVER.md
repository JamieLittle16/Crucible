# Crucible R0 Localhost Server

Status: **R0 EXTERNAL-PROBE CANDIDATE**  
Tracker: #124  
Protocol contract: `PROTO-NET-STATUS-26-2-001`  
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

## External exit gate

Do not close #124 or claim **R0 status compatibility** merely because repository CI is green.
A real unmodified Minecraft Java 26.2 client must still be run against this localhost process and
retain evidence that:

1. Crucible appears in the multiplayer server list;
2. the status response is accepted/rendered;
3. ping/pong completes without a protocol error;
4. the constrained Crucible exchange matches `PROTO-NET-STATUS-26-2-001`.

The external probe evidence should record the exact Crucible commit and admission-session identity.
Only after that probe is admitted may #124 close.
