# R0 Status Admission Session

Status: **P0M qualification orchestration**

Parent: [Client P0 protocol spine](../architecture/PROTOCOL_CLIENT_SPINE.md)

P0M does not create Minecraft protocol knowledge. It composes the independent evidence gates that
must already exist before Crucible is allowed to claim one exact R0 target adapter is admitted.

## Why this exists

Running these commands separately is necessary but not sufficient:

```text
current-source gate
finite protocol-contract gate
black-box capture convergence
static target-adapter codegen check
```

Without an outer binding, it is possible to accidentally retain four individually valid results
that refer to different source revisions, contracts, captures, or generated outputs. P0M makes the
**evidence instance itself** explicit.

## Command

```bash
python3 tools/r0_status_admission.py \
  --db .crucible/vanilla/atlas.sqlite \
  --source-gate path/to/r0-status-source-gate.json \
  --contract path/to/PROTO-NET-STATUS-26_2-001.json \
  --capture path/to/preplay-status-capture.json \
  --generated-rust path/to/generated/status_26_2.rs \
  --output path/to/r0-status-admission.json
```

`--lock` defaults to `vanilla/vanilla.lock.toml` and `--records-root` defaults to
`vanilla/records`.

The Atlas database is generated local evidence and must be a real non-symlink file. The source
gate, contract, capture and generated target adapter retain their own stricter input rules.

## Admission sequence

The driver executes these boundaries in order:

```text
vanilla_source_gate.evaluate
        ↓ admitted only
exact Atlas target identity
        ↓
contract target equality
        ↓
every contract VAR ⊆ current gated VARs
        ↓
protocol_capture_admission.crosscheck_capture
        ↓ exact independent byte convergence
protocol_codegen.generate(..., check=True)
        ↓ exact generated-Rust bytes
compact R0 session identity
```

A downstream boundary is not executed after an upstream rejection.

## Cross-boundary invariants

### One exact target

The source-gate report and finite contract must agree on all of:

- Minecraft version;
- protocol version;
- official source archive SHA-256; and
- Atlas fingerprint algorithm.

The P0L convergence summary must then report the same Minecraft/protocol identity.

### Current source admission for every cited VAR

`protocol_contract.py` already requires cited VAR records to exist, be reviewed and support the
packet SEM rules. P0M adds a stronger current-run condition:

> Every VAR cited by the finite R0 contract must also occur in the current source-gate report's
> explicitly required method set.

This prevents a valid historical review record that was not re-admitted against the current Atlas
index from silently entering the R0 target contract.

The source gate may contain additional helper methods not cited directly by packet entries; the
subset relationship is intentionally one-way.

### Generated code is checked, not generated

P0M invokes protocol codegen with `check=True`. The session therefore cannot repair a stale target
adapter while qualifying it. Generation remains an explicit earlier action; admission only proves
the committed output is exactly current.

## Session report

The deterministic report binds:

- schema/kind;
- exact target identity;
- source gate id and gate SHA-256;
- each required VAR id, record digest, source identity, current normalized/body fingerprints and
  SEM links;
- finite contract id and its cited VAR set;
- independent P0K capture SHA-256 and matched directional frame counts;
- generated Rust SHA-256; and
- a canonical `session_sha256` over all prior fields.

Local filesystem paths are deliberately excluded from the session identity. The same evidence bytes
on two machines must produce the same report.

## Non-goals

P0M does **not**:

- inspect Mojang source bodies itself;
- infer packet semantics from black-box traffic;
- construct VAR, SEM or protocol-contract records;
- generate production packet constants during admission;
- claim cross-direction timing from P0K's independently captured streams; or
- prove that a Crucible executable serves the packets correctly.

The final point belongs to the R0 product adapter + real-client probe after the target evidence
session is admitted.

## Tests

`tools/tests/test_r0_status_admission.py` permanently checks the orchestration-specific failure
modes:

- deterministic successful session identity;
- source-gate rejection stops later gates;
- mismatch in every target-identity dimension;
- a contract citing a reviewed but currently ungated VAR;
- P0L capture disagreement;
- convergence-summary target disagreement;
- generated adapter drift;
- malformed required-method evidence; and
- unsafe/missing Atlas database identity.

The underlying source gate, contract validator, capture gate and code generator remain covered by
their own independent test suites.
