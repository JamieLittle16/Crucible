# R2B Preparation Scratch Reservation Sweep

This qualification investigates `OPT-R2B-002`: whether the per-join `PacketWriter` used by replay-free Minecraft 26.2 R2B preparation should reserve scratch capacity before the first dynamic packet is encoded.

The semantic packet-body limit remains 4 KiB regardless of reservation. The accepted production reservation is **1024 bytes**; the evidence and selection law are recorded below.

## Question

The pre-qualification runtime created `PacketWriter::new(4096)`. Its `Vec` began with zero retained capacity and grew as dynamic bootstrap bodies were encoded. `PacketWriter::with_capacity(4096, N)` can move some or all of that allocator work to writer construction while preserving the same semantic bound and the same reused-scratch ownership model.

The relevant trade-off is not allocation count alone. A useful reservation must reduce preparation cost and/or tail jitter without reserving substantially more transient memory than the workload justifies.

## Candidate sweep

The qualification compares the zero-reserve path against these initial reservations:

- 64 bytes;
- 128 bytes;
- 256 bytes;
- 512 bytes;
- 1024 bytes;
- 2048 bytes;
- 4096 bytes.

Each candidate is compared independently with the zero-reserve baseline using balanced ABBA/BAAB blocks so linear runner drift is bracketed rather than mistaken for a candidate effect.

## Workloads

Two semantic workloads are required:

1. `fresh-clear`: the selected fresh route with no existing-player initialization, no optional server-data packet and clear weather.
2. `populated-raining`: 64 existing offline player-info entries, optional server data and the admitted raining branch. This intentionally exercises a much larger dynamic player-info body while remaining below the unchanged 4 KiB packet-body limit.

The largest prepared dynamic body observed by the semantic gate is 110 bytes for `fresh-clear` and 2051 bytes for `populated-raining`.

A reservation that wins only on the selected tiny fixture is not sufficient evidence for production.

## Metrics

For every workload/candidate pair the benchmark records:

- p50, p95, p99 and p99.9 sample service time;
- maximum and arithmetic mean;
- median absolute deviation (MAD) and relative MAD;
- paired candidate/baseline block-ratio distribution;
- epoch-ratio distribution and epoch win rate;
- exact semantic checksum;
- largest encoded body observed by the semantic gate;
- reservation bytes.

Hosted CI runs are diagnostic. The acceptance decision requires stable direction across independent pinned runs and no material tail/jitter regression. A small p50 win does not justify a candidate whose p99/p99.9 or variance is worse.

## Full-resolution evidence

The decisive gate used three independent pinned processes in GitHub Actions run `33094176969` (artifact `9655683157`). Each process used 256 warm-up blocks followed by 4096 measured ABBA/BAAB blocks, 32 complete preparations per timed sample and 256 blocks per epoch.

Every 1024-byte epoch beat its paired zero-reserve epoch in all three runs and both workloads. The paired p50 ratios were:

| Workload | Run 1 | Run 2 | Run 3 | Approximate improvement |
| --- | ---: | ---: | ---: | ---: |
| `fresh-clear` | 743489 ppm | 735335 ppm | 733227 ppm | 25.7–26.7% |
| `populated-raining` | 949696 ppm | 951749 ppm | 946967 ppm | 4.8–5.3% |

The 1024-byte candidate also improved p99 and p99.9 service time in every workload/run pair:

| Workload | Run | Baseline p99 | 1024 p99 | Baseline p99.9 | 1024 p99.9 |
| --- | ---: | ---: | ---: | ---: | ---: |
| fresh | 1 | 38177 ns | 30726 ns | 50295 ns | 39008 ns |
| fresh | 2 | 37887 ns | 30225 ns | 46701 ns | 40231 ns |
| fresh | 3 | 38428 ns | 30286 ns | 50016 ns | 36645 ns |
| populated | 1 | 104888 ns | 100452 ns | 127512 ns | 120561 ns |
| populated | 2 | 107742 ns | 106921 ns | 152951 ns | 142514 ns |
| populated | 3 | 112028 ns | 101573 ns | 169596 ns | 157718 ns |

The other candidates establish the trade-off rather than a single monotone optimum:

- 128 and 256 bytes were also tail-clean in the full run, but left more preparation cost on the populated path and gave up a large part of the fresh-path win.
- 512 bytes produced the strongest fresh median result (about 29% faster) and roughly 4.2–4.8% populated improvement, but one populated p99.9 sample was effectively flat/slightly worse (134252 ns versus 134182 ns). That difference is noise-sized rather than a demonstrated regression, but it removes the clean all-runs tail advantage.
- 2048 and 4096 bytes improve the populated median more (roughly 7.7–8.1% and 9.2–9.4% respectively), but retain 2–4 times the scratch memory and give up substantial fresh-path performance relative to the 512/1024-byte region.

## Selection

**1024 bytes is accepted for production.**

It is not described as the smallest safe reservation or the fastest reservation in every workload. It is selected as the balanced Pareto point under the R2B qualification law:

1. exact semantic bytes are unchanged;
2. all measured epochs win in both workloads across all three independent full runs;
3. p50 improves materially in both workloads;
4. p99 and p99.9 improve in every workload/run pair;
5. the reservation remains only one quarter of the unchanged 4 KiB semantic writer limit;
6. it avoids the 2–4 KiB retention tier while preserving most of the heavy-route benefit;
7. it adds no owner, queue, synchronization, pool or runtime registry.

This is deliberately a multi-objective choice. Crucible does not trade smoothness or per-connection memory discipline for a single best-case throughput number.

## Architecture gate

The accepted production change may only replace the writer constructor with the already-existing bounded `PacketWriter::with_capacity` API. It must not add:

- a pool;
- cross-connection scratch ownership;
- synchronization;
- a second queue or buffer owner;
- a larger semantic packet limit;
- fixture-derived correctness assumptions.

The initial capacity is a performance hint, not a semantic maximum. The existing 4096-byte packet-body bound remains authoritative and the same scratch writer remains connection-local and transactionally reset by preparation.

## Acceptance gate

The production change is accepted only when the branch passes the normal workspace format/check/Clippy/tests, the R0 server gate and the R2B performance-evidence smoke workflow after the runtime constructor is changed. The full sweep above qualifies the numeric choice; normal CI qualifies the actual integrated implementation.

## Ongoing CI policy

The permanent R2B performance workflow returns to the shorter smoke sweep after selection. The full sweep is a qualification event, not a tax on every later network PR. Re-running `--full` remains available whenever preparation semantics, allocator behavior or representative bootstrap workloads change enough to invalidate this decision.

## Command

```text
cargo bench --locked --package crucible-target-26-2 --bench r2b_prepare_scratch -- --full --output target/r2b-prepare-scratch.json
```
