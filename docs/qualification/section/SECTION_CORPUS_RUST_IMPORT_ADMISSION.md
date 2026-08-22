# Rust section-corpus import admission record

Status: **PASS — parser/import evidence only; not production-selection evidence**  
Parent: #19  
PR: #38  
Target: Minecraft Java 26.2

This record freezes the first real-target admission of the Rust `CRUCIBLE-SECTION-CORPUS/1` consumer used by `section-bench-v2`.

It does **not** select a production representation. The admitted corpus is intentionally classified as `parser-admission`, is overwhelmingly all-air/cardinality-1, and is rejected by the production-decision gate.

## Admitted code/evidence checkpoint

Qualification head:

`8d8c08ef895c9dae126f43685b966a60ed8b5e9f`

Hosted `Section Corpus Probe` run:

`32584743713`

Evidence artifact:

- name: `minecraft-26.2-section-corpus-probe`;
- artifact id: `9478781919`;
- artifact SHA-256: `e09e989b7ce659b9695096f7039fad41914f0afcbe6fe8fbec7039838bf6cf2a`.

Independent normal gates on the same head:

- strict CI: PASS;
- release `Section Benchmark Smoke`: PASS;
- full release semantic matrix: direct/adaptive/fast-local/packed-local PASS.

## Real-target chain

The admission run freshly performed the complete chain rather than consuming a committed corpus:

1. reflection-probed the pinned official Minecraft 26.2 server;
2. bound runtime identities to committed source qualification;
3. reproduced the frozen target-state input SHA-256;
4. generated a deterministic-seed official spawn world;
5. extracted real 26.2 Anvil/NBT section data;
6. independently validated `CRUCIBLE-SECTION-CORPUS/1` in Python;
7. streamed the same corpus through the Rust importer;
8. reconstructed every section through the direct reference plus all four production candidates;
9. required exact 4096-cell readback and exact generated-fact `SectionSummary` for every candidate;
10. required `--corpus-decision-check` to reject the corpus;
11. cross-checked Python and Rust evidence independently;
12. verified pinned target/server/source identities and uploaded the evidence bundle.

Every step PASSed.

## Target/provenance identity

- Minecraft: `26.2`;
- protocol: `776`;
- data version: `4903`;
- state count: `32,366`;
- generated state-data SHA-256: `79e5803347d6fb6f7ffccea4cef783998a1c6469ed869d26fa48ab5f2328cd3b`;
- qualified state-data input SHA-256: `2cbdfa96881be3f47dee8afdb5c4006e42cf2a718c440bf8ae76761a6acd31af`;
- official server SHA-256: `cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5`;
- extraction policy: `vanilla-save-region-v1-stored-sections`;
- corpus purpose: `parser-admission`;
- decision eligible: **false**.

This run's source inventory SHA-256:

`67a51a1d2ab631555fea64ffea9129c1e791eb3e4f9b9bb267b9c3ced6f292b3`

This run's normalized provenance-bound corpus SHA-256:

`5dcd0ea0fe74d94d1250f09057ab7c20a2e73f59ea62c84cab7e464d6c319be4`

## Corpus observations

- sections: `12,696`;
- cells: `52,002,816`;
- dimensions: `minecraft:overworld = 12,696`;
- distinct state IDs: `81`;
- all-air sections: `12,452`;
- cardinality-1 sections: `12,453`;
- non-uniform sections: `243`;
- counted-fluid cells: `8,959`;
- non-air cells: `759,382`;
- random-block cells: `4,626`;
- random-fluid cells: `4,019`;
- sections containing fluid: `46`;
- sections with random-block presence: `52`;
- sections with random-fluid presence: `15`.

Cardinality histogram:

| cardinality | sections |
|---:|---:|
| 1 | 12,453 |
| 2 | 34 |
| 3 | 33 |
| 4 | 26 |
| 5 | 18 |
| 6 | 19 |
| 7 | 12 |
| 8 | 14 |
| 9 | 19 |
| 10 | 17 |
| 11 | 11 |
| 12 | 6 |
| 13 | 5 |
| 14 | 11 |
| 15 | 2 |
| 16 | 4 |
| 17 | 2 |
| 18 | 4 |
| 19 | 2 |
| 20 | 2 |
| 21 | 2 |

## Rust reconstruction result

All five mechanisms reconstructed all `12,696` sections exactly.

| mechanism | production candidate? | final representation distribution | deterministic total owned bytes | maximum section bytes | construction transitions | logical backing allocations |
|---|---:|---|---:|---:|---:|---:|
| direct reference | no | direct-reference 12,696 | 104,208,768 | 8,208 | 0 | 12,696 |
| direct | yes | direct-n 12,696 | 104,208,768 | 8,208 | 0 | 12,696 |
| adaptive | yes | uniform 12,453; local4 231; local8 12 | 1,158,720 | 5,168 | 255 | 510 |
| fast-local | yes | uniform 12,453; local8 243 | 1,752,000 | 5,160 | 243 | 486 |
| packed-local | yes | uniform 12,453; packed-1 34; packed-2 59; packed-3 63; packed-4 75; packed-5 12 | 1,079,456 | 2,744 | 701 | 1,402 |

These are structural diagnostics from an intentionally biased parser-admission corpus. They are **not** target-hardware RSS measurements and **not** evidence for retaining or deleting any mechanism.

The important admission result is semantic:

> every parsed real 26.2 section produced the same exact 4096-state image and exact generated-fact section summary in the direct oracle and all four optimized candidates.

## Cross-run semantic reproducibility

The #37 parser-admission run recorded:

- source inventory SHA-256: `54279c23aa14032f1000ec82aad930b3b3f18af5c36210751b64db0aa321960e`;
- normalized corpus SHA-256: `8f1b623f4cd323ff8072c3c2722f96190dfe49b624ae65cf612f1e5ba785febf`.

The #38 run has different provenance hashes because the source inventory includes run-specific bytes such as `level.dat`. The normalized corpus header intentionally embeds that inventory digest, so the top-level corpus digest changes with the source instance.

A direct byte experiment on the #38 artifact replaced **only** its `SOURCE.inventory_sha256` header value with the #37 inventory SHA. The resulting SHA-256 was exactly:

`8f1b623f4cd323ff8072c3c2722f96190dfe49b624ae65cf612f1e5ba785febf`

which is the #37 corpus SHA exactly.

Therefore the complete semantic SECTION body is byte-for-byte identical across the two fresh official-server runs; the observed corpus SHA difference is entirely explained by the provenance header. This is strong evidence that the deterministic-seed spawn corpus semantics are reproducible while still retaining source-instance provenance.

## Permanent importer architecture

The admitted Rust boundary uses:

- one authoritative file stream;
- strict target/provenance header validation before section work;
- one reusable section-line buffer;
- direct token iteration rather than 4096 retained token strings;
- fixed target-state bitset cardinality tracking;
- one 4096-state semantic section image at a time;
- one independent generated-fact summary recomputation per section;
- all five mechanisms consuming the same parsed image before it is discarded;
- exact full-cell readback after reconstruction;
- exact maintained-summary comparison;
- typed representation transition identities for diagnostics;
- no benchmark instrumentation in production section structs;
- `#![forbid(unsafe_code)]` for the benchmark binary.

This deliberately avoids whole-corpus retention and the former six-pass design, and removes the possibility of different candidates observing different file contents between passes.

## Permanent regression classes

#38 retains tests for:

- Minecraft/protocol/data/state-count/generator drift;
- CRLF, blank-line and missing-final-LF rejection;
- malformed source kind/hash/extractor identity;
- unknown-policy fail-closed behavior;
- invalid resource locations;
- noncanonical coordinates (`+1`, `01`, `-0`);
- duplicate/out-of-order coordinates;
- 4095/4097-cell rejection;
- noncanonical/nonnumeric/negative/out-of-range state IDs;
- empty-corpus rejection;
- exact state ordering and cardinality;
- generated state-data evidence fields;
- all-five reconstruction at cardinalities `1, 2, 15, 16, 17, 255, 256, 257`;
- adaptive/packed transition-boundary behavior;
- real generated 26.2 air/solid/fluid/random-block/random-fluid fact classes;
- exact independently recomputed semantic summaries;
- independent Python/Rust evidence disagreement cases;
- mandatory decision rejection for parser-admission corpora.

## Decision

Admit the Rust normalized-corpus consumer as the M0.3D real-corpus **structural/semantic input boundary**.

Do not use this spawn corpus to freeze representation policy or thresholds.

The next production-decision work remains:

1. define and qualify a representative corpus/sampling policy, including omitted all-air sections and broader terrain/activity/dimension coverage;
2. run isolated target-hardware timing and process/RSS qualification;
3. perform noise-aware Pareto analysis;
4. write the final decision record;
5. remove dominated production mechanisms while preserving their permanent experiment records.
