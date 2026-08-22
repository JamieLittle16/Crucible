# Section target-hardware orchestrator — M0.3D

Parent: #19  
Depends on: controlled candidate-isolated child protocol (#43)  
Status: **population evidence controller; no representation decision**

This document specifies the controller that turns already-admitted representative-population packs into reproducible target-hardware evidence.

It does not define Minecraft semantics, choose a representation, or turn hosted CI timing into qualification evidence.

## Principle

A benchmark result is decision-bearing only if Crucible can answer all of these independently:

1. **What source revision was built?**
2. **What exact executable bytes were run?**
3. **What exact representative population bytes were read?**
4. **Which candidate/dimension/round/order position was measured?**
5. **What CPU and memory-node placement was in force?**
6. **Did the child report agree with the controller's expected identities?**
7. **Was the repeated run stable enough to distinguish mechanism cost from environmental noise?**

A process exiting successfully is not sufficient.

## Inputs

The controller consumes:

- a clean Crucible checkout;
- a content-addressed `section-target-benchmark-pack-set` produced from an independently admitted representative-set artifact;
- an explicit Linux logical CPU;
- an external evidence directory;
- smoke or qualification mode;
- an explicit/reproducible round count.

The pack root and output directory must be outside the repository so benchmark preparation cannot make the checkout dirty or contaminate source identity.

## Source → binary firewall

Qualification fails closed when the checkout has tracked or untracked changes.

The controller refuses non-empty compilation overrides including:

- `RUSTFLAGS`;
- `CARGO_ENCODED_RUSTFLAGS`;
- `RUSTC`, `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER`;
- `RUSTC_BOOTSTRAP`;
- `CARGO_BUILD_TARGET` / `CARGO_BUILD_RUSTFLAGS`;
- `CARGO_PROFILE_RELEASE_*`;
- target-specific Cargo rustflags.

It then:

1. requires pinned Rust `1.97.1` and the pinned rustc commit;
2. uses an isolated `CARGO_HOME`;
3. uses an isolated `CARGO_TARGET_DIR`;
4. disables incremental compilation;
5. builds `section_bench` itself with the workspace release profile;
6. records hashes of `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml`;
7. hashes the exact produced executable.

The child report's `commit_sha` and nominal codegen fields remain useful cross-checks, but the controller does **not** treat them as proof of executable identity. The executable SHA-256 is the binary identity.

## Pack firewall

Before any run the controller independently rechecks:

- pack-manifest schema/kind and canonical manifest digest;
- exact Minecraft/state-data identity against the checkout;
- population/admission/source-artifact identities;
- exactly four distinct seed/corpus identities with indices `0..3`;
- exactly the three standard dimensions;
- the dimension-separated decision firewall;
- per-member/per-dimension section counts;
- safe pack basenames;
- exact pack file size and SHA-256.

Immediately before **and after every child**, the controller re-hashes:

- the exact dimension pack;
- the exact benchmark executable.

Mutation of either during a child invalidates the run.

## Placement

Qualification is Linux-first.

The requested CPU must already be present in the controller's allowed affinity set. Every child is launched with:

```text
taskset -c <cpu> <exact-built-section_bench> ...
```

The child must independently report `Cpus_allowed_list=<cpu>` or it is rejected.

The evidence also records:

- `Mems_allowed_list`;
- package/core/thread-sibling topology when exposed;
- NUMA-node association when exposed;
- governor/frequency bounds/current frequency;
- turbo policy when exposed;
- load average;
- thermal-zone readings when exposed.

Missing optional sensors remain `unknown`; affinity/identity invariants do not silently degrade.

## Schedule

Every round visits all three dimensions and all five candidate identities.

Dimension order rotates by round. Candidate order rotates independently per dimension:

```text
candidate_offset = round + dimension_ordinal
```

Therefore, over every five rounds, each candidate occupies every candidate-order position exactly once **within each dimension**.

This prevents a candidate from systematically receiving the coldest/earliest or hottest/latest slot.

Qualification currently requires at least five rounds and a multiple of five. More rounds may be used when noise or hardware policy warrants them.

The complete deterministic schedule is emitted in the evidence artifact.

## Child acceptance

A child result is accepted only if the controller independently verifies:

- child schema/harness version;
- smoke vs qualification mode;
- candidate identity and production flag;
- workspace release/codegen contract;
- repository commit;
- empty Rust flag overrides;
- exact CPU affinity;
- non-empty memory-node affinity provenance;
- target/data/state digests;
- population/admission identity;
- exact dimension and section count;
- RSS protocol and signed `loaded - baseline` arithmetic;
- representation counts summing to the pack section count;
- one construction timing sample per section;
- exact steady-state workload set;
- expected raw timing sample counts and positive operation counts.

Accepted child JSON, stdout and stderr are all retained by content hash.

## Noise qualification

Raw samples are never rewritten or discarded to make a run look stable.

Across repeated child processes, the controller computes integer:

- median;
- median absolute deviation (MAD);
- relative MAD in parts per million;
- min/max.

The first controller policy uses conservative diagnostic gates:

| Evidence | Relative MAD ceiling |
|---|---:|
| candidate-independent integer control | 5% |
| repeated workload p50 | 10% |
| RSS delta | 10% |

RSS additionally requires a positive repeated-run median for decision-bearing memory evidence. A zero/negative or unstable RSS observation is preserved but cannot qualify memory ranking.

These are **qualification noise gates**, not claims about game-performance significance. The later Pareto/decision layer still applies the separate material-benefit thresholds defined by #19.

A run may therefore be:

- structurally complete;
- semantically correct;
- successfully executed;
- **but still diagnostic rather than decision-bearing** because its environment was too noisy.

## Aggregation firewall

All aggregates remain dimension-separated.

The controller does not invent a weighted Overworld/Nether/End score. Cross-dimension scoring remains forbidden until a separately admitted workload model exists.

The controller also does not select a winner. It produces inputs for the later Pareto decision.

## Evidence artifact

A completed run retains:

- controlled-build stdout/stderr;
- exact build/toolchain/source hashes;
- executable SHA-256;
- pack-manifest/population/admission/source-artifact identities;
- pack SHA-256 per dimension;
- CPU/topology/NUMA provenance;
- deterministic schedule;
- every child evidence JSON plus stdout/stderr hashes;
- pre/post environmental snapshots around every child;
- repeated-run aggregates;
- explicit noise thresholds/results/reasons;
- orchestration evidence SHA-256;
- final artifact-manifest SHA-256 over retained files.

## Hosted smoke

CI may run a tiny three-dimension pack through one controller round to prove orchestration mechanics.

Hosted smoke must remain:

```text
decision_evidence_eligible = false
```

regardless of apparent timing quality.

GitHub-hosted timing and RSS values are never copied into the production decision.

## Deliberate next slice

This controller initially orchestrates real-population steady-state/RSS evidence only.

The next slice will add a **candidate-isolated synthetic mode** for replacement/churn/promotion-tail evidence and run it through the same controlled build, CPU pinning, order-rotation and noise protocol. Only the combined real-population + synthetic-tail + correctness evidence may feed the first section Pareto decision.
