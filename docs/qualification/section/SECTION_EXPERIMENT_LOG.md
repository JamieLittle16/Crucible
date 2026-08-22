# World-section experiment and defect log

Parent: #19  
Target: Minecraft Java 26.2  
Purpose: chronological record of experiments or defects that materially changed the section architecture, qualification system, or production decision.

Routine compiler/formatting failures are intentionally omitted. This is a laboratory notebook, not a CI transcript.

---

## 2026-08-22 — target state universe frozen

**Question**  
Can live-section semantics use compact generated state identity/facts without consulting runtime registries or object graphs on the HOT mutation path?

**Result**
- official target: Minecraft 26.2;
- protocol: 776;
- data version: 4903;
- state count: 32,366;
- state IDs are dense and identity-mapped;
- AIR = state ID 0;
- narrowest safe current direct representation = `u16`;
- generated mutation fact table covers non-air, counted-fluid, random-block, random-fluid semantics.

**Important evidence identities**
- source archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`;
- official server SHA-256: `cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5`;
- source qualification digest: `5d312d6025fa6556feaf5fa26c80577dcb024e7e5be5cd1bda98d101367600c8`;
- qualified runtime digest: `8f12bb4a3636aa258929eeb963c7d9512256bfe37d01d9a9c5821e7947b1aed6`;
- generated state-data digest: `79e5803347d6fb6f7ffccea4cef783998a1c6469ed869d26fa48ab5f2328cd3b`.

**Decision**  
Generated state facts become the HOT semantic metadata source. Representation policy remains independent of state identity width.

---

## 2026-08-22 — historical palette allocation rejected as cardinality witness

**Question**  
Can stable local palette entries simply remain allocated forever while promotion uses palette length as its capacity test?

**Finding**  
No. Dead entries accumulate under churn, so historical palette length can reach 16/256 while fewer than 16/256 states are simultaneously live. This causes premature promotion and makes physical representation depend on mutation history rather than current live semantic cardinality.

**Decision**
- supersede append-only cardinality accounting;
- palette slots carry exact usage counts;
- zero-use slots are reusable;
- if the overwritten state has usage 1, its own slot may be repurposed in place;
- promotion occurs only when a genuinely new simultaneously-live state cannot fit.

**Permanent regression classes**
- pre-existing dead-slot reuse;
- full-palette last-use replacement at 16 and 256;
- genuine 16 -> 17 and 256 -> 257 transitions.

**Candidate record**  
`SEC-OLD-APPEND-PALETTE` -> `superseded`.

---

## 2026-08-22 — four distinct representation hypotheses admitted

**Question**  
Which representation mechanisms deserve correctness qualification before performance measurement?

**Admitted set**
- `SEC-CAND-DIRECTN`;
- `SEC-CAND-ADAPTIVE-L4L8`;
- `SEC-CAND-FAST-L8`;
- `SEC-CAND-PACKED-1_8`.

**Reasoning**  
The set intentionally spans different points on the expected CPU/memory frontier rather than minor implementation variants:
- direct CPU/simple baseline;
- two-stage stable local hierarchy;
- simpler byte-local hierarchy;
- minimal-bit memory frontier.

**Decision**  
Do not add more mechanisms until evidence identifies a distinct bottleneck/hypothesis.

---

## 2026-08-22 — deterministic semantic qualification established

**Question**  
Can representation equivalence be made an evidence graph rather than a collection of hand-picked unit tests?

**Result**  
A reusable qualification crate now drives deterministic versioned traces through each optimized candidate and the independent direct oracle.

Coverage includes:
- all-air stability;
- one-cell mutation/reversal;
- localized churn;
- random and high-entropy writes;
- dead-palette churn;
- exact 15/16/17 and 255/256/257 boundaries;
- all 16 synthetic fact-bit combinations;
- long deterministic seeded traces;
- previous-state and readback checks;
- incremental summary equivalence;
- complete 4096-cell checkpoints;
- independent full summary recomputation;
- conservative `maybe_contains` no-false-negative checks;
- clone independence.

**Decision**  
Quick semantic qualification becomes normal PR CI. Correctness is a prerequisite to #19 performance measurement.

---

## 2026-08-22 — source/runtime fixture chain established

**Question**  
How do we prevent the direct Crucible oracle and optimized candidates from merely agreeing with each other about the wrong semantics?

**Result**
- committed semantic fixtures are representation-independent;
- block fixture facts are independently checked against freshly reflection-probed official 26.2 runtime data;
- source-backed biome lattice/order/replacement semantics are separately tested;
- no Mojang source/runtime artifact is committed.

**Decision**  
Maintain the three-oracle chain:

```text
OFFICIAL SOURCE / RUNTIME
        -> SEM / fixtures
        -> DIRECT CRUCIBLE ORACLE
        -> OPTIMIZED CANDIDATE
```

Wire/decode rules remain deferred to their adapter rather than contaminating the live CPU-storage experiment.

---

## 2026-08-22 — full release qualification catches packed widening defect

**Question**  
Do candidates remain equivalent under optimized release compilation and multi-million-operation traces?

**Initial result**
- direct: PASS;
- adaptive: PASS;
- fast-local: PASS;
- packed-local: **FAIL**.

**Minimal observed failure**
- trace: `localized-churn`;
- seed: `0x10ca11ced00d`;
- operation: 6;
- failure: replacement did not install requested state.

**Root cause**
The pending write after packed-width expansion was performed inside a debug-only assertion:

```rust
// defective shape
debug_assert_eq!(widened.try_replace(cell, state), PackedReplace::Done);
```

In debug builds, `try_replace` executed. In release builds, the assertion expression was compiled out, removing the side effect entirely.

**Fix**
- execute `try_replace` unconditionally;
- match the returned invariant separately;
- make an impossible second widening/direct request an explicit invariant violation;
- add a focused first-width-growth regression.

**Post-fix result**
All four candidates PASS the complete release full suite.

Per candidate:
- 16 deterministic traces;
- 2,013,879 target trace operations;
- 4,112 synthetic operations;
- eight long seeds contribute 2,000,000 mutations;
- common trace fingerprint: `6a4814a1551a9e5a`.

**Decision**  
Full section qualification runs in release mode as an independent matrix. Performance work may proceed only from the post-fix qualified set.

**General lesson**  
Debug-only assertions must never be relied upon for mutation/validation side effects. Regression policy checks the concrete packed path in both the section implementation and the release benchmark/qualification build shape.

---

## 2026-08-22 — benchmark harness audit before trusting numbers

**Question**  
Is the initial M0.3D benchmark harness sufficiently faithful to #19 to produce production-selection evidence?

**Finding**  
The interrupted harness was a strong scaffold but was **not freeze-grade**.

Material gaps found during audit:
1. no explicit fluid-containing workload;
2. `survival-like` was not grounded as an actual air/solid semantic distribution;
3. positive `maybe_contains` could query a pool state that the spatial pattern never installed;
4. some timing records used requested pool cardinality instead of observed actual cardinality;
5. deterministic owned bytes existed, but representative lifetime allocation-event accounting was absent;
6. hardware metadata needed stronger affinity/current-frequency/environment/codegen capture;
7. real vanilla-derived section corpus ingestion was absent.

**Decision**  
Do not interpret initial synthetic timing as a production policy. Harden the harness, add regressions for the audit findings, keep GitHub timing diagnostic-only, then add the real vanilla-derived corpus before freezing thresholds.

---

## 2026-08-22 — section benchmark harness v2 admitted

**Question**  
Can the synthetic benchmark laboratory itself be made strict enough that workload-construction mistakes are regression-tested before any target-hardware numbers are trusted?

**Commit / CI identity**  
Branch checkpoint: `c2bfc5196659aa5f73d532592b39afd80d787472` (PR #34 integration state).

Diagnostic smoke artifact:
- artifact digest: `sha256:3f2c1cbd9409c84f24352ca55605e2298f1bdc9e2b3e487d59ad5a0d4f15e5cb`;
- schema: `2`;
- harness: `section-bench-v2`.

**Changes admitted**
- benchmark code split into model/workload/measurement/hardware/report modules;
- explicit target-fact-backed fluid-containing workload;
- survival-like workload defined as AIR plus qualified plain-solid states rather than arbitrary IDs;
- positive `maybe_contains` needle selected only from states observed in the final image;
- negative needle selected from the target universe outside the final image;
- `actual_cardinality` computed from final construction and independently regression-checked by full semantic rescan;
- deterministic construction/lifetime representation-transition and logical backing-allocation records;
- peak/final owned-byte lifetime records;
- integer picosecond-per-operation normalization instead of floating-point normalized timing;
- CPU affinity, governor/current/min/max frequency where exposed, load, turbo state and Rust flag environment captured in artifacts;
- release smoke artifact validator checks candidates, workload classes, cardinality invariants, lifetime records and memory relationships.

**Permanent benchmark regressions**
- actual cardinality equals independently observed final-image cardinality;
- positive membership needle is truly present;
- fluid workload contains a qualified counted-fluid state;
- survival workload contains AIR and only qualified non-fluid/non-random solid states otherwise;
- packed logical width-transition allocation accounting;
- packed-to-direct logical allocation accounting;
- packed first-width growth installs the requested state in the benchmark build shape;
- integer timing normalization;
- report JSON escaping;
- basic immutable hardware identity capture.

**Gate result**
- `Section Benchmark Smoke`: PASS, including release execution and emitted-artifact validation;
- normal strict CI: PASS through format, all-target check, Clippy `-D warnings`, Rust regressions, quick/source-backed semantic qualification, Atlas/Python and rustdoc;
- M0.3C full release matrix: direct/adaptive/fast-local/packed-local all PASS unchanged.

**Interpretation**  
Audit gaps 1–6 are now represented in code and regressions. GitHub timing remains explicitly non-qualifying.

**Remaining production-decision gates**
1. a provenance-bound vanilla-derived real section corpus;
2. controlled target-hardware/process-RSS qualification and Pareto/noise analysis.

**Decision**  
Admit benchmark harness v2 as the synthetic/diagnostic M0.3D laboratory. Do not freeze any representation or adaptive threshold until the two remaining evidence layers are complete.

---

## 2026-08-22 — official 26.2 real-save corpus admission

**Question**  
Can the normalized corpus pipeline consume a world generated by the exact pinned official 26.2 server, through a freshly source+runtime-bound state identity map, without save-format assumptions leaking in from historical Minecraft versions?

**First real-target result**  
FAIL before extraction. The world generator completed successfully, but the postcondition and extractor assumed the historical root dimension layout (`region/`, `DIM-1/region/`, `DIM1/region/`). The pinned 26.2 target uses the namespaced dimension hierarchy introduced in 26.1.

**Fix / regression**
- require `dimensions/minecraft/overworld/region`, `dimensions/minecraft/the_nether/region`, and `dimensions/minecraft/the_end/region` for the standard target dimensions;
- update synthetic fixtures to use the target layout;
- add a permanent regression that legacy root `region/` is not a standard 26.2 dimension;
- reject overlapping Anvil sector allocations.

**Post-fix evidence**  
The complete hosted `Section Corpus Probe` PASSed:
- fresh official runtime states: 32,366;
- fresh runtime + committed source qualification reproduced frozen input SHA-256 `2cbdfa96881be3f47dee8afdb5c4006e42cf2a718c440bf8ae76761a6acd31af`;
- deterministic official spawn world generated and stopped cleanly;
- real 26.2 Anvil/NBT extraction PASSed;
- independent corpus validator PASSed.

Real parser-admission corpus:
- stored sections: 12,696;
- cells: 52,002,816;
- distinct states: 81;
- all-air sections: 12,452;
- cardinality-1 sections: 12,453;
- sections with cardinality > 1: 243;
- source inventory SHA-256: `54279c23aa14032f1000ec82aad930b3b3f18af5c36210751b64db0aa321960e`;
- normalized corpus SHA-256: `8f1b623f4cd323ff8072c3c2722f96190dfe49b624ae65cf612f1e5ba785febf`;
- Actions artifact ZIP SHA-256: `e4d13661b538df496ac4758182112ef1c68b7f4c4cd19d20001231e01cee2361`.

Full evidence and histogram: [`SECTION_REAL_CORPUS_ADMISSION.md`](SECTION_REAL_CORPUS_ADMISSION.md).

**Interpretation**  
The extractor is now admitted against a real pinned 26.2 save. The spawn corpus is deliberately classified as parser-admission/non-qualifying workload evidence: its overwhelming homogeneous/all-air distribution would bias a production representation decision if treated as representative gameplay.

**Decision**  
Admit the extractor and normalized corpus boundary. Next, wire `CRUCIBLE-SECTION-CORPUS/1` into the Rust benchmark lab with corpus-purpose enforcement. Do not freeze representation thresholds until a broader explicit sampling/weighting policy and controlled hardware/RSS evidence exist.

---

## 2026-08-22 — independent Rust corpus importer and purpose firewall admitted

**Question**  
Can the normalized vanilla corpus be consumed by the Rust benchmark laboratory without trusting the Python validator as the only implementation, and can structurally valid but non-representative corpora be prevented from entering production decisions?

**Result**
- merged PR #38: `5a424ca913142eacf889031ebb569e9305b60a0d`;
- Rust corpus parsing independently rechecks target/version/generation identity, canonical line endings/coordinates/resource locations, strict ordering, exact 4096-cell shape and state bounds;
- one section is streamed at a time rather than retaining the raw corpus in benchmark memory;
- every imported section is reconstructed through direct-reference, direct, adaptive, fast-local and packed-local;
- every reconstruction is checked cell-for-cell and against independently recomputed semantic summary evidence;
- corpus purpose is an explicit policy value rather than inferred from “real vanilla” provenance alone;
- `parser-admission` and unknown policies are decision-ineligible by default;
- the official 26.2 parser corpus is independently cross-checked between Python and Rust;
- decision mode is required to reject that parser-admission corpus.

**Decision**  
Admit the normalized corpus as an independent Rust benchmark input boundary, but keep structural validity separate from workload decision eligibility.

---

## 2026-08-22 — representative-v1 sampling law frozen

**Question**  
What real vanilla workload population is broad and explicit enough to inform representation selection without allowing content-dependent cherry-picking or implicit cross-dimension weighting?

**Result**
- merged PR #41: `c192a498d3a6a8cd5a8e0b37716f3db0bb80397d`;
- policy: `vanilla-section-representative-v1`;
- plan SHA-256: `fecb9c9bc77aa9689ceaf6d88fa9af96019a48d9533269f3bd15824f7dfc7191`;
- four content-independent frozen seeds;
- 64 selected chunk columns per standard dimension per seed;
- equal seed weighting;
- natural vertical-section weighting within each dimension;
- dimensions remain separate decision strata;
- cross-dimension scoring is forbidden without a later explicit workload model;
- seed-0 real admission demonstrated 3,584 sections / 14,680,064 cells / 432 distinct states and all-five candidate reconstruction.

**Interpretation**  
“Real vanilla data” is not sufficient by itself: the selection law and weighting policy are part of benchmark semantics and must be frozen before observing candidate performance.

**Decision**  
Use representative-v1 as the first production-decision population law. Changes to seeds, coordinates, dimensions or weighting require a new policy identity rather than silent benchmark tuning.

---

## 2026-08-22 — complete four-seed population admitted for benchmark handoff

**Question**  
Is the complete representative-v1 population sufficiently coherent and independently validated to become an input to target-hardware performance qualification?

**Result**
- merged PR #42: `5029a73a195dc067a2e0360f770f0e3605a60df9`;
- every frozen member is generated from the pinned official 26.2 server and remains individually decision-ineligible;
- the structural set validator requires exact four-member seed/corpus identity, common target/server/plan/lattices, exact selected-coordinate schedule and Python/Rust reconstruction agreement;
- a second independent population-admission firewall re-opens all member evidence and rechecks exact seed-specific generation properties, semantic fact/class keys and subset relations, per-dimension/global recomposition, structural-set identity and post-digest mutation resistance;
- only the complete admitted population emits `benchmark_handoff_eligible = true`;
- artifact schema distinguishes complete qualification from partial diagnostic output;
- `decision_scope = dimension-separated-only` and `cross_dimension_score_allowed = false` are preserved through the handoff.

**Decision**  
Admit the four-seed population as production-decision **workload evidence**. It still contains no qualifying timing/RSS evidence and therefore does not select a representation.

---

## 2026-08-22 — target-hardware foundation audited before admission

**Question**  
Can real-population CPU/tail/RSS measurement be isolated tightly enough that later target-machine numbers describe section mechanisms rather than parser memory, demand-paging artifacts or biased query construction?

**Checkpoint**  
PR #43 hardened code checkpoint before documentation: `2c1deae93bbc7e71240beb312aa07a037df2ed3c`.

**Foundation**
- content-addressed per-dimension little-endian-u16 benchmark packs derived only from the admitted four-seed artifact;
- source corpus rehash during conversion closes validation-to-pack TOCTOU;
- exactly one candidate and one dimension per population benchmark process;
- every loaded section is checked cell-for-cell and by independent semantic summary before timing;
- steady-state random/sequential/4×4×4/membership workloads plus candidate-independent control work;
- raw construction samples, representation transitions, logical allocation events, logical owned bytes and process RSS/HWM recorded separately;
- complete toolchain/commit/CPU/affinity/frequency/governor/turbo/load provenance.

**Methodology defects found and fixed during audit**
1. merely allocating common scratch before RSS baseline did not guarantee physical residency; all retained common scratch is now explicitly dirty-prefaulted and restored before baseline;
2. saturating RSS subtraction hid negative/noisy observations; loaded-minus-baseline RSS is now an exact signed integer;
3. positive `maybe_contains` used a privileged fixed cell; each positive needle now comes from the deterministic planned `(section, cell)` and is preflighted for semantic truth before timing.

**Permanent regression/evidence additions**
- prefault restores canonical scratch contents;
- signed positive/negative/zero RSS deltas are preserved;
- positive membership follows the planned cell rather than cell zero;
- hosted target smoke enforces exact RSS arithmetic and the explicit prefault protocol;
- hardened hosted smoke artifact SHA-256: `66b655f7dd8b3c070d6038607845801d7676bd61fb3f90215bf4a656b03b9605`.

**Interpretation**  
GitHub-hosted timing and RSS remain diagnostic only. Construction samples describe the current benchmark materialization path and are not relabelled as production decode latency.

**Admission state at this log update**  
Strict CI, synthetic benchmark smoke, target benchmark smoke, full semantic qualification and the official parser-corpus probe have passed on the hardened implementation. The exact final PR head is still waiting for its official representative-member probe before squash merge.

Full foundation record: [`SECTION_TARGET_HARDWARE_FOUNDATION_ADMISSION.md`](SECTION_TARGET_HARDWARE_FOUNDATION_ADMISSION.md).

**Decision / follow-up**  
Do not rank candidates yet. Finish the real representative-member gate, then admit the foundation and build the target-machine orchestrator with exact pack-hash binding, CPU affinity, deterministic order rotation, repeated-process noise diagnostics and candidate-isolated synthetic transition evidence.

---

## Entry template

```markdown
## YYYY-MM-DD — short experiment name

**Question**

**Candidate/configuration**

**Commit / evidence identity**

**Workload / fixture**

**Result**

**Interpretation**

**Decision / follow-up**
```

Every future M0.3D result that changes candidate status, thresholds, representation policy, benchmark methodology, or our understanding of a mechanism should receive an entry.