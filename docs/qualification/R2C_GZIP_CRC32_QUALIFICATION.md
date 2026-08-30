# R2C Gzip CRC32 Qualification

Status: **256-entry byte-table candidate qualified for a production experiment; production selection not yet admitted**  
Target: **Minecraft: Java Edition 26.2 / DataVersion 4903**  
Parent: `R2C_PACKED_IMPORT_PERFORMANCE_QUALIFICATION.md`

## Scope

This qualification isolates the CRC32 implementation used by Helve's bounded gzip world-import decoder. It does **not** weaken or remove gzip checksum verification. The semantic contract remains that malformed gzip CRC, size, framing, stream-consumption and output-bound conditions fail closed.

The question here is only which equivalent CRC32 implementation should calculate the mandatory checksum.

## Why this candidate was tested

The preceding gzip decomposition (`#223`) showed on the independent `differential-packed4-gzip` fixture that complete production gzip decode was approximately 15 us p50 and that the current 16-entry nibble-table CRC32 accounted for roughly 10 us of that cost. Raw DEFLATE itself was only about 4.5 us.

That made checksum implementation the largest identified gzip cost and justified testing a larger lookup table before changing production.

## Mechanisms

### Current mechanism

- 16-entry table;
- 64 bytes of static table data;
- two nibble lookups and two shifts per input byte;
- reversed CRC32 polynomial `0xedb8_8320`.

### Candidate P2

- 256-entry table;
- 1024 bytes of static table data;
- one byte lookup and one shift per input byte;
- table generated entirely by `const fn` at compile time;
- same reversed CRC32 polynomial `0xedb8_8320`;
- no allocation;
- no runtime initialization;
- no unsafe code;
- no new dependency.

The candidate therefore trades **960 additional static bytes** for lower checksum work.

## Benchmark methodology

The qualification-only `r2c_crc32_table_probe` obtains the actual decompressed bytes from the independent packed4 gzip fixture by running the selected production `DeflateChunkPayloadDecoder` first.

Before timing, both CRC mechanisms must equal the CRC stored in the gzip trailer. A mismatch aborts the benchmark.

The smoke methodology is deliberately stronger than a single timing sample:

- decompressed input: **2204 bytes**;
- **1024 measured rounds**;
- **128 warmup rounds**;
- **64 CRC calls per timed batch**;
- current/candidate measurement order alternates every round;
- release build;
- ordinary hermetic build, rustfmt and Clippy gates remain mandatory.

The probe remains `diagnostic_only: true` and `performance_admitted: false`.

## Result

Workflow run `33309850528`, job `99252815161`, AMD EPYC 7763:

| CRC mechanism | Static table | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: |
| 16-entry nibble | 64 B | **10.286 us** | 10.331 us | 10.474 us |
| 256-entry byte | 1024 B | **5.412 us** | 5.566 us | 5.753 us |

The emitted p50 ratio was `526` milli. The byte-table candidate therefore used approximately **52.6% of the current CRC p50**, a **47.4% reduction** on the measured 2204-byte input.

Absolute hosted timings are not target-hardware admission. The magnitude of the same-run mechanism difference is nevertheless large enough to justify a production experiment.

## Same-run outer baselines

The same workflow run measured the still-unmodified production path as:

- complete production gzip decode p50: **15.055 us**;
- transparent diagnostic gzip decode p50: **15.029 us**;
- raw DEFLATE p50: **4.583 us**;
- current CRC p50: **10.165 us**;
- uniform stored-chunk import p50: **4.789 us**;
- packed whole-path import p50: **42.931 us**;
- packed component payload-decode p50: **15.549 us**.

The whole-path packed sample on this hosted run was noisier than earlier baselines, which is why production selection must use a fresh same-run before/after result rather than subtracting the microbenchmark improvement from this number.

## Semantic evidence on the qualification branch

The same R2C qualification run remained green through:

- hermetic `--offline --locked` all-target build and tests;
- rustfmt;
- Clippy with `-D warnings`;
- Rust gzip/import/resident regressions;
- uniform and packed whole-path benchmark smoke;
- packed component decomposition;
- gzip component decomposition;
- independent synthetic importer differential;
- existing vanilla-save oracle regressions.

The seven-section independent importer differential remained:

`98cf921d050b0270c305138664d8fadd9fb85966f2e71a9eb7337cc9a4c24b12`

No production decoder code is changed by this qualification PR.

## Decision

**P2 is accepted for a separate production experiment.**

This is not yet production performance admission. The 256-entry implementation must now be spliced into `compression.rs` on a new stacked branch and requalified end to end.

## Production acceptance criteria

The production experiment is accepted only if all of the following hold:

1. `crc32_fixture_is_stable` remains green with the existing expected CRC;
2. valid gzip and zlib decode regressions remain green;
3. corrupted gzip CRC still produces `GzipCrcMismatch`;
4. corrupted gzip ISIZE still produces `GzipSizeMismatch`;
5. optional gzip header CRC continues to use identical CRC32 semantics;
6. trailing compressed bytes and output bounds remain fail-closed;
7. no allocation, unsafe code, dynamic dispatch or new dependency is introduced;
8. hermetic build, rustfmt, Clippy and Rust tests remain green;
9. the seven-section independent differential digest remains unchanged;
10. official 26.2 real-save corpus qualification remains green;
11. complete production gzip decode improves materially on the selected implementation;
12. packed whole-import shows a same-run improvement consistent with the checksum reduction;
13. uniform/zlib import does not materially regress;
14. the final result and static-table tradeoff are recorded in qualification documentation.

## Expected direction, not an admission claim

The isolated candidate saves approximately **4.9 us** of CRC p50 on this 2204-byte fixture. Because CRC was around two thirds of complete gzip decode, a substantial complete-decoder improvement is expected.

That estimate is only a hypothesis for the production branch. Compiler layout, cache effects and surrounding code may change the realized gain, so Helve will use measured whole-path evidence rather than treating arithmetic subtraction as proof.

## Requalification triggers

Re-run this qualification if any of these change:

- CRC polynomial or table-generation logic;
- compiler/toolchain or target architecture;
- gzip decoder framing/checksum policy;
- decompressed fixture identity/size;
- benchmark batching or ordering methodology;
- a larger CRC mechanism such as slicing-by-N is considered.

A larger table is not automatically better. Any future CRC mechanism must justify its footprint with additional whole-import value beyond this byte-table baseline.
