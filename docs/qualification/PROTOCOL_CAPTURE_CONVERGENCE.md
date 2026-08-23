# Protocol Capture / Contract Convergence

Status: **P0L admission gate**

Parent: [Client P0 protocol spine](../architecture/PROTOCOL_CLIENT_SPINE.md)

This gate joins two deliberately independent kinds of evidence without allowing either to take over
the other's job:

- the **finite protocol contract** obtains packet meaning, target identity and golden bytes from
  reviewed official-source VAR/SEM evidence;
- the **P0K black-box capture** observes the actual plaintext framed bytes exchanged by an
  unmodified client and a controlled vanilla server.

The capture is never a semantic oracle. P0L does not infer packet names, packet IDs, phases,
fields, state transitions or rules from traffic.

## Admission command

```bash
python3 tools/protocol_capture_admission.py \
  path/to/PROTO-NET-STATUS-26_2-001.json \
  path/to/preplay-capture.json
```

Optional `--lock` and `--records-root` arguments exist for isolated qualification fixtures. The
normal repository invocation uses `vanilla/vanilla.lock.toml` and `vanilla/records`.

## Required evidence chain

P0L first invokes the existing protocol-contract firewall. Therefore a contract cannot reach the
black-box comparison unless it already proves:

1. exact Minecraft/protocol/source/fingerprint identity;
2. reviewed VAR provenance;
3. VAR → SEM linkage for every declared semantic rule;
4. unique `(phase, direction, packet id)` identities;
5. canonical packet-id and frame VarInts; and
6. internally consistent golden packet bodies and frames.

P0L then independently revalidates the P0K artifact instead of trusting the capture producer's
metadata. It checks:

- exact capture schema and kind;
- strict unknown-field rejection at artifact, target, limits, stream and frame layers;
- target identity equality with the admitted contract;
- canonical top-level `capture_sha256`;
- exactly one client→server and one server→client stream;
- configured positive bounds;
- contiguous frame ordinals and byte offsets;
- frame/body byte counts;
- per-frame SHA-256;
- canonical length-prefix VarInts and exact framed body bytes;
- per-direction stream byte count and SHA-256; and
- capture limits against the materialized evidence.

Only after both sides pass independently does convergence run.

## Convergence rule

Contract directions map mechanically:

| Contract | Capture |
|---|---|
| `serverbound` | `client-to-server` |
| `clientbound` | `server-to-client` |

Within each direction, contract declaration order is retained. The captured sequence must contain
**exactly** the same number of frames, and each frame and body must match the corresponding golden
bytes exactly. Missing, additional, reordered or altered frames fail admission even when the
capture has been made internally self-consistent by recomputing all hashes.

P0L v1 is intentionally restricted to contract phases `handshake` and `status`. It must not be used
to bless login/configuration traffic once encryption or compression may have changed the observed
wire boundary.

## What the gate does not prove

P0K records the two TCP directions independently. There is intentionally no wall-clock timestamp or
shared global frame ordinal in the artifact. Therefore P0L does **not** claim a global
cross-direction temporal order such as “request definitely preceded response” from capture alone.
That ordering is a semantic/session-state fact and remains source-backed.

The compact success summary contains only:

- contract id;
- capture digest;
- target Minecraft/protocol identity; and
- matched frame counts.

It contains no packet identity or semantic claim learned from the black-box capture.

## Adversarial qualification

`tools/tests/test_protocol_capture_admission.py` covers:

- valid source/capture convergence;
- strict unknown-field rejection;
- capture-digest tampering;
- fully self-consistent wrong-target artifacts;
- booleans masquerading as integer fields;
- reordered frames with recomputed hashes;
- extra and missing frames;
- altered but canonically framed payloads with recomputed metadata;
- independent stream/frame metadata corruption;
- noncanonical frame-length VarInts;
- contracts that fail the source-admission firewall;
- non-R0 phases; and
- duplicate/missing directional streams.

## R0 use

For Minecraft 26.2 R0, the intended sequence is:

```text
review pinned official source
        ↓
VAR-NET-* + SEM-NET-*
        ↓
PROTO-NET-STATUS-26_2-001
        ↓
protocol_contract.py + vanilla_source_gate.py
        ↓
independent P0K client↔vanilla capture
        ↓
protocol_capture_admission.py
        ↓
static protocol_codegen.py adapter
        ↓
Crucible localhost status server + unmodified-client probe
```

A green P0L result permits the independent source and black-box byte evidence to be cited together.
It does not itself select or generate any target packet semantics.
