# R0 Status Source Review

**Target:** Minecraft 26.2 / protocol 776  
**Parent:** #78  
**Status:** discovery/review procedure; no packet-law claim

The first external Crucible milestone is deliberately narrower than the complete pre-play protocol:
an unmodified 26.2 client must be able to discover Crucible in the multiplayer list and complete the
status request plus ping/pong exchange.

`vanilla/frontiers/r0-status.json` narrows the local official-source review to the connection,
handshake and status/ping anchors already present in the broader
`vanilla/frontiers/p0-protocol-client.json` frontier. It introduces no new target identities and no
packet constants.

## Local source preparation

The official source archive remains outside Git history.

```bash
SRC="$HOME/Documents/mc-source/mc-src.zip"
DB=.crucible/vanilla/atlas.sqlite

python3 tools/vanilla_atlas.py \
  --db "$DB" \
  verify-source "$SRC" \
  --lock vanilla/vanilla.lock.toml

python3 tools/vanilla_atlas.py \
  --db "$DB" \
  index "$SRC"
```

The pinned source must verify as Minecraft 26.2 / protocol 776 before review proceeds.

## Narrow R0 inventory

```bash
python3 tools/vanilla_atlas.py \
  --db "$DB" \
  frontier r0-status --json

python3 tools/vanilla_atlas.py \
  --db "$DB" \
  next r0-status --limit 50
```

The frontier report and ranking are **review leads only**. Reachability is not semantic evidence.
Methods enter the final R0 source gate only when the review explicitly relies on them.

## Method review

For each candidate method actually needed to establish R0 behavior, use the normal Atlas review
commands:

```bash
python3 tools/vanilla_atlas.py --db "$DB" show '<exact method query>'
python3 tools/vanilla_atlas.py --db "$DB" deps '<exact method query>'
python3 tools/vanilla_atlas.py --db "$DB" callers '<exact method query>'
python3 tools/vanilla_atlas.py --db "$DB" record-template '<exact method query>'
```

Review the source directly from the pinned local archive where Atlas metadata alone cannot settle a
branch, field codec, registration identity or listener transition. Commit only Crucible-authored
VAR/SEM records and fingerprints—not Mojang source bodies.

## Questions R0 must answer

The finite review must establish, without copying Mojang architecture into Crucible:

1. the exact initial handshake packet body fields, order and limits needed by an ordinary client;
2. how the handshake selects the status state;
3. the exact protocol-776 packet identities for status request/response and ping/pong;
4. the exact status request body law;
5. the exact status response body codec and externally observable fields required by the client;
6. the exact ping request and pong response payload law;
7. the source-backed state/order constraints for the R0 exchange;
8. malformed/invalid-state behavior that is client-visible and belongs in the R0 compatibility
   contract;
9. which observed details are implementation-only and therefore must **not** constrain Crucible.

Any unresolved question remains `UNKNOWN`; it is not filled from memory or community protocol
references.

## Evidence products

The source session should produce only the exact reviewed artifacts needed by the target contract:

```text
VAR-NET-*
SEM-NET-*
R0 source-gate JSON listing exact required methods
```

The gate must then pass `tools/vanilla_source_gate.py` against the same generated Atlas database.

Separately, P0K captures one real unmodified 26.2 client↔vanilla status session. The eventual
`PROTO-NET-STATUS-26_2-001` is constructed from the source review, not inferred from the capture.
P0L requires its golden bytes to converge with the independent capture, and P0M binds the source
gate, finite contract, capture and generated static Rust adapter into one session identity.

## Why this is separate from the broader P0 frontier

Login and configuration remain necessary for the later **Join/Play** milestone, but they are not
needed to answer the first server-list question. Keeping R0 source review small:

- shortens the human review queue;
- reduces the number of source fingerprints that can stale the first milestone;
- makes protocol mistakes easier to localize;
- prevents configuration complexity from delaying the first real-client oracle; and
- does not prevent the broader P0 frontier from continuing immediately after R0.

This is development-scope optimization, not semantic simplification: the ordinary client still
decides whether the admitted R0 behavior is correct.
