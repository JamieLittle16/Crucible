# World-section candidate registry

Parent: #19  
Target: Minecraft Java 26.2  
Status: **M0.3D performance selection active; no production winner frozen**

This registry is the durable record of materially distinct live block-section mechanisms considered during M0.3. Candidate code may later be deleted; these entries remain.

## Candidate status table

| ID | Mechanism | Status | Core hypothesis |
|---|---|---|---|
| `SEC-REF-DIRECT` | direct semantic reference | `reference-only` | a deliberately simple 4096-cell oracle gives structurally independent correctness evidence |
| `SEC-CAND-DIRECTN` | direct production storage | `active` | 8192-byte `u16` direct storage may be sufficiently cheap that palette machinery is not worth its CPU/tail cost |
| `SEC-CAND-ADAPTIVE-L4L8` | `Uniform -> Local4Stable -> Local8Stable -> DirectN` | `active` | a two-stage stable local hierarchy may materially reduce memory while preserving cheap common-case accesses |
| `SEC-CAND-FAST-L8` | `Uniform -> Local8Stable -> DirectN` | `active` | skipping Local4 may trade modest low-cardinality memory for lower mutation/transition complexity |
| `SEC-CAND-PACKED-1_8` | `Uniform -> Packed(1..8) -> DirectN` | `active` | minimal local bit width may create a useful memory-frontier profile despite packed-index CPU and widening costs |
| `SEC-OLD-APPEND-PALETTE` | append-only local palette accounting | `superseded` | stable palette slots without usage accounting looked sufficient to avoid compaction |
| `SEC-DEFER-THERMAL` | adaptive demotion/thermal switching | `deferred` | representation demotion may recover memory after entropy falls |
| `SEC-DEFER-LOCAL8-LOOKUP` | alternative Local8 state-to-slot index | `deferred` | linear palette lookup may become too expensive at large local cardinality |
| `SEC-DEFER-ARENA` | owner/domain-local section backing arena/slab | `deferred` | allocator/layout policy may reduce allocation overhead independently of representation choice |

## `SEC-REF-DIRECT` — direct semantic reference

**Status:** `reference-only`

**Shape**
- 4096 directly stored semantic state IDs;
- independently maintained summary witnesses;
- independent full-summary recomputation by scanning all cells.

**Purpose**
This is intentionally boring and permanently retained as a differential oracle. It is not a production mechanism and must be marked non-production in benchmark output.

**Evidence**
- permanent oracle used by M0.3C deterministic differential traces;
- all production candidates are compared against it rather than only against each other.

**Disposition**
Retain even after production candidate deletion. Its value is verification independence, not runtime performance.

---

## `SEC-CAND-DIRECTN` — direct production storage

**Status:** `active`

**Shape**
- homogeneous construction may still use the candidate's production wrapper/header semantics;
- direct `[BlockStateId; 4096]` backing when materialized;
- current target state identity is `u16`, giving 8192 bytes of direct cell payload.

**Hypothesis**
Minecraft 26.2 has only 32,366 states, so direct `u16` storage is much cheaper than a Java-object mental model suggests. Direct storage may therefore be the CPU winner and remain competitive enough in memory to be a production profile or baseline.

**Expected strength**
- trivial random/sequential lookup;
- no state-to-local-slot search;
- no width/palette transition after materialization;
- simple mutation path.

**Expected weakness**
- fixed 4096-cell payload for non-uniform direct form;
- potentially wasteful over the large population of low-cardinality real sections.

**Selection state**
Not selected. Must survive the real-corpus CPU/memory Pareto decision.

---

## `SEC-CAND-ADAPTIVE-L4L8` — stable Local4/Local8 hierarchy

**Status:** `active`

**Shape**
`Uniform -> Local4Stable -> Local8Stable -> DirectN`

- Local4: 4-bit stable indices, 16-slot capacity;
- Local8: byte stable indices, 256-slot capacity;
- palette slots maintain exact usage counts;
- dead slots are reused without renumbering live cells;
- promotion depends on simultaneous live cardinality, not historical churn;
- no demotion in M0.3.

**Hypothesis**
Most real Minecraft sections have low enough cardinality that two local stages materially reduce resident bytes, while stable slots make mutation cheap enough to justify the hierarchy.

**Important design rule**
A full palette does not force promotion when the overwritten state's usage count is one. The slot can be repurposed in place because simultaneous live cardinality does not increase.

**Expected strength**
- 2048-byte Local4 index backing over common low cardinalities;
- 4096-byte Local8 index backing up to 256 live states;
- no packed bit extraction in either local form;
- dead-slot reuse avoids representation-wide compaction.

**Expected weakness**
- Local4/Local8 state lookup currently scans palette entries linearly;
- extra transition stage and code compared with fast-local;
- Local4 -> Local8 promotion is an O(4096) rewrite.

**Selection state**
Correctness-qualified; performance selection pending #19.

---

## `SEC-CAND-FAST-L8` — fast Local8 hierarchy

**Status:** `active`

**Shape**
`Uniform -> Local8Stable -> DirectN`

**Hypothesis**
Skipping Local4 may improve mutation simplicity/tail behavior enough to justify spending more memory in low-cardinality sections.

**Expected strength**
- one local encoding;
- direct byte index reads;
- avoids Local4 -> Local8 transition;
- stable-slot reuse semantics shared with the adaptive candidate.

**Expected weakness**
- 4096 bytes of local indices even at cardinality 2;
- same current linear palette lookup pressure as Local8 in adaptive.

**Selection state**
Correctness-qualified; performance selection pending #19.

---

## `SEC-CAND-PACKED-1_8` — minimal packed local indices

**Status:** `active`

**Shape**
`Uniform -> Packed(1) -> ... -> Packed(8) -> DirectN`

- bit width grows monotonically from 1 to 8;
- stable palette slots and usage counts;
- widening rewrites all 4096 packed indices;
- a true 257th simultaneously live state promotes to direct.

**Hypothesis**
Very low-cardinality sections may be common enough that 512/1024/... byte packed index arrays create a meaningful memory profile that justifies bit extraction and widening cost.

**Expected strength**
- minimum index width for current allocated stable-slot capacity;
- explicit memory-frontier candidate.

**Expected weakness**
- bit arithmetic on reads/writes;
- widening spikes;
- more implementation complexity than byte/nibble storage.

**Known defect discovered by qualification**
The first release-mode full M0.3C run found a release-only first-widen bug. The pending mutation was executed inside `debug_assert_eq!(widened.try_replace(...))`; release compilation removed the entire side-effecting expression. Packed storage widened but did not install the requested state.

Reproducer:
- trace: `localized-churn`;
- seed: `0x10ca11ced00d`;
- operation: 6;
- symptom: replacement returned but readback did not equal requested state.

Fix:
- execute `try_replace` unconditionally;
- separately match/assert the invariant result;
- retain focused first-widen regression;
- rerun full release qualification.

Post-fix full evidence:
- all four candidates pass;
- 16 deterministic traces/candidate;
- 2,013,879 target trace operations/candidate;
- 4,112 synthetic operations/candidate;
- shared trace fingerprint `6a4814a1551a9e5a`.

**Selection state**
Correctness-qualified after fix; performance selection pending #19.

---

## `SEC-OLD-APPEND-PALETTE` — append-only palette cardinality

**Status:** `superseded`

**Original hypothesis**
Keep stable palette entries forever during ordinary mutation. Avoiding compaction appeared sufficient to make local IDs stable and membership conservative.

**Why it was superseded**
Historical allocated palette size is not the same as simultaneously live semantic cardinality. Under churn, dead entries accumulate and can trigger premature 16/17 or 256/257 promotion even though the number of currently live states remains within the local representation's capacity.

That makes representation behavior depend on mutation history rather than the actual live-cardinality hypothesis being tested.

**Replacement**
Stable slots now maintain exact usage counts. Zero-use slots are reusable. A last-use overwritten slot may be repurposed in place without rewriting other indices.

**Knowledge retained**
The important lesson is not merely “add counts”: stable local identity requires separating **slot allocation history** from **simultaneous semantic cardinality**.

No production code should retain the append-only mechanism for historical interest.

---

## `SEC-DEFER-THERMAL` — demotion / thermal switching

**Status:** `deferred`

One-way promotion is deliberately simpler. Demotion would add representation-wide transitions, hysteresis policy, additional state, and potential tail spikes.

Do not implement merely because memory can theoretically be recovered. It becomes an admitted candidate only if whole-trace corpus evidence shows that entropy commonly falls enough, for long enough, that a measured memory/CPU benefit justifies the machinery.

---

## `SEC-DEFER-LOCAL8-LOOKUP` — alternate Local8 lookup structure

**Status:** `deferred`

Current Local8 mutation finds states by linear palette scan. This is a consciously simple baseline, not a frozen mechanism.

Potential alternatives may include a side index or other small lookup structure, but each consumes memory and maintenance work. Introduce one only as a separate hypothesis if #19 shows Local8 lookup is a material bottleneck.

Do not silently complicate the baseline.

---

## `SEC-DEFER-ARENA` — backing allocator/layout experiment

**Status:** `deferred`

Boxes currently keep large variants out of the small uniform object. Arena/slab/owner-local backing may eventually improve allocation behavior or locality, but allocator policy is orthogonal to representation selection.

Do not confound M0.3D by changing both at once unless allocation evidence shows allocator overhead is material.

## Requalification / registry rules

- Any semantic failure moves a candidate out of the performance-admitted set until fixed and fully requalified.
- Any materially distinct representation policy receives a new candidate ID rather than silently mutating the historical hypothesis.
- A bug fix that preserves the mechanism's hypothesis remains under the same candidate ID, with the defect recorded here and in the experiment log.
- When #19 selects production policy, surviving candidates become `selected`; dominated candidates become `rejected` with decision-record links.
- Rejected candidate implementation code should be deleted from production builds. This registry remains.