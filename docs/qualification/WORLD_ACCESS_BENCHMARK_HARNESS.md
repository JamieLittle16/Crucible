# World-access benchmark harness

**Status:** M0.4C.2 qualification harness  
**Parent:** #72 / #5  
**Normative standard:** `docs/qualification/PERFORMANCE_QUALIFICATION_STANDARD.md`

## Question

The first experiment asks one narrow architectural question:

```text
repeated world point lookup
        vs
resolve an exact chunk window once -> dense repeated reads
```

`ResolvedChunkWindow` already passed independent semantic equivalence before this benchmark existed. This harness determines whether the mechanism earns a **performance** role and where its construction cost breaks even.

It does not assume that the resolved path wins.

## Isolation

Both candidates read:

- the same `LiveChunkCore<BlockStateId, DirectBlockSection<BlockStateId>>` values;
- the standard target vertical lattice (`-64..319` blocks);
- the same deterministic target-state identities;
- the same precomputed `BlockPos` trace.

The deliberately simple direct section keeps this experiment focused on world-routing overhead rather than mixing in the still-separate production section representation decision.

### Reference

The reference owns one conventional `HashMap<ChunkPos, &LiveChunkCore<...>>` directory. Each block read:

1. maps world X/Z to a chunk coordinate;
2. performs the hash-directory lookup;
3. invokes the checked `LiveChunkCore::get_block` path.

The directory is constructed once because a real world already owns a long-lived chunk directory. Rebuilding it per query would manufacture cost that production does not pay.

### Resolved candidate

The candidate constructs one exact rectangular `ResolvedChunkWindow`, which performs boundary/generalization work once and stores dense row-major chunk references. Repeated reads use its admitted dense path.

Window construction and destruction are measured separately and also included inside the construction+N-read whole-cost experiments.

## Workloads

Full mode includes positive and negative coordinate cases for:

- one-chunk random reads;
- 3x3 collision-shaped compact volume sweeps;
- 5x5 pathfinding-shaped irregular local walks;
- 9x9 streaming/large-working-set reads.

The trace is generated before timing and deterministic from the case identity. The harness first computes both complete semantic checksums and refuses to benchmark unequal work.

## Measurement regimes

This harness currently qualifies **naturally warm / steady-state routing** plus **window construction and amortization**.

It deliberately does not claim a synthetic cache-flushed cold-start result. A genuine cold/cache-state experiment must be added separately if profiling makes it relevant.

For steady-state timing:

- warm-up round count is explicit and retained in the artifact;
- reference/resolved order alternates every round;
- every measured round stores both raw durations and the order used;
- p50/p95/p99/max are summaries over retained samples, never substitutes for them.

For whole-cost timing, each configured reuse count `N` measures:

```text
reference = N reads through the already-existing world directory
resolved  = construct window + N reads + destroy window
```

This prevents resolution cost from being moved outside the decision boundary.

The report also derives a median analytical break-even from measured setup and query-path medians. The actual construction+N samples remain authoritative when the derived and observed curves disagree.

## Machine provenance

`crucible-benchmark-support` records the common machine/toolchain identity used by Crucible performance labs:

- exact commit;
- verbose Rust compiler identity and target triple;
- CPU vendor/model/family/stepping/microcode;
- process CPU/memory-node allowance;
- online CPU and SMT state;
- cache hierarchy/line size/sharing topology;
- governor/current/min/max frequency where exposed;
- turbo policy where exposed;
- kernel;
- THP and perf-event policy;
- memory size/load average;
- Rust flags.

The existing section benchmark is switched onto the same collector so machine-evidence fields cannot silently diverge between labs.

## CI smoke

Hosted CI runs:

```bash
cargo run --release --locked \
  --package crucible-world-access-qualification \
  --bin world_access_bench -- \
  --smoke \
  --output target/world-access-smoke.json
```

CI validates the artifact schema and presence of paired/setup/whole-cost samples.

**Hosted timing is diagnostic only. It cannot admit or reject a production mechanism.**

## Controlled target-hardware run

Choose one intended physical core/logical CPU after inspecting topology. Avoid unrelated work on its SMT sibling for the uncontended single-thread qualification when practical.

Example:

```bash
CPU=4
mkdir -p qualification-results/world-access

taskset -c "$CPU" \
  cargo run --release --locked \
    --package crucible-world-access-qualification \
    --bin world_access_bench -- \
    --full \
    --require-single-cpu \
    --output qualification-results/world-access/resolved-window.json
```

`--require-single-cpu` rejects the run unless the process observes exactly one allowed logical CPU. The artifact still records the full affinity/cache/SMT/frequency context.

For a production decision, repeat controlled runs as required by the performance standard rather than treating one invocation as sufficient evidence. Preserve raw artifacts; do not copy only summary numbers into a decision record.

## Decision

The resolved mechanism earns a default HOT role only if:

1. equivalence remains green;
2. steady-state improvement is material beyond measured noise on an intended workload;
3. actual construction+N measurements show realistic reuse amortizes setup;
4. memory/tail/other whole-cost effects are acceptable;
5. the result reproduces on controlled target hardware.

The default complexity filter is roughly a >=5% CPU/latency improvement beyond noise, not a contractual magic number.

If it loses, or wins only after unrealistic reuse, Crucible keeps the semantic mechanism only where it is useful and does not route ordinary HOT access through it by pride.

## Next low-level hypothesis

The admitted resolved inner path still performs signed world-to-chunk division/remainder work for every read. **Do not optimize this yet.**

Only if the routing experiment shows that global lookup removal is worthwhile and profiling/counter evidence identifies coordinate resolution as a material remaining cost should a separate candidate test:

```text
precomputed block-space origin/bounds
+ nonnegative relative offsets
+ >> 4 chunk selection
+ & 15 local coordinate extraction
```

That candidate must pass the same semantic traces and its own whole-cost benchmark. Unsafe indexing, SIMD, manual prefetch and architecture-specific code remain separate hypotheses.
