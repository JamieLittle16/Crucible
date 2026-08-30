# R2C Packed Four-Bit Specialization Qualification

Status: **four-bit cell-major specialization qualified for a separate production experiment**  
Target: **Minecraft: Java Edition 26.2 / DataVersion 4903**  
Parent: `R2C_PACKED_IMPORT_PERFORMANCE_QUALIFICATION.md`

## Scope

This qualification asks whether Helve should specialize the non-spanning packed-state decode arithmetic used when `bits_per_entry == 4` while preserving the existing cell-major semantic order and all checked palette/error behavior.

It does **not** change production importer code. It does not revive the previously rejected word-major traversal, remove palette bounds checks, use unchecked indexing, change the persisted layout, or alter the generic path used by five-bit and wider palettes.

## Why this candidate was tested

PR #226 localized the post-CRC packed importer residual away from NBT parsing and packed-long ingestion. On its qualification run:

- real semantic decode with the final reference copy removed: **10.095 us p50**;
- NBT/schema traversal reading all 256 packed longs: **0.335 us**;
- the same traversal fast-skipping the long-array payload: **0.157 us**;
- packed-long ingestion delta: **0.178 us**.

A later runner reproduced the same shape: semantic no-copy **9.644 us**, NBT read **0.367 us**, NBT skip **0.147 us**.

The residual was therefore localized primarily to 4096-cell state materialization rather than schema traversal.

## Mechanisms compared

### Runtime-generic cell-major baseline

The baseline intentionally receives `bits_per_entry` through `black_box(4)` so the compiler cannot simply constant-fold the benchmark into the candidate:

```text
values_per_word = 64 / bits_per_entry
mask = (1 << bits_per_entry) - 1
word = words[cell / values_per_word]
shift = (cell % values_per_word) * bits_per_entry
```

### Four-bit-specialized cell-major candidate

For the exact `bits_per_entry == 4` case:

```text
word = words[cell >> 4]
shift = (cell & 15) << 2
mask = 0x0f
```

Both mechanisms still perform:

- exactly 4096 cells in the same linear order;
- `usize` conversion of the packed palette index;
- checked palette lookup;
- one output push per successfully decoded cell;
- the same structured out-of-range error fields.

The candidate therefore changes arithmetic specialization only.

## Benchmark methodology

Qualification-only example: `r2c_packed_four_bit_probe`.

Smoke mode uses:

- **2048 measured rounds**;
- **256 warmup rounds**;
- alternating baseline/candidate measurement order every round;
- preallocated output vectors reused across rounds;
- an identical deterministic 4096-cell four-bit fixture;
- full decoded-vector equality before measurement;
- a stable semantic checksum on every round.

The probe remains `diagnostic_only: true` and `performance_admitted: false`.

## Exact failure-semantics witness

Performance cannot be obtained by weakening checked palette behavior.

The probe injects palette index **3** at exact cell **37** into a two-entry palette. Before timing, both implementations must return exactly:

```text
PaletteIndexOutOfRange {
    cell: 37,
    palette_index: 3,
    palette_entries: 2,
}
```

Both output vectors must also contain exactly 37 successfully decoded cells, proving that neither path detects the error late or silently continues.

## Result 1 — Intel

Workflow `33328938741`, first successful job `99303912248`, Intel Xeon Platinum 8573C:

| Mechanism | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| runtime-generic | **6.872 us** | 6.893 us | 8.321 us |
| four-bit specialized | **4.021 us** | 4.033 us | 4.530 us |

Emitted p50 ratio: **585 milli**.

The specialization reduced p50 by **2.851 us**, approximately **41.5%**.

The same run also reconfirmed the earlier word-major rejection: cell-major **4.025 us** versus word-major **4.579 us**.

## Result 2 — unchanged rerun on AMD

The exact same GitHub Actions job was rerun without changing code or methodology. Job `99304062884`, AMD EPYC 9V74:

| Mechanism | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| runtime-generic | **6.680 us** | 6.690 us | 7.281 us |
| four-bit specialized | **3.355 us** | 3.375 us | 3.466 us |

Emitted p50 ratio: **502 milli**.

The specialization reduced p50 by **3.325 us**, approximately **49.8%**.

The unchanged rerun therefore reproduces a large win on a second CPU architecture rather than merely repeating one noisy hosted measurement.

## Same-run surrounding evidence

On the AMD rerun, the production path remained unchanged and reported:

- packed import p50: **24.867 us**;
- packed component import p50: **24.376 us**;
- payload decode p50: **7.721 us**;
- state resolution p50: **0.120 us**;
- transparent/reference section build p50: **9.094 us**;
- component residual p50: **7.441 us**;
- semantic no-copy decode p50: **7.798 us**;
- NBT/schema read-all-longs p50: **0.260 us**;
- NBT/schema fast-skip p50: **0.122 us**.

These surrounding values are anchors, not additive accounting. The isolated specialization result is the mechanism-selection evidence; a separate production experiment must establish how much survives in the real importer.

## Semantic and engineering gates

Both successful qualification executions are green through the relevant R2C job gates, including:

- hermetic `--offline --locked` all-target build and tests;
- rustfmt;
- Clippy with `-D warnings`;
- 48 `helve-world-import` unit tests;
- resident/import regressions;
- uniform and packed whole-path smoke;
- existing packed-loop and residual diagnostics;
- independent synthetic Python/Rust importer differential;
- stored-state lookup regressions;
- vanilla-save extractor regressions.

The seven-section differential remains exactly:

`98cf921d050b0270c305138664d8fadd9fb85966f2e71a9eb7337cc9a4c24b12`

No production importer code is changed by this qualification PR.

## Decision

**P3 is accepted for a separate production experiment.**

The win is large, repeated without methodology changes, reproduced across Intel and AMD hosted runners, and achieved without weakening either successful decode semantics or exact out-of-range failure semantics.

This is still not production selection. The specialization must now be spliced into `stored_blocks.rs` on a separate stacked branch and requalified end to end.

## Production experiment shape

The intended production structure is deliberately narrow:

1. retain the existing `packed_bits(palette_entries)` and exact packed-word-count validation;
2. branch **once per section** on `bits_per_entry == 4`;
3. use the specialized cell-major arithmetic only in that branch;
4. preserve checked palette lookup and the exact `PaletteIndexOutOfRange` construction;
5. leave the existing generic loop unchanged for `bits_per_entry >= 5`.

There must be no per-cell dynamic dispatch, trait object, allocation, unsafe indexing, or new dependency.

## Production acceptance criteria

A production specialization is selected only if all of the following hold:

1. the existing four-bit YZX semantic regression remains exact;
2. the existing out-of-range palette-index regression remains exact;
3. a focused five-bit/non-spanning regression proves the generic fallback remains intact;
4. the seven-section independent differential digest remains unchanged;
5. the official 26.2 real-save corpus remains green;
6. hermetic build, rustfmt, Clippy and Rust tests remain green;
7. complete packed import improves on the production branch;
8. the residual/no-copy semantic path improves in the expected direction;
9. uniform sections do not enter the specialized loop or materially regress;
10. no per-cell dispatch, allocation, unsafe code or unchecked indexing is introduced;
11. the production result is documented before selection.

## Non-claims

This evidence does **not** establish that:

- every packed width should receive a specialized loop;
- the synthetic loop delta will survive 1:1 in whole import;
- the transparent/reference section builder is an optimization target;
- hosted CI timings are a target-hardware throughput guarantee;
- five-bit and wider persisted palettes may be changed without their own evidence.

## Requalification triggers

Re-run this qualification if any of these change:

- packed-state layout or non-spanning law;
- `packed_bits` policy;
- palette lookup/error semantics;
- compiler/toolchain or target architecture;
- benchmark fixture or measurement ordering;
- output materialization strategy;
- a wider packed-width specialization is proposed.
