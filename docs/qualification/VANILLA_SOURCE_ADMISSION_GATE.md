# Vanilla Source Admission Gate

Status: **qualification infrastructure**  
Scope: narrow, source-backed implementation slices

## Purpose

Vanilla Atlas already separates discovery from semantic review. A dependency frontier can contain
hundreds of implementation methods, while a Crucible slice normally depends on a much smaller set
of exact source observations.

`tools/vanilla_source_gate.py` turns that distinction into a fail-closed admission boundary.

It does **not** declare an entire frontier semantic. It requires the implementation slice to name the
exact source methods it relies upon and proves that each one is still represented by the expected
version-controlled Vanilla Algorithm Record (VAR).

This gate has a different responsibility from `tools/protocol_contract.py`:

```text
local official source + Atlas
          ↓
 reviewed VAR + SEM
          ↓
vanilla_source_gate.py
  current-source admission
          ↓
 protocol contract artifact
          ↓
 protocol_contract.py
 finite-wire-data admission
          ↓
   target Rust adapter
```

The source gate proves that the reviewed semantic evidence is still attached to the current official
source. The protocol-contract gate proves that finite packet identities and golden frames are
canonical and bound to that reviewed evidence. Neither gate substitutes for the other.

## Gate input

A gate is a small JSON document:

```json
{
  "schema": 1,
  "id": "GATE-NET-STATUS-26_2-001",
  "frontier": "p0-protocol-client",
  "minimum_status": "VAR_REVIEWED",
  "require_semantic_rules": true,
  "require_hazards_reviewed": true,
  "methods": [
    {
      "query": "exact.qualified.Type#exactMethod(signature)",
      "var_id": "VAR-NET-STATUS-001"
    }
  ]
}
```

`query` must resolve to exactly one method in the generated Atlas index. Production gates should
prefer the exact `qualified type#signature` identity emitted by `record-template` rather than a
substring query.

The gate configuration itself is hashed into the output artifact.

## Admission law

For every required method, the gate requires all of the following:

1. the query resolves to exactly one current source method;
2. when a frontier is declared, the method is inside that frontier's resolved closure;
3. the named version-controlled VAR exists;
4. VAR type/signature exactly match the resolved source method;
5. fingerprint algorithm, normalized source fingerprint and raw body fingerprint match the current
   generated Atlas index;
6. the VAR status is at or beyond the configured minimum and is not `STALE`;
7. when requested, the VAR links to at least one Crucible `SEM-*` rule;
8. when requested, every Atlas hazard kind currently observed on the method is explicitly listed in
   `hazards_reviewed`.

Any failure rejects the gate. There is no best-effort mode.

The output also records:

- Minecraft/protocol/world version from the Atlas database;
- official source archive SHA-256;
- Atlas/fingerprint versions;
- gate SHA-256;
- frontier configuration SHA-256 and closure size;
- each admitted VAR record SHA-256;
- exact source fingerprints and SEM links;
- closure review-status distribution;
- unresolved call-site count inside the frontier.

The closure metrics are **diagnostic**. They help reviewers decide what to inspect next but do not
silently expand the semantic contract.

## Protocol 776 use

For the first Minecraft 26.2 status milestone the intended sequence is:

```bash
SRC="$HOME/Documents/mc-source/mc-src.zip"
DB=.crucible/vanilla/atlas.sqlite

python3 tools/vanilla_atlas.py --db "$DB" verify-source "$SRC" \
  --lock vanilla/vanilla.lock.toml
python3 tools/vanilla_atlas.py --db "$DB" index "$SRC"
python3 tools/vanilla_atlas.py --db "$DB" frontier p0-protocol-client --json
python3 tools/vanilla_atlas.py --db "$DB" next p0-protocol-client --limit 50
```

Reviewers then trace the exact handshake/status registration, packet codecs and listener behavior,
create fingerprint-pinned `VAR-NET-*` records using `record-template`, write the corresponding
`SEM-NET-*` contract, and create a narrow status source gate containing only the methods on which
that contract depends.

First admit the reviewed source evidence:

```bash
python3 tools/vanilla_source_gate.py \
  --db "$DB" \
  --gate vanilla/gates/network/GATE-NET-STATUS-26_2-001.json \
  --records vanilla/records \
  --output .crucible/evidence/GATE-NET-STATUS-26_2-001.json
```

Then admit the finite packet identities and golden frames that cite those reviewed VAR/SEM records:

```bash
python3 tools/protocol_contract.py \
  vanilla/protocol/PROTO-NET-STATUS-26_2-001.json \
  --lock vanilla/vanilla.lock.toml \
  --records-root vanilla/records
```

No target packet ID or field layout is admitted into Rust until **both** firewalls are green and the
separate black-box/client qualification agrees with the finite contract.

## Update behavior

A future Minecraft pin is expected to change source fingerprints. Re-indexing then makes an old
source gate fail until its required VARs are re-reviewed or explicitly confirmed against the new
source. The protocol contract is independently bound to the exact `vanilla.lock.toml` target and
therefore fails when the target identity changes.

This is intentional. Version upgrades should invalidate assumptions mechanically rather than rely on
maintainer memory.

## Non-goals

This tool does not:

- copy Mojang source into Crucible;
- infer semantics automatically;
- require an entire dependency frontier to become semantic law;
- replace black-box client/server fixtures;
- validate packet bytes by itself;
- make performance claims.

It is the bridge between **reviewed official-source evidence** and permission to construct the finite
protocol contract consumed by a narrow Crucible semantic slice.
