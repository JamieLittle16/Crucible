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
Debug-only assertions must never be relied upon for mutation/validation side effects. Regression policy should check the concrete packed path and, where practical, statically discourage side-effectful assertion expressions in core code.

---

## 2026-08-22 — benchmark harness audit before trusting numbers

**Question**  
Is the initial M0.3D benchmark harness sufficiently faithful to #19 to produce production-selection evidence?

**Finding**  
The interrupted harness is a strong scaffold but is **not yet freeze-grade**.

Material gaps found during audit:
1. no explicit fluid-containing workload;
2. `survival-like` is not yet grounded as an actual air/stone-like semantic distribution;
3. positive `maybe_contains` may query a pool state that the spatial pattern never installed;
4. some timing records use requested pool cardinality instead of observed actual cardinality;
5. deterministic owned bytes exist, but representative lifetime allocation-event accounting is absent;
6. hardware metadata needs stronger affinity/current-frequency/environment/codegen capture;
7. real vanilla-derived section corpus ingestion is absent.

**Decision**  
Do not interpret initial synthetic timing as a production policy. Harden the harness, add regressions for the audit findings, keep GitHub timing diagnostic-only, then add the real vanilla-derived corpus before freezing thresholds.

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