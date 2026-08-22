# Section Corpus Benchmark Import Contract

Status: **M0.3D design contract — implementation slice active**  
Parent: #19  
Depends on: real-target corpus admission #37

This document freezes how normalized vanilla section corpora enter the Rust benchmark laboratory. The importer is qualification-critical tooling: a parsing, sampling, or weighting bug can select the wrong production representation even when the section implementations themselves are correct.

## Governing distinction

A corpus has two independent statuses:

1. **structurally/semantically admitted** — its bytes match `CRUCIBLE-SECTION-CORPUS/1`, the target/digests are pinned, every section has exactly 4096 valid semantic state IDs, and source provenance is valid;
2. **decision-eligible** — its sampling/weighting policy is representative enough to influence a production representation decision.

Passing (1) never implies (2).

The first official 26.2 spawn corpus passes (1) but deliberately fails (2): it is `vanilla-save-region-v1-stored-sections`, generated only to admit the parser, and contains an overwhelmingly all-air/cardinality-1 distribution.

## Cold-boundary rule

Corpus parsing and validation are cold qualification operations. They never enter Crucible's live server path and must not alter production section layouts or add benchmark counters to production structs.

The importer is streaming-first:

```text
corpus file
  -> strict header validation
  -> one SECTION record
  -> one exact 4096-state semantic image
  -> candidate construction / verification / diagnostic measurement
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
- generated-state digest through the generated constants rather than a copied literal;
- source kind `vanilla-save`;
- canonical lowercase 64-hex source inventory digest;
- canonical extractor token;
- strictly increasing section coordinates;
- valid resource-location dimensions;
- canonical decimal coordinates;
- exactly 4096 state IDs per section;
- every state ID within the frozen target universe;
- canonical LF/newline rules.

The Python `section_corpus.py` validator remains the canonical external validator. The Rust importer is an independent consumer and must reject malformed input on its own rather than assuming the Python step was run correctly.

## Corpus-purpose registry

Decision eligibility is explicit code, not inferred from words like "real" or "vanilla".

Initial registry:

| Extractor / policy | Purpose | Decision eligible? |
|---|---|---:|
| `vanilla-save-region-v1-stored-sections` | parser admission / exploratory real layout | **No** |
| unknown canonical policy | unclassified | **No** |

A future representative policy becomes decision-eligible only through a reviewed code/documentation change that names the policy and its evidence obligations.

## Candidate image reconstruction

For each corpus section and candidate:

1. construct the candidate from the first semantic state;
2. install the remaining 4095 cells in frozen section order;
3. track representation transitions/logical backing-allocation events outside production structs;
4. after construction, independently read all 4096 cells and require exact equality with the corpus image;
5. record final representation and deterministic owned bytes.

The semantic verification pass occurs outside timing windows.

A corpus benchmark may never continue after an image mismatch.

## Diagnostic real-corpus measurements

The first importer exposes **diagnostic-only** corpus measurement for non-decision-eligible corpora.

Useful aggregate records include:

- sections/cells consumed;
- cardinality histogram observed by the Rust importer;
- final representation histogram per candidate;
- aggregate/final owned bytes per candidate;
- construction representation transitions;
- logical backing-allocation events;
- construction latency distribution;
- sequential full-section read latency distribution.

GitHub-hosted timings remain non-qualifying regardless of corpus purpose.

## Future process/RSS qualification

Final RSS measurement must not include a retained raw corpus buffer as candidate memory. The intended final shape is isolated candidate processes fed section records sequentially, with process baseline and candidate-resident state clearly separated.

Do not introduce a custom allocator merely for this purpose.

## Heavy regression requirements

The importer must permanently test at least:

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
- parser-admission policy is not decision-eligible;
- unknown policy is not decision-eligible;
- exact cell-order preservation;
- exact cardinality recomputation;
- candidate reconstruction equivalence for every benchmarked mechanism;
- construction transition/allocation tracking remains external to production objects;
- diagnostic mode refuses to claim production qualification.

Any material importer or corpus-weighting defect discovered later receives a permanent regression and experiment-log entry.

## Admission gate for this slice

The importer slice is complete when:

1. strict Rust streaming import and tests are green under normal CI;
2. existing synthetic benchmark smoke remains green;
3. existing full semantic qualification remains unchanged/green;
4. the hosted official 26.2 corpus workflow feeds its freshly generated real corpus through the Rust importer/diagnostic path successfully;
5. Python and Rust agree on section count and core corpus identity/provenance;
6. no non-decision-eligible corpus can enter a production-decision benchmark mode.

No representation winner is selected by this slice.
