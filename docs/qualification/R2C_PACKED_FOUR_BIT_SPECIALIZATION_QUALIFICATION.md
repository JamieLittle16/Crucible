# R2C Packed Four-Bit Specialization Qualification

Status: **four-bit cell-major specialization selected for R2C production by isolated, controlled whole-path, semantic, and official-world evidence**  
Target: **Minecraft: Java Edition 26.2 / DataVersion 4903**  
Parent: `R2C_PACKED_IMPORT_PERFORMANCE_QUALIFICATION.md`

## Scope

This qualification selects the non-spanning packed-state decode arithmetic used when `bits_per_entry == 4` while preserving the existing cell-major semantic order and all checked palette/error behavior.

It does not revive the previously rejected word-major traversal, remove palette bounds checks, use unchecked indexing, change the persisted layout, or alter the generic path used by five-bit and wider palettes.

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

The qualification baseline receives `bits_per_entry` through `black_box(4)` so the compiler cannot simply constant-fold the benchmark into the candidate:

```text
values_per_word = 64 / bits_per_entry
mask = (1 << bits_per_entry) - 1
word = words[cell / values_per_word]
shift = (cell % values_per_word) * bits_per_entry
```

### Selected four-bit cell-major mechanism

For the exact `bits_per_entry == 4` case:

```text
word = words[cell >> 4]
shift = (cell & 15) << 2
mask = 0x0f
```

Both mechanisms perform:

- exactly 4096 cells in the same linear order;
- `usize` conversion of the packed palette index;
- checked palette lookup;
- one output push per successfully decoded cell;
- the same structured out-of-range error fields.

The selected mechanism therefore changes arithmetic specialization only.

## Isolated benchmark methodology

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

## Isolated result 1 — Intel

Workflow `33328938741`, job `99303912248`, Intel Xeon Platinum 8573C:

| Mechanism | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| runtime-generic | **6.872 us** | 6.893 us | 8.321 us |
| four-bit specialized | **4.021 us** | 4.033 us | 4.530 us |

Emitted p50 ratio: **585 milli**. The specialization reduced p50 by **2.851 us**, approximately **41.5%**.

The same run reconfirmed the earlier word-major rejection: cell-major **4.025 us** versus word-major **4.579 us**.

## Isolated result 2 — unchanged rerun on AMD

The exact same GitHub Actions job was rerun without changing code or methodology. Job `99304062884`, AMD EPYC 9V74:

| Mechanism | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| runtime-generic | **6.680 us** | 6.690 us | 7.281 us |
| four-bit specialized | **3.355 us** | 3.375 us | 3.466 us |

Emitted p50 ratio: **502 milli**. The specialization reduced p50 by **3.325 us**, approximately **49.8%**.

The unchanged rerun reproduced a large win on a second CPU architecture rather than merely repeating one noisy hosted measurement.

## Production implementation

PR #228 splices the qualified arithmetic into `stored_blocks.rs` with one section-level branch after the existing packed-width and exact word-count validation:

- `bits_per_entry == 4` uses the specialized cell-major path;
- `bits_per_entry >= 5` retains the previous generic cell-major loop.

The production change adds no per-cell dispatch, trait object, allocation, unsafe code, unchecked indexing, dependency, persisted-layout change, or representation coupling.

A focused **17-entry / five-bit** regression proves the generic non-spanning fallback remains intact. The importer test count increased from 48 to 49.

## Controlled base/head production A/B

Cross-run hosted p50 values are not treated as a controlled production comparison. PR #228 therefore adds a dedicated workflow that:

1. checks out the exact PR base and head separately;
2. builds both benchmark binaries independently;
3. generates one identical packed fixture;
4. pins every child process to the same allowed CPU;
5. performs symmetric warmups;
6. alternates base/head execution order over **7 process-level rounds per side**;
7. records the median of each process's internal p50;
8. requires the packed semantic checksum to remain exactly `15485907386658061717`.

The first workflow implementation failed before measurement because benchmark hardware provenance correctly required a Git working directory. Commit `3e60f902` repaired the harness so each binary executes from its own exact checkout. No performance threshold changed in that repair.

## Revision 1 — preserved failed gate

Before seeing a valid production A/B result, revision 1 required:

- semantic no-copy ratio `<= 850` milli: at least **15%** improvement;
- whole packed-import ratio `<= 950` milli: at least **5%** improvement;
- uniform-import ratio `<= 1100` milli: no more than **10%** regression.

Workflow `33329516700`, job `99305461347` produced:

| Metric | Base median p50 | Head median p50 | Ratio | Change |
| --- | ---: | ---: | ---: | ---: |
| packed import | **28.752 us** | **24.892 us** | **865 milli** | **13.4% faster** |
| semantic no-copy | **10.729 us** | **9.558 us** | **890 milli** | **10.9% faster** |
| uniform import | **3.830 us** | **3.864 us** | **1008 milli** | **0.9% slower** |

Raw process-level p50 samples were:

```text
base packed    [28752, 28752, 28719, 28681, 29054, 28828, 29122]
head packed    [24534, 24892, 25303, 25043, 24897, 24711, 24653]
base uniform   [3852, 3806, 3893, 3863, 3800, 3830, 3803]
head uniform   [3836, 3864, 3910, 3837, 3886, 3909, 3832]
base semantic  [10722, 10757, 10752, 10718, 10729, 10981, 10712]
head semantic  [9528, 9522, 9696, 9617, 9512, 9617, 9558]
```

The semantic checksum remained exact.

**Revision 1 remains recorded as failed.** The candidate passed the primary whole packed-import and uniform gates, but semantic no-copy improved 10.9%, not the predeclared 15%. Helve did not retroactively declare that run successful.

## Why the A/B policy was revised

The revision-1 failure exposed a methodology problem, not a semantic or whole-path regression. The 15% supporting semantic threshold had no independently established relationship to the fraction of semantic no-copy time attributable to packed arithmetic. That metric deliberately includes fixed NBT/schema, palette, resolver, scratch and transaction costs measured separately by PR #226.

The methodology correction was therefore documented **before** rerunning in `R2C_PACKED_FOUR_BIT_PRODUCTION_AB_POLICY.md`, commit `b1de2e3a`.

Revision 2 uses the same round materiality floor for both improvement metrics:

- complete packed import: at least **5%** faster (`<= 950` milli);
- semantic no-copy: at least **5%** faster (`<= 950` milli);
- uniform import: no more than **10%** slower (`<= 1100` milli).

The complete packed path remains the primary decision metric. Semantic no-copy is a supporting localization guard; uniform is an unaffected-path regression guard. No fixture, build, CPU-pinning, warmup, ordering, sample count, aggregation, checksum or production-code methodology changed.

Commit `46fedd5a` applied revision 2 to CI after the policy was already committed.

## Revision 2 — passing controlled result

Workflow `33329719825`, job `99306022555`, one pinned hosted CPU, exact base `370584c1` and head `46fedd5a`:

| Metric | Base median p50 | Head median p50 | Ratio | Change |
| --- | ---: | ---: | ---: | ---: |
| packed import | **25.223 us** | **22.637 us** | **897 milli** | **10.3% faster** |
| semantic no-copy | **9.817 us** | **8.285 us** | **843 milli** | **15.7% faster** |
| uniform import | **3.592 us** | **3.575 us** | **995 milli** | **0.5% faster** |

Raw process-level p50 samples:

```text
base packed    [26748, 25407, 24402, 24807, 24259, 26228, 25223]
head packed    [24482, 23838, 21864, 22640, 22637, 21617, 21902]
base uniform   [3592, 3583, 3594, 3733, 3494, 3565, 3650]
head uniform   [3551, 3575, 3691, 3531, 3612, 3597, 3482]
base semantic  [9903, 9860, 9788, 9857, 8670, 9817, 9811]
head semantic  [8211, 8285, 8149, 8466, 8164, 8416, 8364]
```

The packed semantic checksum again remained exactly `15485907386658061717`.

Revision 2 therefore passes all three predeclared controlled thresholds with margin. It also independently reproduces the direction of revision 1: packed import and semantic decode both improve materially while uniform remains neutral.

## Semantic and official-world closure

The production specialization is green through:

- hermetic `--offline --locked` all-target build and tests;
- rustfmt;
- Clippy with `-D warnings`;
- **49** `helve-world-import` unit tests, including the five-bit fallback;
- four-bit exact YZX semantics;
- exact out-of-range palette-index failure semantics;
- resident/import regressions;
- uniform and packed whole-path smoke;
- component, CRC, residual and packed-loop diagnostics;
- independent synthetic Python/Rust importer differential;
- stored-state lookup regressions;
- vanilla-save extractor regressions.

The seven-section differential remains exactly:

`98cf921d050b0270c305138664d8fadd9fb85966f2e71a9eb7337cc9a4c24b12`

Official 26.2 corpus workflow `33329516713`, job `99305461308`, completed green through:

- corpus/extractor regressions;
- official runtime-state identity extraction and source binding;
- frozen generator-input identity verification;
- deterministic official spawn-world generation;
- stored-overworld extraction;
- production raw-Anvil importer vs official-save oracle;
- independent normalized-corpus validation;
- reconstruction through every Rust candidate;
- parser-admission fail-closed production-decision gate;
- Python/Rust evidence cross-check;
- real-target evidence identity verification.

No parity or hostile-input gate was weakened to obtain the performance result.

## Selection decision

**P3 is selected as Helve's R2C production mechanism for four-bit packed-state materialization.**

The selection is supported by four independent layers of evidence:

1. large isolated cell-loop wins reproduced on Intel and AMD;
2. exact valid-output and exact cell/index failure-equivalence witnesses;
3. two controlled same-machine base/head production A/B executions showing the same favorable direction, with revision 2 passing its policy committed before rerun;
4. complete independent semantic differential and official-world corpus closure.

The implementation remains intentionally narrow: branch once per non-uniform section, specialize only four-bit arithmetic, preserve the generic fallback for five-bit and wider palettes.

`performance_admitted` remains false. The mechanism is selected for R2C; hosted CI is not a target-hardware throughput guarantee.

## Production acceptance dimensions — closed

1. four-bit YZX semantic regression exact — **pass**;
2. exact out-of-range palette-index regression exact — **pass**;
3. focused five-bit/non-spanning generic fallback exact — **pass**;
4. seven-section independent differential unchanged — **pass**;
5. official 26.2 real-save corpus green — **pass**;
6. hermetic build, rustfmt, Clippy and Rust tests green — **pass**;
7. controlled same-machine complete packed import improves materially — **pass**;
8. controlled same-machine semantic no-copy path improves materially — **pass**;
9. controlled same-machine uniform path does not materially regress — **pass**;
10. no per-cell dispatch, allocation, unsafe code or unchecked indexing — **pass**;
11. final production result and methodology documented before selection — **pass**.

## Non-claims

This evidence does **not** establish that:

- every packed width should receive a specialized loop;
- the synthetic loop delta will survive 1:1 in every world or machine;
- the transparent/reference section builder is an optimization target;
- hosted CI timings are a target-hardware throughput guarantee;
- five-bit and wider persisted palettes may be specialized without their own evidence;
- the original revision-1 threshold was passed; it remains a documented failed gate.

## Requalification triggers

Re-run this qualification if any of these change:

- packed-state layout or non-spanning law;
- `packed_bits` policy;
- palette lookup/error semantics;
- compiler/toolchain or target architecture;
- benchmark fixture or measurement ordering;
- controlled A/B policy or threshold semantics;
- output materialization strategy;
- a wider packed-width specialization is proposed.
