# Section population isolated-excursion guard — M0.3D

Parent: #19  
Status: **qualification hardening; no representation decision**

This record closes the population-side analogue of the synthetic MAD blind spot discovered while admitting the combined target-hardware controller.

## Defect class

The population orchestrator originally qualified repeated control, steady-workload and RSS measurements using relative median absolute deviation (MAD) alone.

MAD is deliberately robust. For five repeated values:

```text
x, x, x, x, 10x
```

median = `x` and MAD = `0`. A single catastrophic run can therefore remain invisible to a pure MAD stability gate.

The synthetic evidence layer already gained a separate isolated-excursion guard. Leaving population evidence on MAD alone would make the final Pareto decision depend on asymmetric repeatability rules.

## Frozen rule

Population qualification now records and gates both:
- relative MAD — central run-to-run drift;
- maximum relative deviation from the median — isolated run excursion.

The isolated-excursion threshold is exactly `3 ×` the corresponding MAD threshold.

| Population surface | relative MAD ceiling | maximum relative deviation ceiling |
|---|---:|---:|
| candidate-independent control p50 | 5% | 15% |
| production steady-workload p50 | 10% | 30% |
| production RSS loaded delta | 10% | 30% |

A production population run is noise-eligible only when both gates pass on every required production surface and RSS medians remain positive.

This maximum-deviation statistic is a **repeat-to-repeat summary stability metric**. It is not the benchmark timing record's raw `max_ns` tail value.

## Reference policy

`direct-reference` remains a correctness/reference benchmark rather than a production mechanism. Candidate-specific RSS instability in the reference implementation therefore does not veto otherwise qualified production evidence.

Candidate-independent control instability still vetoes the run because it diagnoses the shared measurement environment rather than one representation.

## Permanent regressions

The qualification tests now prove:
- `[100, 100, 100, 100, 1000]` has MAD `0` but maximum relative deviation `9,000,000` ppm;
- one extreme production steady-workload run fails despite MAD `0`;
- one extreme production RSS run fails despite MAD `0`;
- one extreme candidate-independent control result fails despite MAD `0`;
- modest isolated variance below the deliberately looser guard remains admissible;
- each isolated-excursion threshold remains exactly three times its MAD threshold;
- direct-reference-only RSS excursion remains non-blocking for production qualification.

## Decision impact

No benchmark result is changed retroactively and no candidate is selected here.

The next controlled qualification must be generated with this strengthened population classifier and the already-strengthened synthetic classifier. The final Pareto assembler may consume only combined evidence in which both evidence families pass their complete protocol/noise gates.
