# Section Corpus Benchmark Import Contract

Status: **M0.3D implementation/qualification contract**  
Parent: #19  
Depends on: real-target corpus admission #37

This document freezes how normalized vanilla section corpora enter the Rust benchmark laboratory. The importer is qualification-critical tooling: a parsing, reconstruction, or weighting-policy bug can select the wrong production representation even when the section implementations themselves are correct.

## Governing distinction

A corpus has two independent statuses:

1. **structurally/semantically admitted** — its bytes match `CRUCIBLE-SECTION-CORPUS/1`, target identity is pinned, every section has exactly 4096 valid semantic state IDs, source provenance is valid, and every benchmark mechanism reconstructs the same semantic image/summary;
2. **decision-eligible** — its sampling/weighting policy is representative enough to influence a production representation decision.

Passing (1) never implies (2).

The first official 26.2 spawn corpus passes (1) but deliberately fails (2): it is `vanilla-save-region-v1-stored-sections`, generated to admit the real-save/parser/import boundary, and contains an overwhelmingly all-air/cardinality-1 distribution.

## Cold-boundary rule

Corpus parsing and validation are cold qualification operations. They never enter Crucible's live server path and must not alter production section layouts or add benchmark counters to production structs.

The importer is streaming-first:

```text
corpus file
  -> strict header validation / corpus-purpose gate
  -> one SECTION record
  -> one exact 4096-state semantic image
  -> independent target-summary recomputation
  -> five candidate constructions / exact verification
  -> aggregate representation-memory diagnostics
  -> discard raw record
  -> next SECTION record
```

The benchmark process must not load an entire 100+ MiB corpus merely for convenience. That would contaminate later RSS measurements and unnecessarily increase qualification memory.

## Frozen target checks

The Rust importer independently requires:

- magic/schema `CRUCIBLE-SECTION-CORPUS|1`;
- Minecraft `26.2`;
- protocol `776`;
- data version `4903`;
- state count `32366`;
- generated-state generation digest through generated constants rather than a copied literal;
- source kind `vanilla-save`;
- canonical lowercase 64-hex source inventory digest;
- canonical extractor token;
- strictly increasing section coordinates;
- valid resource-location dimensions;
- canonical decimal coordinates;
- exactly 4096 state IDs per section;
- every state ID within the frozen target universe;
- canonical LF/newline rules.

Rust import evidence additionally records the frozen state-data input digest. The normalized corpus-byte SHA-256 is independently computed by the Python validator and bound to the Rust result by the official workflow evidence chain rather than reimplementing hashing in the dependency-light Rust binary.

The Python `section_corpus.py` validator remains the canonical external validator. The Rust importer is an independent consumer and must reject malformed input on its own rather than assuming the Python step was run correctly.

## Corpus-purpose registry

Decision eligibility is explicit code, not inferred from words like "real" or "vanilla".

Initial registry:

| Extractor / policy | Purpose | Decision eligible? |
|---|---|---:|
| `vanilla-save-region-v1-stored-sections` | parser admission / exploratory real layout | **No** |
| unknown canonical policy | unclassified | **No** |

A future representative policy becomes decision-eligible only through a reviewed code/documentation change that names the policy and its evidence obligations.

`--corpus-decision-check` evaluates this policy before performing expensive whole-corpus reconstruction, so a known non-eligible corpus fails quickly and explicitly.

## Candidate image and summary reconstruction

For each corpus section:

1. independently recompute the exact expected `SectionSummary` from all 4096 semantic state IDs using frozen generated 26.2 facts;
2. construct each candidate from the section's first semantic state;
3. install cells whose target state differs from that fill state, in frozen section order;
4. track representation transitions/logical backing-allocation events outside production structs using typed representation identities;
5. after construction, independently read all 4096 cells and require exact equality with the corpus image;
6. require the candidate's maintained non-air count, counted-fluid count, random-block presence and random-fluid presence to equal the independent recomputation;
7. record final representation and deterministic owned bytes.

Skipping redundant fill-state replacements is an image-reconstruction optimization only. It does not weaken same-state replacement qualification: those mutation semantics are independently exercised by M0.3C traces and synthetic benchmark regressions.

A corpus import may never continue after an image or summary mismatch.

## Allocation-conscious implementation

The importer is cold tooling, but unnecessary work is still rejected because corpus admission can process tens of millions of cells and later RSS experiments must have a clean boundary.

Current rules:
- one authoritative streaming pass over section bodies;
- one reusable section-line `String` buffer;
- direct token iteration rather than retaining 4096 token strings;
- fixed target-state bitset for per-section cardinality instead of a tree/hash set;
- no whole-corpus semantic image retention;
- all five mechanisms consume the same parsed section before it is discarded;
- representation transition tracking uses `RepresentationCode` rather than allocating representation-name strings per mutation;
- strings are produced only for final evidence records.

## Current diagnostic output

#38 deliberately emits **structural/semantic diagnostics, not real-corpus timing**:

- sections/cells consumed;
- distinct-state count and cardinality histogram observed by Rust;
- dimension histogram;
- final representation histogram per candidate;
- total/max deterministic owned bytes per candidate;
- construction representation transitions;
- logical backing-allocation events;
- target/provenance/purpose/decision identity.

Construction/read timing over real corpora is a subsequent #19 measurement slice. GitHub-hosted timing remains non-qualifying regardless of corpus purpose.

## Future process/RSS qualification

Final RSS measurement must not include a retained raw corpus buffer as candidate memory. The intended final shape is isolated candidate processes fed section records sequentially, with process baseline and candidate-resident state clearly separated.

Do not introduce a custom allocator merely for this purpose.

## Heavy regression requirements

The importer permanently tests at least:

- exact valid-header admission;
- wrong Minecraft/protocol/data/state-count/generation digest rejection;
- CRLF and missing-final-newline rejection;
- blank-line rejection;
- malformed source hash/extractor rejection;
- invalid dimension resource locations;
- noncanonical coordinate forms (`+1`, `01`, `-0`);
- duplicate and out-of-order coordinates, including negative coordinates;
- 4095/4097-cell rejection;
- leading-zero, negative, nonnumeric and out-of-range state IDs;
- empty-corpus rejection;
- parser-admission policy is not decision-eligible;
- unknown policy is not decision-eligible;
- exact cell-order preservation;
- exact cardinality recomputation;
- full metadata aggregation across multiple sections;
- candidate image and exact summary equivalence for every benchmarked mechanism;
- explicit 17-state transition-boundary reconstruction;
- real generated 26.2 state classes covering air, solid, counted fluid, random block and random fluid;
- construction transition/allocation tracking remains external to production objects.

The independent Python↔Rust evidence checker also has negative tests for target/provenance drift, corpus-summary disagreement, candidate-set/production-flag mismatch, representation-count disagreement and impossible memory totals.

Any material importer or corpus-weighting defect discovered later receives a permanent regression and experiment-log entry.

## Admission gate for this slice

The importer slice is complete only when one exact final head satisfies all of:

1. strict Rust/tooling CI — format, all-target check, Clippy `-D warnings`, Rust tests, quick/source-backed qualification and rustdoc;
2. existing release synthetic benchmark smoke remains green;
3. existing full release semantic qualification remains green for direct/adaptive/fast-local/packed-local;
4. hosted `Section Corpus Probe` freshly generates a world with the pinned official 26.2 server and feeds its normalized corpus through the Rust importer;
5. the independent Python validator and Rust importer agree on target/provenance/section/cardinality/dimension/candidate evidence;
6. every real-corpus section reconstructs exact cells and exact SEM summaries through all five mechanisms;
7. `--corpus-decision-check` rejects the parser-admission corpus for the expected policy reason;
8. the evidence artifact retains Python manifest, Rust import summary and decision-rejection record.

No representation winner is selected by this slice. Representative workload weighting, real-corpus timing and controlled target-hardware/RSS qualification remain subsequent #19 work.
