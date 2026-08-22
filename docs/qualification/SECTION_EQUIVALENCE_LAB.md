# Section Equivalence Laboratory

Status: **M0.3C qualification infrastructure**  
Target: Minecraft Java **26.2**, protocol **776**, data/world version **4903**  
Parent issue: **#18**

## Purpose

This laboratory turns block-section correctness into repeatable evidence rather than candidate-local unit tests.

The evidence chain is:

```text
OFFICIAL 26.2 SOURCE / RUNTIME
            ↓
SEM-WORLD-SECTION-* rules
            ↓
crucible-world-reference::DirectBlockSection
            ↓
versioned deterministic traces
            ↓
all admitted live CPU candidates
            ↓
EQUIV-WORLD-SECTION-* evidence
```

No optimized candidate is accepted because it agrees with another optimized candidate.

## Stable trace format

Schema `CRUCIBLE-SECTION-TRACE|1` is line oriented and deliberately dependency-free.

Operations:

- `G|cell` — exact point get;
- `R|cell|state` — replacement with exact previous-state check;
- `N|cell` — same-state replacement at the current reference value;
- `S` — summary observation;
- `C|state` — conservative membership query;
- `K` — full semantic checkpoint.

Every generated trace is encode/decode round-tripped before execution. A future incompatible trace change requires a new schema version.

## Mandatory deterministic trace classes

The quick/full generators include:

1. all-air stability;
2. one-cell mutation/reversal;
3. localized low-cardinality churn;
4. random uniform-state writes;
5. target-universe high-entropy writes;
6. repeated dead-palette-slot churn;
7. exact 16/17 simultaneously-live boundary;
8. exact 256/257 simultaneously-live boundary;
9. long seeded target-state traces.

All 16 raw synthetic mutation-fact bit inputs are exercised separately through `SectionStateFacts::new`, including inputs that canonicalize to the same legal semantic fact set.

## Checkpoint invariants

At every explicit checkpoint:

- all 4096 candidate cells equal the direct reference;
- reference incremental summaries equal full recomputation;
- candidate summaries equal that recomputed image;
- prior replacement return values have already matched operation-by-operation;
- `maybe_contains == false` has never hidden an actually present state.

Same-state mutations are additionally required to preserve summaries immediately.

Each candidate is cloned and mutated independently to cover `SEM-WORLD-SECTION-014` copy independence.

## Target-data qualification

Before candidate traces run, every generated Minecraft 26.2 state is checked against committed `STATE_MUTATION_FLAGS` through the public `GeneratedStateFacts` lookup. The evidence record links both:

- `STATE_DATA_INPUT_SHA256`;
- `STATE_DATA_GENERATION_SHA256`.

This prevents a section mechanism from qualifying against a fact provider that no longer agrees with the committed generated target table.

## Candidate set

The harness currently qualifies:

- `direct` — `DirectNBlockSection`;
- `adaptive` — `Uniform -> Local4Stable -> Local8Stable -> DirectN`;
- `fast-local` — `Uniform -> Local8Stable -> DirectN`;
- `packed-local` — `Uniform -> Packed(1..8) -> DirectN`.

The permanent oracle remains `crucible-world-reference::DirectBlockSection` and is not one of these production candidates.

## Commands

```text
cargo xtask qualify section --quick
cargo xtask qualify section --full
cargo xtask qualify section --quick --candidate adaptive
cargo xtask qualify section --full --candidate packed-local
```

`--quick` is the mandatory PR-CI tier. `--full` uses eight deterministic long-trace seeds at 250,000 mutations each, exceeding two million seeded target mutations before the common trace classes and synthetic-fact suite are counted.

Evidence is written to:

```text
target/crucible-qualification/section/quick.json
target/crucible-qualification/section/full.json
```

Evidence records include the exact Git commit, target versions, generated-data digests, trace schema, linked SEM IDs, operation counts, and a deterministic FNV-1a trace fingerprint. The FNV fingerprint is an identity/checkpoint aid, **not** a cryptographic provenance digest.

## Evidence scope

The first harness slice links the block-operation rules:

- `SEM-WORLD-SECTION-001`, `002`, `005`–`014`.

It does **not** claim evidence yet for biome rules `003`, `004`, `015`, `016` or wire/decode rules `017`, `018`.

Those require their own semantic fixtures and, for externally observable ambiguity, official-runtime qualification.

## Remaining M0.3C work

Before #18 can close:

1. add official/source-backed section fixtures for the random/count boundary cases;
2. add vanilla-fixture command support;
3. record a full-tier evidence artifact on a concrete commit;
4. classify any remaining source/runtime ambiguity;
5. extend evidence to wire/decode behavior once the relevant adapter exists.

Performance conclusions are explicitly out of scope here. Candidate selection remains #19.
