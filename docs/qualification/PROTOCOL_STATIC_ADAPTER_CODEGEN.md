# Protocol Static Adapter Code Generation

**Status:** P0 qualification infrastructure  
**Scope:** finite admitted protocol contracts -> compile-time Rust packet identity

## Purpose

`tools/protocol_codegen.py` is the boundary between a source-backed, fingerprint-bound protocol
contract and target-specific Rust packet identity.

It does **not** discover Minecraft protocol facts. It may only consume a contract that already
passes `tools/protocol_contract.py`, which in turn requires the exact target lock, reviewed VAR
records, linked SEM rules and canonical golden packet/frame bytes.

The pipeline is:

```text
pinned official source
        ↓
Atlas + explicit source admission
        ↓
reviewed VAR + SEM
        ↓
protocol contract + golden bytes
        ↓
protocol_contract.py
        ↓
protocol_codegen.py
        ↓
static Rust constants
```

This keeps source interpretation, evidence admission and executable packet identity separate.

## Runtime law

Generated production Rust contains only compile-time data:

- target/contract identity constants;
- one nested module per present protocol phase and direction;
- direct `i32` packet-ID constants.

There is no generated runtime packet map, service locator, trait object, lazy initializer, heap
allocation or dynamic registry lookup.

The intended target usage is therefore structurally similar to:

```text
status::serverbound::STATUS_REQUEST
status::clientbound::STATUS_RESPONSE
```

rather than:

```text
registry.lookup(phase, direction, name)
```

Package/version modularity is resolved before the packet HOT path.

## Golden evidence

Canonical golden body/frame bytes are emitted only below `#[cfg(test)]`.

They are retained so the generated target adapter can be qualified against the exact bytes admitted
by the external protocol contract, while normal production builds do not carry those fixtures.

## Determinism

Code generation canonicalizes:

- phase order;
- direction order;
- packet identity order;
- SEM/source-record list order;
- JSON object key order for the generator identity.

Formatting or irrelevant list ordering in the input artifact therefore cannot create a different
Rust result. `CONTRACT_SHA256` binds the canonical admitted semantic/evidence content rather than the
source JSON's whitespace.

Generated output supports a `--check` mode. CI/release workflows should byte-compare regenerated
Rust against the committed target adapter once a real target contract exists.

## Fail-closed rules

Generation fails when:

- the upstream protocol contract does not pass admission;
- a code-generation string cannot be represented by the intentionally narrow ASCII target surface;
- packet names collide after Rust constant normalization;
- check mode sees a missing, symlinked or byte-drifted generated file;
- generation would replace a symlink output.

The generator does not weaken or reinterpret packet IDs, golden bytes, VAR links or SEM links.
Those remain owned by the contract validator.

## Qualification

`tools/tests/test_protocol_codegen.py` uses synthetic protocol data only. It verifies:

- admitted contracts render direct static constants;
- runtime output contains no `HashMap`, `BTreeMap`, `dyn`, `Vec`, `Box`, `Arc`, `Mutex`, `OnceLock`
  or lazy-registry mechanism;
- golden payloads remain test-only;
- packet/evidence ordering is canonical;
- check mode detects byte drift and symlink substitution;
- invalid contracts fail before output is written;
- generated source compiles both as a library and with `#[cfg(test)]` whenever `rustc` is available;
- the ASCII code-generation boundary is explicit.

No Minecraft 26.2 packet ID, layout or inferred Mojang semantic is introduced by this qualification
slice.

## Next gate

For the first real target adapter:

1. run the pinned 26.2 Atlas/source-admission review for handshake/status;
2. create reviewed VAR/SEM records;
3. create the finite protocol contract with canonical golden bytes;
4. validate the contract;
5. generate and commit the target Rust adapter;
6. byte-check regeneration in CI;
7. bind the adapter into the already-qualified connection/session spine;
8. qualify an unmodified 26.2 client status request + ping/pong over localhost.

Only after that external probe passes should Crucible claim R0 server-list compatibility.
