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

1. **steady-80pct** — interval = `1.25 * mean_service`; approximately 80% single-lane offered utilization.
2. **steady-95pct** — interval = `1.05 * mean_service`; approximately 95% utilization and intentionally sensitive to service-time tails.
3. **burst-8of64** — a base interval of `1.5 * mean_service`, with eight arrivals collapsed onto one offered timestamp per 64-arrival cycle. Four phase shifts (`0`, `16`, `32`, `48`) prevent one lucky alignment between burst boundaries and the measured service sequence from defining the result.

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

## Commands

Smoke:

```text
cargo bench --locked --package crucible-server --bench r2b_join_arrival -- --smoke --output target/r2b-join-arrival-smoke.json
```

Full:

```text
cargo bench --locked --package crucible-server --bench r2b_join_arrival -- --full --output target/r2b-join-arrival-full.json
```
