# Crucible CI and qualification roadmap

Status: **normative engineering process**  
Owner: repository-wide  
Motto: **a lower evidence ring never substitutes for a higher one**

Crucible treats CI as executable engineering law rather than a collection of convenient checks. A test exists because it excludes a concrete failure class. A heavyweight gate is activated when the subsystem it can meaningfully test exists; it is not added early as ceremony.

## Evidence rings

| Ring | Purpose | Current status |
|---|---|---|
| 0 | repository, toolchain and supply-chain law | **ACTIVE** |
| 1 | cheap structural correctness | **ACTIVE** |
| 2 | subsystem semantic qualification | **ACTIVE for world sections** |
| 3 | official vanilla/source/runtime qualification | **ACTIVE for state data and world sections** |
| 4 | hostile/stress qualification | **ACTIVATE BY TRIGGER** |
| 5 | controlled performance qualification | **ACTIVE AS LAB; decision evidence still pending** |

Passing Ring 1 does not establish vanilla parity. Passing vanilla parity does not establish performance. Performance can never excuse a semantic failure.

---

## Ring 0 — repository and supply-chain law

### Active now

Universal CI must enforce:

- pinned Rust toolchain;
- checked-in `Cargo.lock` and `--locked` builds;
- no tracked Mojang source/runtime artifacts;
- all external GitHub Actions pinned to full 40-hex commit SHAs;
- Git Rust dependencies forbidden;
- crates.io dependencies allowed only as exact reviewed `name@version` entries in `config/dependency-allowlist.txt`;
- stale or duplicate allowlist entries rejected;
- mutable Action major/minor tags rejected;
- Dependabot may propose Action/Cargo changes, but the normal review/CI path decides whether they are accepted.

### Dependency-policy escalation trigger

**Before the first substantial third-party Rust dependency graph is admitted** (and no later than the first production subsystem that requires external crates), add automated license/advisory/source auditing, expected to use `cargo-deny` or an equally strict replacement.

The mature policy must cover at minimum:

- accepted/denied licenses;
- known security advisories and explicit temporary exceptions with expiry;
- duplicate crate versions;
- registry/source restrictions;
- banned crates or dependency families where architecture requires it;
- exact exception rationale in version control.

The current exact allowlist is intentionally simpler because the workspace has zero external Rust packages today.

---

## Ring 1 — structural correctness

### Active now on every pull request

- handwritten Rust formatting;
- workspace/all-target/all-feature compilation;
- Clippy with warnings denied;
- full Rust unit/integration tests;
- all Python tool tests;
- Python syntax compilation for the complete `tools/` tree;
- rustdoc with warnings denied;
- repository guard;
- CI/supply-chain policy guard.

Generated Rust is generator-owned and is not reformatted by a second writer.

### Generated-data reproducibility

The official state-data workflow must freshly:

1. probe the pinned official Minecraft runtime;
2. bind it to the committed source qualification;
3. regenerate the qualified target Rust and manifest outside the repository tree;
4. byte-compare both outputs against the committed generated artifacts.

Any byte difference is a failure, even if both versions compile.

---

## Ring 2 — subsystem semantic qualification

### Active: world section storage

The section qualifier uses:

- an independent direct reference;
- optimized candidates;
- deterministic quick traces in ordinary CI;
- full multi-seed qualification in release mode;
- exact previous-value/readback checks;
- complete 4096-cell barriers;
- fresh summary recomputation;
- source-backed fixtures.

### Activation rule for future subsystems

A subsystem receives its own Ring-2 qualifier when either:

- it has more than one mechanism/optimization path;
- a simple independent reference can be maintained;
- it owns gameplay-visible state transitions;
- a bug could be schedule/history/representation dependent.

Likely future qualifiers: collision, lighting, heightmaps, entity spatial indexing, tick queues, packet encoding, chunk lifecycle and persistence.

---

## Ring 3 — vanilla qualification

### Active now

- official 26.2 runtime state probe;
- source+runtime state binding;
- source-backed section fixtures;
- real official-server world generation;
- real Anvil/NBT corpus extraction;
- independent Python validation;
- independent Rust reconstruction through all section mechanisms;
- explicit decision-eligibility policy.

### Future activation: whole-server differential laboratory

**Trigger: first bootable Crucible server capable of completing a scripted client/server scenario.**

Build a deterministic scenario runner that executes equivalent scenarios against Mojang and Crucible and compares normalized observable state. Grow the suite progressively to include:

- status/login/configuration/play transitions;
- chunk and biome contents;
- movement/collision;
- block interaction;
- entity state;
- scheduled/random ticks where determinism permits;
- inventory/components;
- save/reload results;
- packet-visible semantics where exact wire parity is required.

The comparison should prefer semantic normalization over copying Mojang implementation internals.

---

## Ring 4 — hostile, fuzz, restart and concurrency qualification

These gates are deliberately **not active yet** where the relevant production subsystem does not exist.

### Fuzzing

**Trigger: first production parser/decoder that consumes untrusted client, network, NBT, compression, command or persistence input.**

Required shape:

- short bounded fuzz/regression corpus in PR CI;
- persisted reproducer corpus;
- longer scheduled fuzz jobs;
- every discovered crash/invariant violation becomes a permanent minimized regression.

### Concurrency/model checking

**Trigger: first shared ownership transfer, cross-region handoff, concurrent chunk install, revision race, scheduler handoff or other gameplay-relevant concurrency primitive.**

Required shape:

- deterministic interleaving/model tests where practical;
- race/revision/install-barrier tests;
- Loom or an equivalent model checker where it adds real coverage;
- sanitizer/race-tool lanes where supported and useful;
- no concurrency test may weaken the singular-mutation-authority law.

### Persistence/crash qualification

**Trigger: first production world persistence path.**

Required cases:

- save -> stop -> reopen -> semantic comparison;
- repeated save/reload cycles;
- interrupted/partial write behavior;
- malformed/corrupt input rejection;
- unknown/new data handling policy;
- revision/save-barrier correctness;
- deterministic byte comparison where byte identity is itself contractual.

### Soak/stress

**Trigger: first end-to-end server loop with meaningful world/player activity.**

Use deterministic workload scripts, bounded resource assertions, queue/backpressure telemetry and long-running invariant checks. Scheduled/nightly is preferred over slowing every small PR.

---

## Ring 5 — performance qualification

Hosted GitHub runner timings are **diagnostic only**.

Production performance decisions require controlled hardware evidence containing:

- commit SHA;
- Minecraft/data digests;
- harness/workload version;
- Rust/toolchain/codegen identity;
- CPU/core affinity;
- OS/kernel;
- governor/frequency context where available;
- warmup/sample counts;
- median plus meaningful tail/spike metrics;
- deterministic owned-byte accounting;
- process/RSS measurements where appropriate;
- noise/confidence context.

For world sections, #19 remains open until representative vanilla-derived weighting, target-hardware CPU/tail measurements, RSS, Pareto analysis and the committed production-selection record exist.

---

## Main-branch governance

`main` is intended to be mechanically protected, not protected by convention.

### Required repository setting

Apply a GitHub branch protection rule/ruleset to `main` with:

- require a pull request before merge;
- require the ordinary `CI` checks (`Rust and tooling quality` and `Repository guard`) to pass;
- require the branch to be up to date before merge;
- require conversation resolution;
- block force pushes;
- block branch deletion;
- no routine bypass of failing required checks;
- continue using squash merge for clean milestone history.

While Crucible has a single maintainer, do **not** require a second human approval merely to manufacture process. Add review-count/CODEOWNERS requirements when additional maintainers are actually participating.

Path-filtered heavyweight workflows should not be made direct globally-required checks unless GitHub can guarantee a neutral/pass result when irrelevant. As more qualification families appear, introduce a small always-running **qualification-gate aggregator** that decides which subsystem gates apply and reports one required result.

### Break-glass policy

If a future production emergency genuinely requires bypass, the exceptional commit/PR must document:

- why normal gates could not be satisfied first;
- exact risk accepted;
- follow-up issue;
- required post-merge qualification.

Bootstrap convenience is not a break-glass reason.

---

## Activation checklist

This table is the anti-forgetting list. Updating subsystem plans must also update this table when a trigger becomes true.

| Future gate | Activation trigger | Status |
|---|---|---|
| automated license/advisory dependency audit | first substantial external Rust dependency graph | **PENDING TRIGGER** |
| whole-server Mojang/Crucible differential runner | first bootable scripted server scenario | **PENDING TRIGGER** |
| packet/parser fuzzing | first production untrusted protocol/parser path | **PENDING TRIGGER** |
| NBT/persistence fuzzing | first production persistence reader/writer | **PENDING TRIGGER** |
| save/restart/crash qualification | first production persistence path | **PENDING TRIGGER** |
| concurrency model/race qualification | first gameplay-relevant shared ownership/handoff | **PENDING TRIGGER** |
| deterministic soak/stress suite | first meaningful end-to-end server workload | **PENDING TRIGGER** |
| scheduled long fuzz jobs | first fuzz target admitted | **PENDING TRIGGER** |
| qualification-gate aggregator | second independent path-filtered subsystem qualifier | **PENDING TRIGGER** |
| controlled target-hardware section Pareto qualification | representative section corpus policy ready | **ACTIVE NEXT — #19** |

A gate must not be marked complete because a nearby test exists. Completion means the stated failure class is actually exercised and the evidence is retained at the appropriate level.
