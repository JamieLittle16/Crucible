# Login Source Review

**Target:** Minecraft 26.2 / protocol 776  
**Parent:** #78  
**Tracking:** #115  
**Status:** discovery/review procedure; no packet-law or authentication-policy claim

This review covers only the path needed to carry an ordinary local-development client from the
initial handshake into Login and through the source-backed handoff to Configuration.

`vanilla/frontiers/p0-login.json` is a narrow discovery frontier. It is not a packet inventory and
has no semantic authority. Exact methods become evidence only when the review explicitly relies on
them and a current-source gate admits their fingerprint-pinned VAR records.

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

The pin must verify as Minecraft 26.2 / protocol 776 before any review result is admitted.

## Narrow Login inventory

```bash
python3 tools/vanilla_atlas.py \
  --db "$DB" \
  frontier p0-login --json

python3 tools/vanilla_atlas.py \
  --db "$DB" \
  next p0-login --limit 50
```

Reachability and ranking are review leads only. They do not make a method semantic.

## Exact-method review

For every method actually required to settle the Login contract:

```bash
python3 tools/vanilla_atlas.py --db "$DB" show '<exact method query>'
python3 tools/vanilla_atlas.py --db "$DB" deps '<exact method query>'
python3 tools/vanilla_atlas.py --db "$DB" callers '<exact method query>'
python3 tools/vanilla_atlas.py --db "$DB" record-template '<exact method query>'
```

Inspect the pinned source directly when Atlas metadata cannot settle registration, codec, branch,
ordering, authentication/profile behavior or listener transition. Commit only Crucible-authored
fingerprints, VAR/SEM records and independent descriptions—not Mojang source bodies.

## Questions the Login review must answer

The finite review must establish from evidence:

1. the exact Handshake → Login admission condition used by the supported first local-development
   route;
2. the exact required Login packet identities, directions, field order, field codecs and limits;
3. the supported first authentication/profile policy and which source-observed branches are
   mandatory for it;
4. whether compression, encryption, session authentication, acknowledgement or profile validation
   participates in that route, and at which observable boundary;
5. the externally meaningful username, UUID/profile and duplicate-session requirements;
6. the exact completion condition and Login → Configuration handoff;
7. required packet/session ordering and exact-payload exhaustion;
8. malformed, duplicate, trailing, wrong-phase and disconnect behavior relevant to compatibility;
9. which implementation details are `IMPLEMENTATION_ONLY` and therefore must not constrain
   Crucible.

Unknown questions remain `UNKNOWN`. They are never completed from memory or community protocol
references.

## Evidence products

The reviewed route should produce only the finite evidence it actually needs:

```text
VAR-NET-LOGIN-*
SEM-NET-LOGIN-*
GATE-NET-LOGIN-26_2-001
PROTO-NET-LOGIN-26_2-001
```

The existing evidence firewall remains authoritative:

```text
current source gate
      +
finite protocol-contract validation
      +
independent black-box convergence where observable
      +
static Rust codegen check
      ↓
admitted Login target adapter
```

A later product integration may choose an offline/local-development route first only if the reviewed
source and declared support policy make that route valid. This document does not pre-decide that
question.

## Scope boundary

This review does not own Configuration registries/tags/features, Play packets, chunks, lighting,
movement, world data or gameplay. The only Configuration concern allowed here is the exact
source-backed handoff boundary needed to finish Login.
