# Section target-hardware qualification

Status: **M0.3D performance qualification protocol; no production winner yet**  
Parent: #19  
Consumes: admitted `vanilla-section-representative-v1` population

This document defines the controlled CPU/tail/RSS layer that may finally feed the section-representation Pareto decision.

The governing rule is:

> Correctness, workload identity, timing, memory and selection are separate evidence layers. None may silently stand in for another.

GitHub-hosted runners compile and smoke-test this machinery only. Their timing and RSS values are never production-selection evidence.

## Evidence chain

```text
M0.3C semantic qualification
        +
admitted four-seed representative-v1 population
        ↓
content-addressed per-dimension benchmark packs
        ↓
candidate-isolated target-hardware child processes
        ↓
real-population steady-state CPU/tail + process RSS
        +
controlled synthetic mutation/promotion tails
        ↓
noise / provenance validation
        ↓
dimension-separated Pareto diagnostics
        ↓
explicit #19 decision record
```

The final decision may only use evidence whose population, source, code, toolchain and hardware identities are all explicit.

## Why benchmark packs exist

The canonical representative corpus is deliberately human-inspectable text. That is excellent for validation, but parsing tens of millions of decimal state IDs is not part of the section mechanism and must not contaminate storage benchmarks.

Target-hardware qualification therefore derives a cold **benchmark pack** from the already admitted population.

A pack:

- contains only normalized Minecraft 26.2 `BlockStateId` values;
- stores them as little-endian `u16` values in exact corpus cell order;
- is separated by dimension;
- concatenates frozen seed members in seed-index order;
- carries the admitted `population_sha256` and `admission_sha256`;
- is itself SHA-256-addressed by the pack-set manifest;
- never contains Mojang palette slots, NBT, source code or implementation internals.

The pack is an acceleration artifact, not a new source of truth. Its builder must revalidate the representative-set artifact and bind every source member corpus SHA before emission.

## Pack format

Schema 1 is:

```text
CRUCIBLE-SECTION-BENCH-PACK|1
TARGET|minecraft=26.2|protocol=776|data=4903|state_count=32366|generation_sha256=<sha256>
POPULATION|population_sha256=<sha256>|admission_sha256=<sha256>
DIMENSION|name=<resource-location>|section_count=<N>
DATA
<binary payload>
```

The binary payload contains exactly:

```text
N * 4096 * sizeof(u16)
```

bytes, with each state ID encoded little-endian in frozen section cell order.

No trailing bytes are permitted. A state ID outside the frozen target universe is a hard failure.

## Dimension firewall

The admitted population has:

```text
decision_scope = dimension-separated-only
cross_dimension_score_allowed = false
```

The hardware layer preserves this rule. Overworld, Nether and End are benchmarked and reported separately.

No target-hardware tool may invent a global gameplay weighting such as 3:2:2 from vertical section counts. A future cross-dimension workload model would require a separately justified, versioned policy.

## Candidate isolation

Each real-population run measures exactly one candidate and one dimension in a fresh process:

- `direct-reference` — diagnostic correctness/performance baseline only;
- `direct`;
- `adaptive`;
- `fast-local`;
- `packed-local`.

The process loads only the selected candidate representation from a binary pack. Raw section-state buffers are reusable scratch storage and are not retained.

Candidate isolation is mandatory for RSS evidence and strongly preferred for CPU evidence. It prevents one candidate's allocator state, caches or retained memory from becoming another candidate's starting condition.

## RSS protocol

Process memory evidence is not inferred from logical owned bytes.

For each child process:

1. open and validate the pack header;
2. allocate one reusable fixed-size section decode buffer;
3. record baseline process `VmRSS`;
4. construct and retain every candidate section in the selected dimension;
5. touch/read back every section as part of semantic reconstruction;
6. record post-load `VmRSS` and `VmHWM`;
7. report the RSS delta, peak RSS and deterministic logical owned bytes separately;
8. only after the RSS snapshot allocate timing query plans.

The qualification artifact must say explicitly that RSS is process-level evidence and logical bytes are representation-level evidence.

No custom allocator is added to production merely to count allocations.

## Real-population CPU workloads

Steady-state measurements operate on the already constructed candidate sections and exclude pack parsing/construction setup:

- deterministic random point reads;
- deterministic sequential full-section reads;
- deterministic 4×4×4 volume reads;
- positive `maybe_contains`;
- negative `maybe_contains` using a state proven absent from the loaded population.

Candidate construction latency is recorded separately, excluding pack I/O, with p50/p95/p99/max.

Synthetic replacement, churn and promotion workloads remain the correct place for controlled mutation and transition-tail experiments; real terrain is not mutated merely to manufacture an arbitrary write distribution.

## Samples and noise

A qualifying orchestration run uses repeated candidate-isolated processes rather than trusting one execution.

The orchestration layer must record at minimum:

- raw per-process timing samples;
- per-process p50/p95/p99/max;
- repeated-run medians and spread;
- exact candidate order;
- CPU affinity;
- process load context;
- governor/frequency/turbo metadata where exposed;
- Rust/toolchain/target/codegen identity;
- repository commit SHA;
- pack-set/population/admission identities.

Candidate ordering must be rotated deterministically across rounds so thermal/frequency drift is not always assigned to the same mechanism.

A run with uncontrolled or materially unstable noise remains diagnostic even if every command succeeded.

## Hardware requirements

Production qualification is Linux-first for M0.3D because the initial process-memory protocol uses `/proc/self/status` and Linux CPU-affinity metadata.

The orchestration layer must pin each child to an explicit logical CPU where `taskset` is available and verify the child reports that same allowed CPU set.

Qualification does not require disabling turbo or forcing one universal governor, but the actual state must be recorded and stable enough that repeated control samples are interpretable. If the machine cannot provide a sufficiently stable run, the result is diagnostic rather than qualifying.

## Pareto rule

The decision generator does not collapse unrelated workloads into one arbitrary score.

Within each dimension it reports component-wise evidence for:

- steady-state CPU/latency workloads;
- p95/p99/max transition/construction tails;
- process RSS delta/peak;
- logical owned bytes.

A candidate may be called strictly dominated only when the evidence supports the #19 rule: it is not materially better on any official decision-bearing dimension/workload and another candidate is at least as good across the relevant CPU/tail and memory axes with a material advantage beyond measured noise.

The suggested complexity bar remains approximately:

- >=5% CPU/latency benefit, or
- >=10% resident-memory benefit,

on at least one official workload class without a disqualifying regression. The final record must include the observed noise/confidence context rather than applying these percentages mechanically.

## Loser preservation

After a production policy is selected, dominated mechanism code should leave the normal engine path.

Its experiment record remains permanently committed with:

- stable candidate ID;
- hypothesis and mechanism shape;
- correctness evidence;
- benchmark pack/population identities;
- raw performance artifact digests;
- measured strengths/weaknesses;
- exact rejection rationale;
- any conditions under which revisiting it would be justified.

This preserves the project rule:

> Code can disappear; experimental knowledge cannot.

## CI versus qualification

CI must:

- unit-test pack construction and corruption rejection;
- compile the release benchmark child;
- run a tiny release-mode population smoke pack;
- verify artifact schema/provenance/workload/candidate fields;
- preserve all existing semantic/full-qualification gates.

CI must **not**:

- publish hosted-runner numbers as target-hardware evidence;
- select a representation;
- freeze promotion thresholds from smoke runs.

## Exit condition

The target-hardware layer is complete only when:

1. an admitted representative-set artifact is converted into content-addressed per-dimension packs;
2. pack identities are bound to the exact population/admission identities;
3. every production candidate plus direct-reference is benchmarked in isolated processes for every dimension;
4. CPU affinity/hardware/toolchain/codegen metadata is complete;
5. raw samples and noise diagnostics are retained;
6. process RSS and logical bytes are both present and clearly distinguished;
7. synthetic mutation/promotion tails are measured on the same controlled target hardware;
8. dimensions remain separate;
9. a committed Pareto/decision record identifies winners and losers with evidence;
10. rejected implementations are removed from the normal production path while their experiment records remain.
