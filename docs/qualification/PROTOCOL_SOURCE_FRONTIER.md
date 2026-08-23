# P0 protocol source frontier

**Target:** Minecraft 26.2 / protocol 776  
**Parent:** #78  
**Status:** source-discovery configuration; no packet-law claim

Crucible does not obtain pre-play protocol semantics from community packet tables or from remembered packet IDs. The P0 client spine is excavated from the locally pinned official source archive through Vanilla Atlas and then frozen into human-reviewed VAR/SEM records.

`vanilla/frontiers/p0-protocol-client.json` defines the initial dependency frontier for that excavation. Its root names are **discovery anchors only**. A root resolving in the Atlas does not make every reachable Java implementation detail semantic, and a root that changed name in the pinned source must be replaced from actual Atlas evidence rather than guessed around.

## Local review workflow

The official source archive remains outside Git history. With the already pinned 26.2 archive and generated Atlas database:

```bash
SRC="$HOME/Documents/mc-source/mc-src.zip"

python3 tools/vanilla_atlas.py \
  --db .crucible/vanilla/atlas.sqlite \
  verify-source "$SRC" \
  --lock vanilla/vanilla.lock.toml

python3 tools/vanilla_atlas.py \
  --db .crucible/vanilla/atlas.sqlite \
  index "$SRC"

python3 tools/vanilla_atlas.py \
  --db .crucible/vanilla/atlas.sqlite \
  frontier p0-protocol-client --json

python3 tools/vanilla_atlas.py \
  --db .crucible/vanilla/atlas.sqlite \
  next p0-protocol-client --limit 50
```

The frontier report is an inventory, not a semantic specification. Reviewers then use `show`, `deps`, `callers`, and `record-template` to pin the exact methods that establish observable behavior.

## Required P0 evidence

Before Crucible hard-codes target-version packet law, human-reviewed records must establish at least:

- connection-state transitions from initial handshake into status or login;
- packet registration/order identities for protocol 776;
- handshake field order, types and limits;
- outer framing and string limits relevant to those packets;
- status request/response and ping/pong payload law;
- login and configuration transition points needed to reach play;
- any compression/encryption boundary that changes subsequent framing;
- disconnect behavior for malformed, invalid-state or unsupported-version input where client-observable.

For each claimed rule, the committed VAR records pin normalized source fingerprints. SEM records state only the client-observable contract Crucible intends to preserve. Implementation architecture, executor choice and buffer layout remain independent.

## Qualification route

```text
pinned official source
    ↓
Atlas frontier / next-review ranking
    ↓
fingerprint-pinned VAR records
    ↓
protocol SEM rules
    ↓
simple packet reference codec
    ↓
golden byte fixtures + fragmented/coalesced replay
    ↓
real vanilla client/status oracle
    ↓
optimized connection path
```

The production connection path may later use pooled buffers, vectored I/O or specialized dispatch, but those mechanisms must remain equivalent to the source-backed reference behavior and must earn their complexity with whole-path measurements.

## Scope discipline

This frontier intentionally excludes `net.minecraft.network.protocol.game`. P0 source work should first make handshake/status/login/configuration precise and get an unmodified client into play. Gameplay packet breadth is a later frontier and should not inflate the initial connection milestone.
