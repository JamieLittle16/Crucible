# Configuration Source Review

**Target:** Minecraft 26.2 / protocol 776  
**Parent:** #78  
**Tracking:** #116  
**Status:** discovery/review procedure; no packet-law or registry-payload claim

This review covers the Configuration phase required before an ordinary client may enter Play.

`vanilla/frontiers/p0-configuration.json` intentionally contains only a small set of discovery
anchors: the generic connection/protocol surface, the Login completion boundary, Configuration
completion packets and the server Configuration listener. It is not a packet inventory and has no
semantic authority.

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

The source must verify as Minecraft 26.2 / protocol 776 before review proceeds.

## Narrow Configuration inventory

```bash
python3 tools/vanilla_atlas.py \
  --db "$DB" \
  frontier p0-configuration --json

python3 tools/vanilla_atlas.py \
  --db "$DB" \
  next p0-configuration --limit 50
```

The dependency closure and ranking are discovery aids only. Exact methods enter semantic evidence
only after deliberate review and current-source admission.

## Exact-method review

For every method actually required to establish Configuration behavior:

```bash
python3 tools/vanilla_atlas.py --db "$DB" show '<exact method query>'
python3 tools/vanilla_atlas.py --db "$DB" deps '<exact method query>'
python3 tools/vanilla_atlas.py --db "$DB" callers '<exact method query>'
python3 tools/vanilla_atlas.py --db "$DB" record-template '<exact method query>'
```

Inspect the pinned source directly where Atlas metadata cannot settle registration, codec, ordering,
registry/data payload construction or listener transition. Commit only Crucible-authored records and
fingerprints, never Mojang source bodies.

## Questions the Configuration review must answer

The finite source review must establish from evidence:

1. the exact Login → Configuration entry condition relevant to the supported route;
2. which Configuration-phase packet identities and ordering constraints are required before an
   ordinary 26.2 client will complete the phase;
3. the registry/data payloads actually required by that client and their external semantic shape;
4. required tags, enabled features, known-pack negotiation, resource-pack behavior or equivalent
   target-visible configuration mechanisms, without assuming any are required until source evidence
   says so;
5. client-information and acknowledgement rules relevant to the supported route;
6. the exact Configuration completion exchange and Configuration → Play transition;
7. exact payload exhaustion and malformed/missing/duplicate/wrong-phase behavior relevant to
   compatibility;
8. which source details are `IMPLEMENTATION_ONLY` and must not constrain Crucible.

Any unresolved item remains `UNKNOWN`. Review does not fill gaps from community packet tables or
memory.

## Evidence products

The final reviewed slice should contain only finite required evidence:

```text
VAR-NET-CONFIG-*
SEM-NET-CONFIG-*
GATE-NET-CONFIG-26_2-001
PROTO-NET-CONFIG-26_2-001
```

The admitted target adapter must pass the same evidence chain already built for R0:

```text
current source gate
      +
finite protocol-contract validation
      +
independent capture/replay convergence where observable
      +
static Rust codegen check
      ↓
admitted Configuration target adapter
```

## Scope boundary

This review does not own Play packet semantics, player state, dimension bootstrap, chunks, lighting,
movement, collision, world persistence or gameplay. It may establish only the exact transition into
Play; Play itself is a separate source-backed vertical slice.
