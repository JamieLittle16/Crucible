# Vanilla-Derived Section Corpus Specification

Status: **M0.3D corpus contract v1**  
Parent: #19  
Target: Minecraft Java 26.2 / protocol 776 / data version 4903

This document defines the normalized real-world section corpus used by the section representation laboratory.

The corpus is evidence. It is not a storage format for the server and it is not a copy of Mojang's palette/container representation.

## Principle

The real-corpus path is:

```text
PINNED VANILLA SAVE SNAPSHOT
        ↓
SOURCE INVENTORY HASH
        ↓
VERSION-PINNED EXTRACTOR
        ↓
NORMALIZED SEMANTIC STATE IDS
        ↓
FAIL-CLOSED PYTHON CORPUS VALIDATOR
        ↓
RECOMPUTED CORPUS MANIFEST
        ↓
INDEPENDENT RUST STREAMING IMPORTER
        ↓
FIVE-CANDIDATE EXACT IMAGE + SUMMARY RECONSTRUCTION
        ↓
PYTHON ↔ RUST EVIDENCE CROSS-CHECK
        ↓
SECTION BENCHMARK LAB
```

The extractor is not trusted to attest to its own correctness. The Python validator independently checks the normalized cells against the frozen 26.2 state universe and recomputes all derived statistics from the generated target fact table. The Rust importer independently reparses the canonical corpus boundary and requires exact reconstruction through every current section mechanism before the corpus can enter the benchmark lab.

## What is normalized

Each corpus record represents exactly one 16×16×16 block section.

A record contains:
- dimension resource location;
- chunk x coordinate;
- chunk z coordinate;
- section y coordinate;
- exactly 4096 vanilla semantic block-state IDs.

Cell order is the frozen section linearization:

```text
index = (y << 8) | (z << 4) | x
```

The corpus deliberately does **not** preserve:
- Mojang palette slot numbers;
- palette bit width;
- indirect container implementation;
- Java object identity;
- hash-table shape;
- packet encoding layout;
- cached or derived server objects.

Those are mechanisms, not semantic workload identity.

## Canonical text format

The file is UTF-8, LF-only, and must end in exactly a normal final LF. Blank lines are forbidden.

### Line 1 — format identity

```text
CRUCIBLE-SECTION-CORPUS|1
```

### Line 2 — frozen target binding

```text
TARGET|minecraft=26.2|protocol=776|data=4903|state_count=32366|generation_sha256=<STATE_DATA_GENERATION_SHA256>
```

Field names and order are part of schema 1.

For the current target, `generation_sha256` is:

```text
79e5803347d6fb6f7ffccea4cef783998a1c6469ed869d26fa48ab5f2328cd3b
```

A corpus with a different target or generated-state digest is not silently upgraded. It is rejected.

### Line 3 — extraction provenance

```text
SOURCE|kind=vanilla-save|inventory_sha256=<SHA256>|extractor=<ID>
```

`inventory_sha256` identifies the exact source-save snapshot presented to the extractor. It is not the normalized-corpus digest.

The extractor must compute the inventory digest from a canonical inventory of every source file it consumes. The intended inventory record is ordered by normalized relative path and contains the SHA-256 of each consumed file. The raw save and inventory may remain local; the digest survives in the corpus/manifest.

Schema 1 admits only `kind=vanilla-save`. Other evidence sources require an explicit schema or source-kind extension rather than being relabelled as a save.

### Line 4 onward — sections

```text
SECTION|<dimension>|<chunk_x>|<chunk_z>|<section_y>|<id0>,<id1>,...,<id4095>
```

Example shape:

```text
SECTION|minecraft:overworld|0|0|-4|0,0,0,...
```

The example is schematic; an actual record contains exactly 4096 IDs.

## Canonicalization rules

Both independent consumers require:
- lowercase canonical resource locations such as `minecraft:overworld`;
- canonical decimal coordinates (`0`, `12`, `-4`; never `00`, `+4`, or `-0`);
- canonical non-negative decimal state IDs;
- every state ID in `0..32366` for the current target;
- exactly 4096 IDs per section;
- strictly increasing section keys ordered by `(dimension, chunk_x, chunk_z, section_y)`;
- therefore no duplicate coordinates;
- LF-only line endings;
- no blank lines;
- a terminal newline.

Strict ordering is intentional. Equivalent source snapshots must normalize byte-identically if the extractor version and selection policy are unchanged.

## Recomputed Python manifest

`tools/section_corpus.py` never accepts derived statistics from the extractor. It recomputes a manifest from the normalized cells and frozen target evidence.

The manifest includes:
- manifest schema and corpus-format identity;
- exact SHA-256 of the normalized corpus bytes;
- Minecraft/protocol/data/state-count binding;
- state-data generation and input digests;
- source kind, source-inventory SHA-256, extractor ID;
- section count and total cell count;
- total number of distinct state IDs observed;
- exact section-cardinality histogram;
- section count by dimension;
- aggregate semantic cell counts for non-air, counted-fluid, random-block and random-fluid facts;
- counts of all-air, fluid-containing, random-block-present and random-fluid-present sections.

The four semantic facts are recomputed from the committed generated target table, not copied from the save extractor.

## Independent Rust import boundary

`section_bench --corpus-check` is a second consumer of the normalized format, not a wrapper around the Python parser.

Its trust boundaries are deliberately separated:
- `corpus/parser.rs` validates untrusted canonical bytes and target/provenance identity;
- `corpus/verify.rs` reconstructs semantic images and diagnostics;
- `corpus.rs` owns purpose/decision policy and evidence serialization;
- `corpus/tests.rs` attacks those boundaries adversarially.

The normal import uses one authoritative streaming pass over section bodies. A section is parsed once, checked against all five current mechanisms, and discarded. Whole-corpus cell arrays are never retained. Per-section cardinality uses a fixed target-state bitset; state tokens are parsed directly; representation transition bookkeeping uses typed identities rather than mutation-time string allocation.

For every section, Rust independently recomputes the exact semantic summary from generated 26.2 facts, reconstructs:
- direct reference;
- direct production;
- adaptive;
- fast-local;
- packed-local;

and then requires:
- all 4096 cell states to read back exactly;
- maintained non-air count to match;
- maintained counted-fluid count to match;
- random-block presence to match;
- random-fluid presence to match.

Only after those checks may representation/memory/transition diagnostics be emitted.

The importer also emits both generated state-data digests, source inventory identity, extractor/purpose identity and corpus statistics. It deliberately does not reimplement SHA-256 in the dependency-light Rust binary; canonical corpus byte identity is computed by the Python validator and bound to Rust evidence in the official workflow.

## Corpus purpose is a machine-enforced property

Schema validity does not imply permission to affect a production decision.

The current `vanilla-save-region-v1-stored-sections` extractor is classified as:

```text
purpose = parser-admission
decision_eligible = false
```

Unknown canonical extractor policies are `unclassified` and are also fail-closed. A future representative policy must be introduced explicitly before `--corpus-decision-check` can succeed.

This prevents the first official spawn-world corpus—whose stored sections are overwhelmingly all-air/homogeneous—from silently becoming production workload weighting merely because it came from a real server.

## Validation commands

Python structural/semantic validator:

```bash
python3 tools/section_corpus.py validate .crucible/vanilla/section-corpus.txt \
  --manifest .crucible/vanilla/section-corpus-manifest.json

python3 tools/section_corpus.py inspect .crucible/vanilla/section-corpus.txt
```

Independent Rust reconstruction:

```bash
cargo run --release --locked \
  -p crucible-section-qualification \
  --bin section_bench -- \
  --corpus-check .crucible/vanilla/section-corpus.txt \
  --output .crucible/vanilla/section-corpus-rust-import.json
```

Decision-policy gate:

```bash
cargo run --release --locked \
  -p crucible-section-qualification \
  --bin section_bench -- \
  --corpus-decision-check .crucible/vanilla/section-corpus.txt
```

Python↔Rust evidence agreement:

```bash
python3 tools/section_corpus_import_evidence.py \
  --manifest .crucible/vanilla/section-corpus-manifest.json \
  --rust-import .crucible/vanilla/section-corpus-rust-import.json
```

`Section Corpus Probe` performs the entire chain from a fresh pinned official 26.2 server world and archives both evidence outputs plus the expected decision-rejection record.

## What may be committed

The normalized real corpus can become large and may encode a specific source world. It is therefore **not automatically a repository source file**.

Default policy:
- raw vanilla save: local/artifact only;
- canonical source inventory: local/artifact unless deliberately selected for publication;
- full normalized corpus: local or qualification artifact by default;
- corpus manifest/digest: commit-worthy evidence;
- Rust import summary / cross-check evidence: commit-worthy when part of a qualification record;
- tiny synthetic/adversarial test material: commit-worthy;
- extractor and validators/importer: commit-worthy.

No Mojang source body or server artifact is introduced by this format.

## Corpus selection policy must be explicit

A future extractor must not silently choose "whatever sections happened to be loaded".

Every qualifying corpus needs a documented selection policy covering at least:
- source world provenance;
- dimensions included;
- region/chunk selection rule;
- treatment of absent/un-generated sections;
- whether empty sections are included;
- any world-border/radius bounds;
- whether chunks are pregenerated or naturally played;
- extraction tool/version;
- source inventory digest.

Changing the selection policy creates a new corpus identity even if the source save is unchanged.

## Qualification use

Synthetic cases remain essential because they deliberately hit exact representation boundaries such as 16/17 and 256/257 states.

A representative real corpus later supplies the empirical weighting and spatial distributions needed to answer whether an optimization that wins at a synthetic boundary matters in real vanilla gameplay. The currently admitted spawn corpus proves the extraction/import boundary only; it is explicitly not that weighting corpus.

A production representation decision therefore requires:

```text
controlled synthetic boundary curves
              +
representative provenance-bound vanilla corpus curves
              +
controlled target-hardware/RSS evidence
              ↓
      Pareto / complexity decision
```

No candidate wins merely because it performs best on one corpus, one seed, or one machine.

## Evidence retention

When #19 selects a winner and deletes dominated implementation code, the corpus manifest/digest, Rust import evidence and corresponding benchmark artifacts remain part of the decision record.

This follows the project rule:

> **Code can disappear; experimental knowledge cannot.**
