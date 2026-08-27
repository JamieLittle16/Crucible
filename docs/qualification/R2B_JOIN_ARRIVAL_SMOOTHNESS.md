# R2B Join-Arrival Smoothness Qualification

This qualification measures a different property from the R2B preparation microbenchmarks: whether the complete replay-free connection-entry service produces stable player-completion timing when joins are offered steadily or in bounded bursts.

The governing distinction is:

- **service-time distribution** measures the cost of one real R2B entry operation;
- **arrival smoothness** measures the queueing, backlog and completion-spacing consequences when those measured service times are placed under an offered-arrival schedule.

A mechanism can have a good mean or p50 and still produce undesirable completion bunching. Crucible therefore treats low completion-spacing tails, queue tails and burst recovery as first-class performance evidence.

## Production boundary under measurement

Every service sample calls the production function:

```text
enter_r2b_play_blocking_transport
```

The measured path includes:

```text
Handshake / Login
        -> source-admitted Configuration
        -> zero-captured-Play drained ownership handoff
        -> replay-free PreparedR2bPlan preparation
        -> staged publication through the continuing bounded driver
        -> transport drain
        -> WorldProjectionReady
```

Fixture construction is excluded from the timed region. Connection-local allocations and all work performed by the production entry function remain inside it.

The benchmark uses a private counting transport. Client input chunks are prepared once outside timing; reads still copy through the production retained read scratch. Writes consume every produced byte and maintain a checksum without accumulating an unbounded output `Vec`.

Every measured join must reproduce the same semantic witness:

- zero captured Play frames in the source-admitted Configuration context;
- exactly the selected three client read chunks;
- identical output byte count and checksum;
- drained userspace ingress and egress at `WorldProjectionReady`;
- retained pre-play read scratch transferred into continuing Play;
- pending initial teleport acknowledgement ID `1`.

Timing evidence is discarded if this witness changes.

## Why the arrival scheduler is virtual

The benchmark does **not** call `sleep()` to emulate sub-millisecond arrivals. Hosted scheduler wake-up noise would overwhelm the property being measured and make queueing conclusions depend on the CI operating system rather than the Crucible service path.

Instead, real measured service durations feed a deterministic single-lane arrival model:

```text
start_i      = max(offered_i, completion_{i-1})
queue_i      = start_i - offered_i
completion_i = start_i + measured_service_i
```

This models one finite R2B entry-service lane exactly from observed service costs. It is intentionally **not** a permanent listener, worker-count or executor policy. Crucible's architecture keeps authority and worker placement separate; a later multi-lane/executor qualification may place this service behind a different admission mechanism without changing R2B semantics.

Accordingly every artifact states:

```text
model_scope = one-r2b-entry-service-lane
runtime_scheduler_selected_by_this_benchmark = false
```

## Offered-arrival profiles

The model uses the measured mean service cost to normalize load on the current benchmark host. This avoids pretending that one absolute nanosecond arrival interval is portable across unrelated hosted CPUs.

Required profiles are:

1. **steady-80pct** — interval = `5/4 * mean_service`; approximately 80% single-lane offered utilization.
2. **steady-95pct** — interval = `20/19 * mean_service`; exactly 95% nominal utilization before integer rounding and intentionally sensitive to service-time tails.
3. **burst-8of64** — a base interval of `3/2 * mean_service`, with eight arrivals collapsed onto one offered timestamp per 64-arrival cycle. Four phase shifts (`0`, `16`, `32`, `48`) prevent one lucky alignment between burst boundaries and the measured service sequence from defining the result.

The burst profile has headroom on average and therefore tests bounded backlog recovery rather than permanent overload.

## Metrics

The raw service distribution records:

- min, p1, p5, p50, p95, p99, p99.9 and max;
- mean;
- median absolute deviation (MAD) and relative MAD;
- per-epoch mean distribution;
- diagnostic estimated single-lane joins/second.

Every arrival profile records:

- queue-delay distribution;
- end-to-end offered-arrival -> completion (`sojourn`) distribution;
- completion-spacing distribution, including p1/p5 low tails;
- backlog-in-system distribution and maximum;
- fraction of joins that queued;
- completion-compression rate: fraction of completion gaps smaller than half the profile's nominal paced interval;
- maximum consecutive queued run;
- total modeled span.

Low completion-spacing tails are deliberate. A lower p1 is not automatically "better": it can mean the server is releasing players in a bunch after a queue episode. Smoothness decisions must consider service latency, queue latency and completion spacing together.

## Full qualification evidence

The decisive baseline run was GitHub Actions run `33097908497`, artifact `9657168838`, from branch commit `64aecf25f1b60a3baf1bc01e92f9294d4bf86497`. The one-shot full workflow was removed after the artifact was sealed; the permanent R2B workflow retains the cheaper smoke diagnostic.

The qualification executed three independent CPU-pinned processes. Each process used 512 warm-up joins followed by 8,192 measured joins, partitioned into 32 epochs of 256 joins. All three artifacts reproduced the exact same semantic witness.

### Complete R2B entry service

Across the three full processes:

| Metric | Observed range |
| --- | ---: |
| p50 | 60.704–61.325 us |
| p99 | 70.964–76.263 us |
| p99.9 | 80.572–90.861 us |
| max | 103.926–123.351 us |
| diagnostic serial capacity from mean | 16,136–16,327 joins/s |

The estimated joins/second value is **not** a production capacity promise. It is merely the reciprocal of the measured single-lane mean on this hosted CPU and exists to normalize the pressure model.

The 512-join smoke run had observed a 215.253 us p99.9 value. That tail did not persist in any of the three 8,192-join full processes, demonstrating why the full qualification was required before recording the baseline.

### Steady 80% offered load

Across all three full processes:

- queue-delay p99 was **0 ns**;
- queue-delay p99.9 was 13.581–14.108 us;
- backlog p99 was exactly `1` in-system join;
- maximum backlog was `2`;
- maximum consecutive queued run was 3–10 joins;
- completion-compression rate was `0 ppm`;
- completion-spacing p1 was 62.966–66.802 us.

This is a strong low-pressure baseline: ordinary service variance is absorbed without a p99 queue penalty, and rare queue episodes remain shallow.

### Steady 95% offered load

Across all three full processes:

- queue-delay p99 was 12.888–92.303 us;
- queue-delay p99.9 was 44.137–164.495 us;
- backlog p99 was `2–3` in-system joins;
- maximum backlog was `2–4`;
- maximum consecutive queued run was 21–101 joins;
- completion-compression rate was `0 ppm`;
- completion-spacing p1 was 60.223–60.653 us.

This profile is deliberately close to saturation and is therefore sensitive to sparse service-tail placement even when the underlying service distribution is stable. It is a **stress baseline, not a recommended production listener utilization target**. A future admission/executor mechanism should improve or bound these tails rather than using the modeled 95% lane as a default operating point.

### Eight-at-once burst recovery

The four phase-shifted burst profiles were highly stable across all three full processes:

- effective average offered load was approximately 748,536–748,544 ppm;
- queue-delay p99 was 425.139–429.948 us;
- queue-delay p99.9 was 436.349–453.893 us;
- backlog p99 and maximum backlog were both exactly `8` in-system joins;
- maximum consecutive queued run was 22–27 joins;
- completion-spacing p1 was 60.193–60.624 us;
- completion-compression rate was `0 ppm` in every phase and every process.

The important result is not merely that the burst drains. Eight simultaneous offered joins are serialized into roughly one normal service interval between completions rather than being released as an output clump. The phase shifts also show that this recovery shape does not depend materially on one lucky alignment with the measured service sequence.

## Baseline conclusion

For the currently qualified replay-free R2B path, a single finite entry-service lane has a compact and repeatable service distribution and shows no completion compression under the admitted steady or bounded-burst profiles.

The evidence establishes three useful architectural facts:

1. **80% single-lane pressure has substantial headroom.** p99 queue delay remains zero and backlog is shallow.
2. **Near-saturation pressure is dominated by service-tail placement.** 95% modeled load remains bounded, but queued-run length and queue p99/p99.9 vary enough that a future real admission mechanism should not be designed around permanently running one lane this close to saturation.
3. **Bounded bursts recover smoothly.** An eight-arrival burst creates the expected bounded backlog of eight and drains with normal service-scale completion spacing rather than completion bunching.

No scheduler, lane count, listener policy or executor topology is selected by this result. It is a reusable baseline against which those future mechanisms must be measured.

## Interpretation law

Hosted CI evidence is diagnostic rather than an absolute production-capacity claim. A future mechanism should not be accepted solely because it increases estimated joins/second.

A candidate must preserve semantic equivalence and should improve the Pareto surface across:

1. service p50/p99/p99.9;
2. queue p99/p99.9 near saturation;
3. maximum and high-tail backlog;
4. burst recovery / maximum queued run;
5. completion-spacing low-tail compression;
6. connection-local memory and ownership cost.

An average-throughput improvement that creates materially worse queue or completion-spacing tails is a regression under Crucible's performance architecture.

## Architecture gate

This qualification must not introduce a production queue, pool, scheduler, socket abstraction or worker policy merely to make measurement easier. The benchmark is private qualification code; the timed operation remains the existing production R2B composition.

The deterministic single-lane model establishes a reusable admission-pressure baseline. When the real listener/executor policy becomes a production candidate, it should be compared against this baseline using the same offered-arrival metrics plus actual concurrent scheduling evidence.

The permanent CI form remains a CPU-pinned smoke measurement with structural validation only. Hosted timing values are never hard-coded as CI pass/fail thresholds. The full three-process qualification should be repeated when the measured R2B entry semantics, connection ownership shape, or representative admission workloads materially change.

## Commands

Smoke:

```text
cargo bench --locked --package crucible-server --bench r2b_join_arrival -- --smoke --output target/r2b-join-arrival-smoke.json
```

Full:

```text
cargo bench --locked --package crucible-server --bench r2b_join_arrival -- --full --output target/r2b-join-arrival-full.json
```
