# Crucible R1A Login Server Composition

Status: **R1A PRODUCT-COMPOSITION CANDIDATE**  
Tracker: #146 / #143  
Protocol contract: `PROTO-NET-LOGIN-26-2-001`

## Purpose

This slice wires Crucible's already-admitted Minecraft Java 26.2 Login target into the actual
`crucible-server` executable. It proves the product can compose:

```text
TCP
 -> bounded framing / I/O
 -> Handshake
 -> Login Hello
 -> LoginFinished
 -> LoginAcknowledged
 -> Configuration boundary
```

It does **not** claim R1 completion. Configuration packet identities/codecs and the first
Play/bootstrap surface remain gated by #158/#146.

Without an explicit Login session epoch, the executable remains the existing R0 Status server.

## Server session epoch ownership

Official source places the `LoginFinished` session UUID on the server connection population rather
than generating a new UUID for each accepted socket. Crucible therefore models this as one
`ServerSessionEpoch` owned by listener/product composition and copied into every R1A connection.

The protocol target does not call an RNG. Entropy/bootstrap policy is deliberately separate from
packet semantics.

The repository currently has no external Rust dependencies. Rather than introduce an unreviewed
randomness dependency or invent a weak local RNG, the R1A development executable requires an
explicit RFC-4122 version-4 epoch encoded as exactly 32 hexadecimal digits:

```text
--login-session-epoch=<32 hex digits>
```

This is a development/qualification seam, not the final operator-facing configuration.

## Strict Configuration boundary

While Configuration is not admitted, the product uses exactly one semantic action per I/O service
iteration. This prevents a subtle boundary bug:

```text
one socket read
  contains LoginAcknowledged
  + first Configuration frame
```

Crucible commits only `LoginAcknowledged`, observes the transition to Configuration, and leaves the
following bytes buffered and untouched. The tests permanently cover this coalesced-input case.

Once R1B is admitted, the same connection/I/O object can continue forward; no new framing or socket
stack is required.

## Permanent tests

The composition tests require:

- exact parsing of the admitted source-shaped version-4 server epoch;
- fail-closed rejection of wrong UUID version/variant bits;
- byte-exact `LoginFinished` through the production server composition;
- preservation of already-read Configuration bytes after `LoginAcknowledged`;
- identical session-epoch bytes across two independent connections using one listener epoch.

These tests reuse generated admitted Login evidence rather than implementing a second packet oracle.

## Running the R1A development boundary

The following epoch is the version-4 session UUID used by the admitted Login golden fixture and is
convenient for deterministic local qualification:

```bash
cargo run --release --locked --package crucible-server -- \
  --login-session-epoch=4d7f604f196a43b08987f0b2a27c2663 \
  127.0.0.1:25565
```

For a one-connection probe:

```bash
cargo run --release --locked --package crucible-server -- \
  --once \
  --login-session-epoch=4d7f604f196a43b08987f0b2a27c2663 \
  127.0.0.1:25565
```

An official 26.2 client should complete the admitted Login exchange. The server then logs a
`ConfigurationReady` exit and closes because R1B is intentionally not implemented on this branch.
That disconnect is expected and is **not** an R1 success claim.

## Exit condition

This slice may merge when repository CI and R0 non-regression gates are green. It does not depend on
a real-client Configuration probe because it deliberately stops before Configuration semantics.

R1 still requires:

```text
#158 source admission
 -> GATE-NET-CONFIG-26_2-001
 -> finite Configuration contract/codegen
 -> Target26_2 Configuration state
 -> bounded publication integration
 -> minimum Play bootstrap
 -> official-client Crucible probe reaching Play
```
