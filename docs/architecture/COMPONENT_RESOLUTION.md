# Component Resolution and Static Composition

Status: **M0.1A foundation**  
Parent: #2  
Follow-up: M0.1B HOT composition qualification after the first production section policy is admitted by #19.

## Architectural law

Crucible separates **installation-time choice** from **runtime dispatch**.

```text
package manifests + operator profile
            ↓ cold path
strict deterministic resolver
            ↓
content-addressed Crucible.lock
            ↓
generated concrete Rust wiring
            ↓
ordinary statically typed engine code
```

A package system is not permission to insert a runtime service registry into HOT engine paths.

> Everything may be replaceable at the composition boundary; indirection is not automatically part of the runtime boundary.

For a HOT exactly-one capability, generated wiring should normally be a concrete type import/re-export or equivalent compile-time specialization. Runtime hash maps, string lookup, mandatory `dyn Trait`, reflection, plugin calls, or repeated capability resolution require a separate qualification proving that their total value exceeds their HOT cost.

## M0.1A scope

M0.1A admits the cold composition substrate before the first optimized production section provider has been selected.

It provides:

- schema-1 component manifests;
- structurally versioned capability identities;
- exact provider cardinality;
- package trust and semantic-fidelity policy;
- qualification/deviation admission rules;
- Minecraft-target compatibility;
- deterministic dependency closure;
- exact profile-selected providers;
- content-addressed `Crucible.lock`;
- generated static Rust composition wiring;
- byte-for-byte CI regeneration checks.

The first admitted manifest is the independent reference section implementation. It provides:

```text
world.section-store/1
```

through:

```text
crucible_world_reference::DirectBlockSection
```

The generated `crucible-composition` crate directly re-exports that concrete type. There is no runtime component lookup in this path.

## Deliberate unresolved profiles

The repository already carries four official strict profile intents:

- `reference`;
- `balanced`;
- `performance`;
- `memory`.

Only `reference` selects a section provider in M0.1A.

`balanced`, `performance`, and `memory` deliberately remain without a section-store selection until M0.3D/#19 finishes the real correctness + representative-population + synthetic-tail + target-hardware + Pareto decision.

This prevents the composition layer from quietly pre-selecting `direct`, `adaptive`, `fast-local`, or `packed-local` before the representation laboratory has earned that decision.

An unresolved profile is an intent document, not a runnable composition. Generating a production composition from an unresolved profile must fail closed rather than emit an empty engine.

## Component manifest contract

A component manifest is `component.toml` beneath `components/`.

Schema 1 binds at least:

```text
package identity/version
package kind
trust class
semantic fidelity
qualification state
local Cargo package/crate identity
semantic deviation records
local qualification records
supported Minecraft targets
provided versioned capabilities
provider cardinality
cost class
concrete generated Rust export/type
required capabilities
```

Unknown fields are rejected. This is intentional: a typo in a security/fidelity field must not silently become ignored metadata.

### Capability identity

Capabilities use:

```text
<semantic-name>/<positive-version>
```

For example:

```text
world.section-store/1
```

The Rust vocabulary stores the semantic name and version separately. An unversioned name is not a complete compatibility identity.

### Cardinality

Schema 1 supports:

- `exactly-one`;
- `many`.

An `exactly-one` capability with zero or multiple selected providers is invalid. Components providing the same exact capability must agree on cardinality.

## Profile policy

Profiles bind a Minecraft target and semantic/security policy independently of provider implementation.

Current controls include:

```text
fidelity
allow_unqualified
allow_third_party_native
allow_semantic_deviations
```

For the existing strict official profiles:

- relaxed semantic components are forbidden;
- unqualified components are forbidden;
- third-party native components are forbidden;
- declared semantic deviations are forbidden.

Future profiles may deliberately loosen a policy, but the relaxation must be explicit in the profile bytes and therefore changes composition identity.

## Qualification provenance

A package declaring `qualified = true` must cite local qualification records that exist in the repository.

The resolver does not infer qualification from package naming, code ownership, or trust class. `engine-native` means where the code belongs in the trust model; it does not automatically make the implementation semantically/performance qualified.

The component manifest bytes are included in the composition identity, so changing provenance, policy, capability declaration, or even other manifest content changes `Crucible.lock` identity.

## Deterministic lock identity

`Crucible.lock` binds the resolved composition to at least:

- resolver schema and engine SPI version;
- Minecraft target;
- exact profile-file SHA-256;
- pinned Rust toolchain;
- generated target-data generation SHA-256;
- exact selected package versions;
- exact component-manifest SHA-256 values;
- trust/fidelity/qualification/deviation state;
- qualification-record paths;
- exact versioned provider identities;
- generated Rust export/type identity;
- cost class;
- canonical composition SHA-256.

The lock is generated output. Hand editing is invalid.

## Generated Rust wiring

`crates/crucible-composition` is also generated output.

For the M0.1A reference composition its HOT section-store wiring is intentionally equivalent to:

```rust
pub use crucible_world_reference::DirectBlockSection as SectionStore;
```

This is a compile-time name binding, not a wrapper call.

Generated files are kept in the Cargo workspace so normal `check`, `clippy`, tests, and rustdoc exercise the exact composition that the lock describes. They are excluded from handwritten `rustfmt` ownership for the same reason as other generated artifacts: one generator owns the bytes.

## CI firewall

Ordinary CI runs:

```text
python3 tools/composition_resolver.py check \
  --repo-root . \
  --profile profiles/reference.toml
```

The check rebuilds the expected lock and generated crate in memory and requires byte-for-byte equality with the committed outputs.

Permanent adversarial tests cover at least:

- unversioned/zero-version capability IDs;
- unknown profile/component fields;
- relaxed component in a strict profile;
- unqualified component rejection;
- semantic-deviation rejection;
- forbidden third-party native code;
- missing qualification records;
- repository path escape;
- ambiguous dependency providers;
- deterministic dependency closure;
- generated-output determinism;
- generated-file drift;
- profile-byte and manifest-byte composition-identity changes.

## M0.1B exit gate

M0.1A does **not** satisfy the complete M0.1 milestone by itself.

After #19 freezes the first production `world.section-store/1` provider, M0.1B must:

1. add the admitted production component manifest and qualification provenance;
2. resolve the appropriate official `balanced`/`performance`/`memory` profiles without inventing mechanism choices;
3. generate the corresponding static wiring/lock identities;
4. create a hand-wired baseline and generated-composition baseline for the same representative HOT operation;
5. benchmark them under the normal performance qualification discipline;
6. require no meaningful HOT dispatch penalty attributable to composition;
7. redesign the composition boundary if that requirement fails.

Only then is #2 complete.
