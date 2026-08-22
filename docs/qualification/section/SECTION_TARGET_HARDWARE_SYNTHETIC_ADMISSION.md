# Section target-hardware synthetic child admission — M0.3D

Parent: #19  
Depends on: merged #44 (`e72196553d3013d16f3c61b7e51ab3668e367c4c`)  
Implementation PR: #45  
Target: Minecraft Java 26.2

This record preserves admission evidence for the candidate-isolated synthetic mutation/churn/promotion benchmark child.

It is a **measurement-child admission record**, not a representation decision. GitHub-hosted timing values remain diagnostic only.

## Implementation checkpoint

Implementation head:

`780915abb9e894ce0b594ffc8cabd0fc974de8d7`

At this checkpoint the following gates passed:

- ordinary strict CI, including supply-chain policy, pinned rustfmt, all-target check, Clippy `-D warnings`, Rust tests, quick semantic qualification, source-backed section fixtures, Python tooling tests/syntax and rustdoc;
- Section Full Qualification;
- Section Benchmark Smoke;
- Section Target Benchmark Smoke;
- Section Target Orchestrator Smoke;
- Section Target Synthetic Smoke.

The expensive Section Corpus Probe and Section Representative Member Probe were also required to finish successfully before final PR admission because this slice modifies the central `section_bench` binary used by those paths.

## Child contract

The child executes one candidate per process:

- `direct-reference`;
- `direct`;
- `adaptive`;
- `fast-local`;
- `packed-local`.

It reuses benchmark-v2's frozen case construction rather than redefining synthetic semantics.

Qualification contains 22 prepared cases. Every case measures:

1. `same-state-replace`;
2. `low-entropy-replace`;
3. `high-entropy-replace`;
4. `palette-churn`.

Promotion is measured separately at:

`2, 3, 5, 9, 17, 33, 65, 129, 257` live states.

Qualification therefore emits 88 replacement records plus nine promotion records per candidate. It retains 25 raw replacement samples at 32,768 operations/sample and 1,000 raw one-replacement samples per promotion boundary.

## Release-mode semantic preflight

No synthetic mutation timing begins from an unchecked mechanism state.

For every replacement workload the child performs one untimed release-mode preflight:

1. snapshots the complete 4096-cell semantic base image;
2. applies the deterministic mutation plan to an independent expected image;
3. applies the same plan through the candidate;
4. requires all 4096 cells to match exactly.

Promotion boundaries receive the same full-image check after one candidate promotion and before timing begins.

This guard is deliberately outside timed samples. Rechecking 4096 cells after every sample would contaminate the cache/thermal state of the tail measurement.

The full M0.3C differential suite remains the correctness authority; this preflight is an additional benchmark-boundary guard against timing incorrect release code.

## Hosted smoke evidence

Workflow: `Section Target Synthetic Smoke`  
Run: `32600437469`  
Head: `780915abb9e894ce0b594ffc8cabd0fc974de8d7`

Actions artifact:

`sha256:b3a679affa161d9e327c2308d4948d9eff54f2564c163625c053cf883589100e`

An independent post-workflow inspection of the downloaded artifact confirmed exactly five evidence files:

| Candidate | Evidence SHA-256 | Timing records |
|---|---|---:|
| `direct-reference` | `be25eac1af8f1656f8e4596439a9a276076bd96c0745c1b2350ea80aced2e343` | 49 |
| `direct` | `21fe51cb9e9abe92a7345737702fc148269e6385dc34166e33e67e977aa95002` | 49 |
| `adaptive` | `8848d3f89cf0dda4e7010f56116035119f6c2b6ca7826354c67b2f2b8283a4e1` | 49 |
| `fast-local` | `192e7d748221a40fe6f49b317c7cc08d299651888028916e10e72d3025e0799d` | 49 |
| `packed-local` | `b362d77f6c75f9efdab05d36be5385a76d5ada919c366eede870b4238b4801cf` | 49 |

Every file contained:

- schema `1`;
- harness version `section-target-synthetic-bench-v1`;
- mode `smoke`;
- scope `synthetic-mechanism-stress`;
- target `26.2 / protocol 776 / data 4903 / 32,366 states`;
- one candidate identity and correct production/reference classification;
- ten prepared synthetic cases;
- 40 replacement timing records;
- nine promotion timing records;
- three raw replacement samples at 2,048 operations/sample;
- eight raw promotion samples at one operation/sample;
- three raw control samples at 20,000 integer iterations/sample;
- exact commit/toolchain/release/CPU/NUMA provenance.

## Packed transition-chain evidence

The independent artifact inspection confirmed packed-local's promotion records exercise the complete expected width chain:

```text
promotion-to-2    uniform  -> packed-1
promotion-to-3    packed-1 -> packed-2
promotion-to-5    packed-2 -> packed-3
promotion-to-9    packed-3 -> packed-4
promotion-to-17   packed-4 -> packed-5
promotion-to-33   packed-5 -> packed-6
promotion-to-65   packed-6 -> packed-7
promotion-to-129  packed-7 -> packed-8
promotion-to-257  packed-8 -> direct-n
```

This means the smoke does not merely carry promotion labels in metadata; it executes every packed-width growth plus the final direct promotion.

The `promotion-to-3` regression permanently pins the first 1→2-bit widening, the transition class that exposed the earlier release-only side-effect-in-`debug_assert!` defect.

## Frozen workload identity

The production measurement loop and the regression suite use the same constant replacement-workload registry:

```text
same-state-replace
low-entropy-replace
high-entropy-replace
palette-churn
```

This avoids a test-only list that can drift from what the child actually executes.

The exact promotion target array is likewise inherited from benchmark v2:

`[2, 3, 5, 9, 17, 33, 65, 129, 257]`.

## What this admits

Once #45 is merged, all section measurement primitives required by the current M0.3D plan are structurally present:

1. semantic/differential correctness qualification;
2. source-backed real-world corpus extraction and admission;
3. representative-v1 four-seed population construction;
4. candidate-isolated real-population CPU/RSS child;
5. controlled repeated target-hardware population orchestrator;
6. candidate-isolated synthetic mutation/churn/promotion-tail child.

The system is therefore close to complete, but **measurement infrastructure is not the decision itself**.

Remaining gates:

1. route repeated synthetic children through the same controlled source→binary→CPU/order/noise orchestrator;
2. admit combined population + synthetic evidence only when their independent noise/protocol gates pass;
3. perform actual repeated qualification on target hardware rather than use GitHub timing;
4. assemble dimension-separated Pareto/materiality tables from correctness + population CPU/RSS/logical bytes + synthetic mutation/promotion tails;
5. commit the final selection/rejection record;
6. remove losing mechanisms from the production engine path while retaining their candidate/experiment/history documentation.

Until those gates are complete, no production representation winner is frozen.
