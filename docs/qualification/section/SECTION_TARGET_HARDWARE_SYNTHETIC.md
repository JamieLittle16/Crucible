# Section target-hardware synthetic stress protocol — M0.3D

Parent: #19  
Depends on: benchmark v2 semantics and merged target-hardware population controller (#44)  
Status: **candidate-isolated transition/tail evidence; no representation decision**

This document defines the synthetic mechanism-stress evidence that complements the real representative-population CPU/RSS evidence.

The two evidence classes answer different questions:

- **representative population:** what does a candidate cost across a frozen real vanilla section population?
- **synthetic mechanism stress:** what does a candidate cost at exact mutation/cardinality/promotion boundaries that real terrain may visit too rarely to characterize tails reliably?

Synthetic evidence is therefore not a substitute for the representative population and must never be weighted as though its artificial cases were natural gameplay frequencies.

## Isolation rule

Target synthetic qualification runs exactly one candidate per process:

- `direct-reference` — permanent non-production oracle/baseline;
- `direct`;
- `adaptive`;
- `fast-local`;
- `packed-local`.

The child uses static generic dispatch after candidate selection. It does not put trait objects, benchmark counters or synchronization into production section structs.

CLI:

```text
section_bench \
  --synthetic-candidate <candidate> \
  --synthetic-target-qualification \
  --output <evidence.json>
```

Hosted structural smoke uses `--synthetic-target-smoke` and remains non-qualifying timing evidence.

## Frozen case law

The child reuses benchmark-v2's existing `CaseSpec`, `cases_for`, `prepare`, state pools, deterministic position streams and state streams. It does not maintain a second definition of the synthetic workload population.

Qualification therefore inherits the complete v2 cardinality surface:

`1, 2, 4, 8, 16, 17, 32, 64, 128, 256, 257, 1024, 4096`

and the qualified spatial classes:

- cardinality spread;
- homogeneous;
- layered;
- clustered;
- checker;
- noisy;
- explicit fluid-containing;
- survival-like;
- build-like.

Qualification contains 22 prepared cases. Smoke contains the existing 10-case bounded subset.

## Replacement workloads

Every prepared case measures exactly four mutation mechanisms:

1. `same-state-replace`;
2. `low-entropy-replace`;
3. `high-entropy-replace`;
4. `palette-churn`.

The deterministic plans are allocated outside timed regions. Every timed sample begins from a clone of the same prepared base so one sample's mutation history cannot leak into another.

For each workload, the child first executes one **untimed semantic preflight**:

1. read the complete 4096-cell base semantic image;
2. apply the exact deterministic replacement plan to an independent expected image;
3. execute the same plan through the candidate;
4. require all 4096 candidate cells to equal the expected image.

Only after this preflight passes may timing begin. This is specifically intended to stop release-only mutation defects from becoming attractive benchmark numbers.

The full semantic qualification suite remains the correctness authority; the preflight is an additional benchmark-boundary guard, not a replacement oracle programme.

## Promotion-tail workloads

Promotion is measured separately at:

`2, 3, 5, 9, 17, 33, 65, 129, 257` live states.

These points expose:

- first local allocation;
- packed 1→2, 2→3, 3→4, 4→5, 5→6, 6→7 and 7→8 width growth where applicable;
- Local4 capacity crossing at 16→17;
- Local8/packed-local direct crossing at 256→257.

Each promotion sample measures **one replacement only**. Base construction and cloning occur outside the timer.

Before timing each boundary, the child performs a full 4096-cell semantic preflight of the promoted image and records the before→after representation identity.

Qualification retains 1,000 raw single-replace samples per promotion boundary and reports p50/p95/p99/max. Smoke retains eight.

The promotion-to-3 regression explicitly exercises packed-local's first `packed-1 -> packed-2` widening, the same class of transition on which the earlier release-only side-effect defect was discovered.

## Sample scale

The target synthetic child intentionally reuses benchmark-v2 sampling scale:

| Setting | Smoke | Qualification |
|---|---:|---:|
| warmup samples | 1 | 5 |
| measured replacement samples | 3 | 25 |
| replacement operations/sample | 2,048 | 32,768 |
| promotion samples/boundary | 8 | 1,000 |
| control iterations/sample | 20,000 | 1,000,000 |
| prepared cases | 10 | 22 |

Therefore a qualification child emits:

- `22 × 4 = 88` replacement timing records;
- `9` promotion timing records;
- `1` candidate-independent integer control record;
- `97` mechanism timing records total.

## Candidate-independent control

Every child measures the same deterministic integer control loop used by the population benchmark family.

The control exists to detect process/environment drift. It is not a section-performance metric and cannot make one candidate preferable to another.

The later controller must keep synthetic control/noise qualification separate from mechanism significance thresholds.

## Evidence record

Schema/version:

```text
schema = 1
harness_version = section-target-synthetic-bench-v1
scope = synthetic-mechanism-stress
```

Every child records:

- exact candidate and production/non-production status;
- Minecraft/protocol/data/state identities and generated-data digests;
- exact repository commit;
- release/codegen policy;
- Rust toolchain/target;
- CPU model/kernel;
- CPU and memory-node affinity;
- governor/frequency/turbo/load provenance where exposed;
- explicit smoke/qualification settings;
- exact promotion target list;
- raw control samples;
- every replacement/promotion record with pattern/cardinality/representation identity;
- p50/p95/p99/max and raw nanosecond samples.

The report contains no cross-dimension score because synthetic mechanism stress is dimensionless by construction.

## Hosted smoke rule

A dedicated hosted workflow runs all five isolated candidates in release mode and validates:

- report schema/target/candidate identity;
- exact 10-case smoke surface;
- exactly 49 mechanism timing records (`10 × 4 + 9`);
- exact four replacement workload names;
- exact nine promotion targets;
- three raw replacement samples and eight raw promotion samples per record;
- promotion operations-per-sample = 1;
- candidate-independent control shape;
- CPU/NUMA/toolchain provenance.

Hosted timing values are diagnostic only.

## Decision firewall

This child cannot select a representation.

The final section decision requires, at minimum:

```text
full semantic correctness evidence
        +
admitted representative-v1 real population
        +
controlled repeated target-hardware population CPU/RSS evidence
        +
controlled repeated target-hardware synthetic mutation/promotion-tail evidence
        +
dimension-separated Pareto/materiality analysis
        +
committed candidate rejection/selection record
```

Until the later controller and Pareto layer admit the combined evidence, `decision_evidence_eligible` remains false.
