# R2C Gzip CRC32 Qualification

Status: **256-entry byte-table selected for R2C production; semantic and hosted performance qualification complete**  
Target: **Minecraft: Java Edition 26.2 / DataVersion 4903**  
Parent: `R2C_PACKED_IMPORT_PERFORMANCE_QUALIFICATION.md`

## Scope

This qualification selects the CRC32 implementation used by Helve's bounded gzip world-import decoder. It does **not** weaken or remove gzip checksum verification. The semantic contract remains that malformed gzip CRC, optional header CRC, size, framing, stream-consumption and output-bound conditions fail closed.

The question is only which equivalent CRC32 implementation should calculate the mandatory checksum.

## Why this candidate was tested

The preceding gzip decomposition (`#223`) showed on the independent `differential-packed4-gzip` fixture that complete production gzip decode was approximately 15 us p50 and that the previous 16-entry nibble-table CRC32 accounted for roughly 10 us of that cost. Raw DEFLATE itself was only about 4.5 us.

That made checksum implementation the largest identified gzip cost and justified testing a larger lookup table before changing production.

## Mechanisms

### Previous mechanism

- 16-entry table;
- 64 bytes of static table data;
- two nibble lookups and two shifts per input byte;
- reversed CRC32 polynomial `0xedb8_8320`.

### Selected mechanism P2

- 256-entry table;
- 1024 bytes of static table data;
- one byte lookup and one shift per input byte;
- table generated entirely by `const fn` at compile time;
- same reversed CRC32 polynomial `0xedb8_8320`;
- no allocation;
- no runtime initialization;
- no unsafe code;
- no new dependency.

The selected mechanism therefore trades **960 additional static bytes** for lower checksum work.

## Isolated benchmark methodology

The qualification-only `r2c_crc32_table_probe` obtains the actual decompressed bytes from the independent packed4 gzip fixture by running the production `DeflateChunkPayloadDecoder` first.

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

## Isolated qualification result

Workflow run `33309850528`, job `99252815161`, AMD EPYC 7763:

| CRC mechanism | Static table | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: |
| 16-entry nibble | 64 B | **10.286 us** | 10.331 us | 10.474 us |
| 256-entry byte | 1024 B | **5.412 us** | 5.566 us | 5.753 us |

The emitted p50 ratio was `526` milli. The byte-table candidate therefore used approximately **52.6% of the previous CRC p50**, a **47.4% reduction** on the measured 2204-byte input.

Absolute hosted timings are not target-hardware admission. The magnitude of the same-run mechanism difference was large enough to justify a production experiment.

## Production experiment

PR `#225` splices the exact qualified byte-table recurrence into `compression.rs`. The production diff changes only CRC table generation and the CRC loop, plus one additional semantic regression for optional gzip FHCRC handling.

No gzip framing, DEFLATE, exact-consumption, output-bound, trailer CRC, ISIZE, decoder-buffer, ownership or dispatch policy changes.

### Same-run production result

Workflow run `33317192460`, job `99272688327`, Intel Xeon 6973P-C:

| Measurement | p50 |
| --- | ---: |
| old-equivalent diagnostic complete gzip decode | **11.967 us** |
| production byte-table complete gzip decode | **7.718 us** |
| nibble CRC probe | **8.038 us** |
| byte-table CRC probe | **4.282 us** |
| production packed payload-decode component | **7.513 us** |
| production packed import | **21.399 us** |
| production uniform/zlib import | **3.355 us** |

The complete same-run decoder improvement is **4.249 us p50**, approximately **35.5%**. The isolated CRC probe independently reports about a **46.7%** CRC reduction on this CPU, reproducing the direction and magnitude of the qualification result on a second architecture.

The packed component harness reports exactly **one decoder call per measured packed chunk**. Because CRC is the only production runtime mechanism changed by this PR, the complete same-run decoder reduction is a direct causal reduction inside each packed import.

The absolute packed whole-import p50 above is retained as hosted diagnostic evidence only. It is **not** compared numerically with `#224`'s whole-import number because the two PRs ran on different hosted CPUs. Helve does not treat cross-run hardware variance as a controlled A/B.

## Semantic and safety evidence on the production branch

Workflow `33317192460` is green through:

- hermetic `--offline --locked` all-target build and tests;
- rustfmt;
- Clippy with `-D warnings`;
- **48** `helve-world-import` unit tests;
- stable CRC fixture;
- valid gzip and zlib decode;
- corrupted gzip trailer CRC rejection;
- corrupted gzip ISIZE rejection;
- optional gzip FHCRC acceptance and one-bit-corruption rejection;
- trailing compressed-byte and output-bound failures;
- resident/import regressions;
- uniform and packed whole-path benchmark smoke;
- packed component decomposition;
- gzip component decomposition;
- independent synthetic importer differential;
- existing vanilla-save oracle regressions.

The seven-section independent importer differential remains exactly:

`98cf921d050b0270c305138664d8fadd9fb85966f2e71a9eb7337cc9a4c24b12`

The separately regenerated official 26.2 corpus workflow `33317192512`, job `99272688526`, completed successfully through every gate:

- corpus/extractor regressions;
- official runtime-state identity extraction and binding;
- frozen generator-input identity check;
- deterministic official spawn-world generation;
- stored-overworld section extraction;
- production raw-Anvil fact-emitter build;
- production importer comparison against the official-save oracle;
- independent normalized-corpus validation;
- reconstruction through every Rust candidate;
- parser-admission corpus rejection by the production-decision gate;
- independent Python/Rust evidence cross-check;
- real-target evidence identity verification;
- normalized corpus evidence upload.

## Selection decision

**P2 is selected as Helve's R2C production CRC32 mechanism.**

The evidence is stronger than the original isolated benchmark:

1. the byte-table recurrence wins materially in an alternating-order CRC A/B;
2. that result reproduces on a second CPU architecture;
3. the exact production decoder is materially faster than an old-equivalent diagnostic decoder on the same bytes and runner;
4. the packed importer invokes that decoder exactly once per measured chunk;
5. checksum and malformed-input semantics remain unchanged;
6. the regenerated official 26.2 real-save corpus remains exact through the production importer and independent oracle checks.

The added 960 bytes of static table footprint are justified by the measured complete-decoder reduction. No runtime allocation or initialization cost is introduced.

## Production acceptance criteria

The production experiment required all of the following:

1. `crc32_fixture_is_stable` remains green with the existing expected CRC;
2. valid gzip and zlib decode regressions remain green;
3. corrupted gzip CRC still produces `GzipCrcMismatch`;
4. corrupted gzip ISIZE still produces `GzipSizeMismatch`;
5. optional gzip header CRC uses identical CRC32 semantics and corruption fails closed;
6. trailing compressed bytes and output bounds remain fail-closed;
7. no allocation, unsafe code, dynamic dispatch or new dependency is introduced;
8. hermetic build, rustfmt, Clippy and Rust tests remain green;
9. the seven-section independent differential digest remains unchanged;
10. official 26.2 real-save corpus qualification remains green;
11. complete production gzip decode improves materially on the selected implementation;
12. packed import contains a measured same-run reduction at its single decoder call;
13. uniform/zlib import does not acquire any CRC-path work or architectural regression;
14. the final result and static-table tradeoff are recorded in qualification documentation.

**All fourteen criteria are satisfied.**

`performance_admitted` remains false because hosted CI is mechanism-selection evidence, not controlled target-hardware throughput admission.

## Requalification triggers

Re-run this qualification if any of these change:

- CRC polynomial or table-generation logic;
- compiler/toolchain or target architecture;
- gzip decoder framing/checksum policy;
- decompressed fixture identity/size;
- benchmark batching or ordering methodology;
- a larger CRC mechanism such as slicing-by-N is considered.

A larger table is not automatically better. Any future CRC mechanism must justify its footprint with additional complete-decoder and import value beyond this byte-table baseline.
