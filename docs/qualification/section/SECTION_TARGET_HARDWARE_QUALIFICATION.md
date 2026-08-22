# Section target-hardware qualification protocol

Status: **M0.3D performance-qualification design; no production winner yet**  
Parent: #19  
Input policy: `vanilla-section-representative-v1`

This document defines the final measurement boundary used to select Crucible's first production block-section representation.

Correctness is already a prerequisite. This layer answers a different question:

> Among the already-qualified mechanisms, which are non-dominated on real vanilla section populations and controlled mutation/tail workloads on target hardware?

No GitHub-hosted timing can answer that question.

## Evidence chain

```text
M0.3C semantic qualification
        +
complete representative-v1 population
        ↓
structural corpus-set firewall
        ↓
independent population admission
        ↓
self-describing schema-2 handoff artifact
        ↓
independent target-hardware input preflight
        ↓
content-addressed performance run plan
        ↓
┌──────────────────────────────┐
│ steady-state real-corpus CPU │
│ isolated process/RSS         │
│ controlled write/tail tests  │
└──────────────────────────────┘
        ↓
per-dimension, per-seed evidence
        ↓
noise / Pareto analysis
        ↓
committed M0.3D decision record
```

Every arrow is a fail-closed boundary.

## 1. Benchmark handoff input

Qualifying performance runs consume the extracted directory produced by the merged `Section Representative Set Qualification` workflow.

The consumer must **not** trust:

- the ZIP/artifact name;
- a manually supplied `decision_eligible=true` flag by itself;
- file timestamps;
- GitHub UI success status alone;
- an individual representative member;
- a corpus copied without its admission evidence.

`tools/section_performance_input.py` independently validates the handoff before any timing process starts.

It requires:

- schema-2 `artifact-manifest.json`;
- `qualification_complete = true`;
- `decision_eligible = true`;
- `benchmark_handoff_eligible = true`;
- canonical artifact-manifest digest;
- exact file inventory with no untracked files, duplicate paths, path traversal or symlinks;
- SHA-256 agreement for every retained file;
- canonical `population-admission.json` digest;
- canonical `corpus-set.json` population/evidence digests;
- exact raw `corpus-set.json` file digest bound by admission;
- exact frozen representative-v1 plan;
- exact Minecraft 26.2 target/state-data identity;
- four frozen seed identities;
- four distinct admitted member corpus identities;
- raw SHA-256 of every `seed-N/member.corpus`;
- representative-member corpus provenance headers;
- `decision_scope = dimension-separated-only`;
- `cross_dimension_score_allowed = false`;
- equal seed weighting and no implicit dimension weighting.

A successful preflight emits `section-performance-run-plan-v1`, whose own SHA-256 binds the exact admitted population and four corpus paths/digests to the benchmark policy.

The run-plan output is written **outside** the admitted artifact directory so the source evidence remains immutable.

## 2. What real-corpus timing measures

The representative population is best suited to weighting operations whose cost depends directly on the resident section representation and naturally generated section contents.

The real-corpus CPU layer therefore measures steady-state operations such as:

- random point reads;
- sequential complete-section reads;
- deterministic 4×4×4 volume reads;
- positive `maybe_contains` queries using guaranteed-present needles;
- negative `maybe_contains` queries using guaranteed-absent needles.

### Setup is outside the timed region

Corpus parsing, target validation, candidate construction, access-index generation, positive/negative needle selection and warm-up preparation are never included in steady-state access timing.

A representation must not appear slower merely because the qualification harness parsed its input differently, nor faster because setup was accidentally omitted for another candidate.

### Keep seed evidence visible

Within each dimension the four seeds have equal policy weight. Results therefore retain raw per-seed measurements; they are not first collapsed into one section-count-weighted global sample.

The eventual analysis may summarize equal-seed evidence inside a dimension, but must preserve the raw samples needed to audit that calculation.

## 3. What remains controlled/synthetic

Naturally generated terrain is not a clean causal workload for mutation boundaries. The existing controlled benchmark laboratory remains authoritative for:

- same-state replacement;
- low-entropy replacement;
- high-entropy replacement;
- palette churn;
- exact 16/17 and 256/257 live-cardinality boundaries;
- packed-width transitions;
- representation-promotion p50/p95/p99/max latency.

The final decision combines these controlled tails with real-population steady-state and memory evidence. It does not replace one with the other.

## 4. Process/RSS isolation

Deterministic `owned_bytes()` accounting remains useful but is not a substitute for resident-memory evidence.

RSS qualification runs one **candidate + dimension** in a fresh child process. The child:

1. validates the run-plan identity;
2. streams the four admitted member corpora;
3. constructs only the selected dimension's sections into that candidate;
4. does not retain raw corpus line buffers or a second semantic image after construction;
5. reaches an explicit post-construction barrier;
6. records process memory evidence;
7. performs optional read-touch stabilization where the protocol requires it;
8. exits.

No custom allocator is introduced merely to count memory.

Linux evidence should include, where available:

- `/proc/self/status` `VmRSS`;
- `/proc/self/status` `VmHWM`;
- deterministic aggregate `owned_bytes`;
- section count;
- representation histogram.

Input-parser and orchestration processes are not the candidate RSS process.

## 5. Monomorphized candidate execution

The target runner may select a candidate with one cold outer `match`, but measured section operations remain generic/monomorphized.

No trait object, lock, atomic, `Arc`, global registry lookup or service-locator dispatch is introduced into the HOT cell path for benchmark convenience.

The required identities remain:

- `direct-reference` — non-production oracle/baseline;
- `direct`;
- `adaptive`;
- `fast-local`;
- `packed-local`.

Reference timings may be informative but cannot become a production policy.

## 6. Dimension firewall

Overworld, Nether and End remain separate decision strata.

A target run must emit separate evidence for:

```text
minecraft:overworld
minecraft:the_nether
minecraft:the_end
```

The harness must not create a cross-dimension weighted score.

The target's 24:16:16 section lattices would otherwise silently create a 3:2:2 weighting that says nothing about real server time spent in each dimension.

If Crucible later introduces a documented gameplay/workload profile that weights dimensions, that profile is a separate evidence object and policy decision.

## 7. Target hardware protocol

A production-qualifying timing artifact records at minimum:

- exact Crucible commit SHA;
- `population_sha256`;
- population `admission_sha256`;
- performance run-plan SHA-256;
- candidate and dimension;
- all four member corpus SHA-256 values;
- Rust compiler verbose identity;
- target triple;
- release/codegen policy;
- `RUSTFLAGS` / encoded Rust flags;
- CPU model;
- process CPU affinity;
- OS/kernel;
- governor;
- current/min/max frequency when exposed;
- turbo state when exposed;
- load average;
- benchmark harness version;
- warm-up and sample counts;
- raw timing samples.

### CPU affinity

Qualifying runs should pin the benchmark process to a stable physical core when practical. The artifact records the actual allowed CPU set; the runner never silently assumes pinning succeeded.

### Noise

No mechanism is selected from one timing sample. The qualification record must retain repeated samples and sufficient context to distinguish a material difference from run noise.

Hosted CI only verifies that the target-hardware harness compiles, its cold preflight rejects malformed inputs, and bounded diagnostic smoke runs execute. Hosted timing remains non-qualifying.

## 8. Decision rule

The final M0.3D decision follows #19:

1. any correctness-failing mechanism is ineligible;
2. strictly CPU/tail + memory dominated mechanisms are rejected;
3. multiple mechanisms survive only for a genuine profile-relevant Pareto trade-off;
4. adaptive/thermal switching is not added unless whole-trace evidence beats the best simple policy materially;
5. permanent complexity requires a material effect beyond noise—roughly ≥5% CPU/latency or ≥10% resident memory on an official workload class without a disqualifying regression.

The exact threshold interpretation is recorded with the measured noise/confidence context rather than treated as a universal constant.

## 9. Loser record

When a mechanism loses, production code may be deleted, but the candidate registry and decision record retain:

- candidate ID and implementation shape;
- original hypothesis;
- correctness evidence;
- performance artifact digests;
- per-dimension CPU/tail/RSS outcome;
- Pareto status;
- rejection rationale;
- any useful mechanism-specific lessons;
- conditions under which revisiting it would be justified.

This is the practical meaning of:

> **Code can disappear; experimental knowledge cannot.**

## 10. Exit gate

M0.3D is complete only when:

1. the complete four-seed population has a real `benchmark_handoff_eligible=true` artifact;
2. the target-hardware preflight independently accepts that exact artifact;
3. every candidate has controlled-hardware CPU evidence for each dimension/seed stratum;
4. mutation/promotion tail evidence remains green and current;
5. deterministic owned-byte and isolated process/RSS evidence exist;
6. raw samples and hardware provenance are archived;
7. a committed Pareto/decision record selects the production policy and documents every rejection;
8. losing experimental implementations are removed from the normal engine path while their records remain;
9. the selected mechanism has no accidental HOT-path dynamic dispatch, synchronization or global lookup.
