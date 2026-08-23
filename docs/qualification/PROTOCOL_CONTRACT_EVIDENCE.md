# Protocol Contract Evidence

**Status:** admission tooling

## Purpose

Finite protocol facts are external compatibility data. They must not enter Crucible as remembered
packet IDs or hand-maintained magic bytes.

`tools/protocol_contract.py` validates a compact protocol-contract artifact after the relevant
official-source review has produced fingerprint-pinned `VAR_REVIEWED` sidecars and corresponding SEM
rules.

The validator is target-version-agnostic. It contains no Minecraft 26.2 packet identity or layout.

## Evidence chain

```text
pinned vanilla.lock.toml
        +
local official source + Atlas
        ↓
fingerprint-pinned VAR records
        ↓
human-reviewed SEM rules
        ↓
protocol-contract artifact
        ↓
protocol_contract.py
        ↓
validated finite packet identities + golden frames
        ↓
target Rust adapter
```

The target Rust adapter must consume admitted data. It must not become an independent second source
of packet constants.

## Artifact shape

Schema 1 is intentionally narrow:

```json
{
  "schema": 1,
  "id": "PROTO-EXAMPLE-001",
  "target": {
    "minecraft": "<exact lock value>",
    "protocol": 0,
    "source_archive_sha256": "<exact lock SHA-256>",
    "fingerprint_algorithm": "<exact lock algorithm>"
  },
  "packets": [
    {
      "name": "example-packet",
      "phase": "status",
      "direction": "serverbound",
      "id": 0,
      "semantic_rules": ["SEM-PROTOCOL-EXAMPLE-001"],
      "source_records": ["VAR-PROTOCOL-EXAMPLE-001"],
      "golden": {
        "body_hex": "00",
        "frame_hex": "0100"
      }
    }
  ]
}
```

The example is schema documentation, not a Minecraft packet claim.

## Admission rules

The validator fails closed unless all of the following hold:

- the artifact target exactly matches `vanilla/vanilla.lock.toml` for Minecraft version, protocol
  version, official source archive digest, and Atlas fingerprint algorithm;
- all object fields belong to schema 1;
- every cited source record exists, is `VAR_REVIEWED`, and uses the pinned fingerprint algorithm;
- every cited source record carries valid normalized/body SHA-256 fingerprints;
- every declared SEM rule is linked by at least one cited VAR record;
- packet names and `(phase, direction, id)` identities are unique;
- packet IDs fit a non-negative Minecraft VarInt;
- golden bytes are lowercase canonical hexadecimal with no whitespace or alternate spelling;
- the packet ID decoded from `body_hex` equals the declared ID and uses canonical VarInt encoding;
- `frame_hex` contains a canonical VarInt body length, has exactly that many remaining bytes, and
  those remaining bytes equal `body_hex` byte-for-byte.

Unknown fields are errors rather than silently ignored extensions.

## Why body and frame bytes are both retained

The packet body proves the target packet identity independently of outer framing. The complete frame
proves the exact reusable byte fixture expected at the stream boundary. Retaining both makes a
mismatch visible instead of allowing a fixture generator to become an unreviewed source of truth.

The generic wire/packet implementation still receives its own Rust qualification. This artifact is
external-contract evidence, not a replacement for executable codec tests.

## Use

After source review has produced the real artifact:

```bash
python3 tools/protocol_contract.py \
  vanilla/protocol/<contract>.json \
  --lock vanilla/vanilla.lock.toml \
  --records-root vanilla/records
```

A successful invocation prints only a compact admission summary. A failing invariant exits nonzero.

## Qualification

`tools/tests/test_protocol_contract.py` uses only temporary synthetic lock/VAR/contract data. It
covers target drift, type confusion, unknown fields, missing/unreviewed/stale source records,
unlinked SEM rules, duplicate identities, malformed hexadecimal, noncanonical VarInts, packet-ID
disagreement, and frame-length/body disagreement.

The ordinary Python tooling CI discovers these tests automatically.

## Non-goals

This layer does not decide protocol semantics, infer packet IDs, read Mojang source bodies, select a
socket runtime, implement packet handlers, or claim vanilla compatibility. Those claims remain at
the VAR/SEM/reference/integration layers.
