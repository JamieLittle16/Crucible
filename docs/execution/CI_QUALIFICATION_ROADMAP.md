# Helve CI and qualification roadmap

Status: **normative engineering process**  
Owner: repository-wide  
Motto: **a lower evidence ring never substitutes for a higher one**

Helve treats CI as executable engineering law rather than a collection of convenient checks. A test exists because it excludes a concrete failure class. A heavyweight gate is activated when the subsystem it can meaningfully test exists; it is not added early as ceremony, and it must not remain marked "future" after its trigger has actually been crossed.

## Evidence rings

| Ring | Purpose | Current status |
|---|---|---|
| 0 | repository, toolchain and supply-chain law | **ACTIVE** |
| 1 | cheap structural correctness | **ACTIVE** |
| 2 | subsystem semantic qualification | **ACTIVE — sections, networking/session boundaries, resident-world lifecycle** |
| 3 | official vanilla/source/runtime qualification | **ACTIVE — pre-play/state/section evidence; R2C world-wire admission in progress** |
| 4 | hostile/stress/fuzz/restart/concurrency qualification | **PARTIALLY TRIGGERED; activate per subsystem** |
| 5 | controlled performance qualification | **ACTIVE AS LAB; production decisions require target evidence** |

Passing Ring 1 does not establish vanilla parity. Passing vanilla parity does not establish performance. Performance can never excuse a semantic failure.

The performance evidence state machine is defined normatively in `../qualification/EVIDENCE_AND_EXPERIMENT_RECORDS.md`.

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

The exact allowlist remains intentionally simple while the workspace has no substantial external Rust package graph.

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

Official-data workflows must freshly:

1. probe the pinned official Minecraft runtime/source input where applicable;
2. bind it to the committed source qualification;
3. regenerate qualified target Rust/manifests outside the repository tree;
4. byte-compare outputs against committed generated artifacts.

Any byte difference is a failure, even if both versions compile.

---

## Ring 2 — subsystem semantic qualification

A subsystem receives its own Ring-2 qualifier when either:

- it has more than one mechanism/optimization path;
- a simple independent reference can be maintained;
- it owns gameplay-visible state transitions;
- a bug could be schedule/history/representation dependent;
- a bounded architectural invariant is important enough to protect permanently.

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

### Active: R2C resident-world lifecycle

The dedicated resident-world qualifier protects:

- exact load/discover/resolve/unload identity;
- fresh generation identity after reload;
- fail-closed stale-handle rejection;
- real generated 26.2 block-state integration;
- signed/negative chunk coordinates;
- compact handle/profile structural bounds;
- equivalence between repeated directory routing and resolve-once HOT access;
- complete 1/9/25/81-resident-chunk structural workload coverage.

Hosted timings are diagnostic. Target-baseline admission is separately controlled by `R2C_RESIDENT_WORLD_QUALIFICATION.md`.

### Next likely Ring-2 qualifiers

- pregenerated-world import;
- heightmaps;
- lighting;
- collision;
- chunk projection/encoding;
- persistence;
- entity spatial indexing;
- tick queues.

---

## Ring 3 — vanilla/source/runtime qualification

### Active now

The repository already has source/runtime-backed qualification for material pre-R2C areas including:

- 26.2 state-data generation/binding;
- source-backed world-section fixtures;
- real official-server world generation and Anvil/NBT corpus extraction used by section qualification;
- Login/Configuration/R2B source review and replay-free stock-client validation;
- independent Python/Rust reconstruction where the relevant subsystem requires it.

### R2C world-wire admission

R2C.1 is **not** considered admitted merely because discovery tooling is green. Exact chunk/light/world publication semantics still require the pinned local Mojang source/runtime review, source-free materialization and independent admission gate.

Captures may confirm an already source-backed law but do not substitute for it.

### Whole-server differential laboratory — trigger crossed

The original trigger was the first bootable Helve server capable of completing a scripted client/server scenario. R2B has crossed that trigger: an unmodified 26.2 client can complete replay-free pre-play and enter Play through the live Helve driver.

Therefore the differential laboratory is no longer a hypothetical future gate. Build it progressively as the observable surface becomes meaningful:

- Handshake/Login/Configuration/Play transitions;
- R2B bootstrap/session/liveness outcomes;
- R2C chunk/biome/light contents when native projection exists;
- later movement/collision, block interaction, entities, ticks, inventory/components and save/reload.

The comparison should prefer semantic normalization over copying Mojang implementation internals.

---

## Ring 4 — hostile, fuzz, restart and concurrency qualification

These gates activate when their concrete failure class exists. A crossed trigger is engineering debt until its gate is either implemented or explicitly scoped/deferred.

### Protocol/parser fuzzing — trigger crossed

The trigger is the first production parser/decoder that consumes untrusted network/client input. Helve has crossed this trigger.

Required activation shape:

- short bounded fuzz/regression corpus in PR CI where practical;
- persisted minimized reproducers;
- longer scheduled fuzz jobs once the first fuzz target is admitted;
- every discovered crash/invariant violation becomes a permanent regression.

This should be introduced without blocking R2C source-independent work, but it is no longer correctly classified as "pending trigger".

### NBT/persistence fuzzing

**Trigger:** first production R2C persistence/import reader that consumes untrusted or externally supplied NBT/Anvil data.

R2C.3 should activate malformed-input/property/fuzz coverage as the importer becomes real. Do not wait until save support exists if the reader already consumes complex persisted data.

### Concurrency/model checking

**Trigger:** first shared ownership transfer, cross-region handoff, concurrent chunk install, revision race, scheduler handoff or other gameplay-relevant concurrency primitive.

Required shape:

- deterministic interleaving/model tests where practical;
- race/revision/install-barrier tests;
- Loom or an equivalent model checker where it adds real coverage;
- sanitizer/race-tool lanes where supported and useful;
- no concurrency test may weaken the singular-mutation-authority law.

The initial R2C resident-world substrate deliberately avoids claiming this trigger merely because future regionization will need it.

### Persistence/crash qualification

**Trigger:** first production world persistence writer/save path.

Required cases:

- save -> stop -> reopen -> semantic comparison;
- repeated save/reload cycles;
- interrupted/partial write behavior;
- malformed/corrupt input rejection;
- unknown/new data handling policy;
- revision/save-barrier correctness;
- deterministic byte comparison where byte identity is itself contractual.

### Soak/stress

**Trigger:** first end-to-end server loop with meaningful persistent world/player activity.

R2B liveness testing alone does not close this gate. R2C/R2D will make the trigger materially useful once native world residence/publication and repeated joins exist.

Use deterministic workload scripts, bounded resource assertions, queue/backpressure telemetry and long-running invariant checks. Scheduled/nightly is preferred over slowing every small PR.

---

## Ring 5 — performance qualification

Hosted GitHub runner timings are **diagnostic only** unless a subsystem specification explicitly establishes a stronger policy.

Production performance decisions require controlled hardware evidence containing, as applicable:

- commit SHA;
- Minecraft/data digests;
- harness/workload version;
- Rust/toolchain/codegen identity;
- CPU/core affinity;
- OS/kernel;
- governor/frequency/turbo context;
- warmup/sample counts;
- median plus meaningful tail/spike metrics;
- deterministic owned-byte accounting;
- process/RSS measurements where appropriate;
- independent-process variation/noise context;
- an explicit target-run provenance witness when the subsystem requires one.

### Active controlled-performance work

- **World sections:** issue #19 remains open until representative vanilla-derived weighting, target-hardware CPU/tail measurements, RSS/Pareto analysis and the committed production-selection record exist.
- **R2C resident world:** hosted diagnostic qualification is active; explicit target-run and cross-process tooling exists; the first accepted target-hardware baseline is still pending.

A baseline is not a mechanism decision. Later complexity must still earn itself on whole-cost evidence.

---

## Main-branch governance

`main` is intended to be mechanically protected, not protected by convention.

### Required repository setting

Apply/maintain a GitHub branch protection rule/ruleset to `main` with:

- require a pull request before merge;
- require the ordinary `CI` checks (`Rust and tooling quality` and `Repository guard`) to pass;
- require the branch to be up to date before merge;
- require conversation resolution;
- block force pushes;
- block branch deletion;
- no routine bypass of failing required checks;
- continue using squash merge for clean milestone history.

While Helve has a single maintainer, do **not** require a second human approval merely to manufacture process. Add review-count/CODEOWNERS requirements when additional maintainers are actually participating.

Path-filtered heavyweight workflows should not be made direct globally-required checks unless GitHub can guarantee a neutral/pass result when irrelevant.

### Qualification-gate aggregator — trigger crossed

The original trigger was the second independent path-filtered subsystem qualifier. With the world-section qualification family and dedicated R2C resident-world qualification, that trigger is now crossed.

Before the number of independent qualifiers grows significantly, add a small always-running **qualification-gate aggregator** that determines which subsystem gates apply and reports one stable required result. This avoids branch protection depending directly on many path-filtered workflows that may be skipped when irrelevant.

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

| Gate | Activation trigger | Status |
|---|---|---|
| automated license/advisory dependency audit | first substantial external Rust dependency graph | **PENDING TRIGGER** |
| whole-server Mojang/Helve differential runner | first bootable scripted server scenario | **TRIGGER CROSSED — BUILD PROGRESSIVELY** |
| packet/parser fuzzing | first production untrusted protocol/parser path | **TRIGGER CROSSED — ACTIVATE** |
| NBT/import fuzzing | first production R2C NBT/Anvil reader | **ACTIVATE WITH R2C.3** |
| save/restart/crash qualification | first production persistence writer | **PENDING TRIGGER** |
| concurrency model/race qualification | first gameplay-relevant shared ownership/handoff | **PENDING TRIGGER** |
| deterministic soak/stress suite | first meaningful persistent world/player workload | **PENDING R2C/R2D TRIGGER** |
| scheduled long fuzz jobs | first fuzz target admitted | **PENDING FIRST FUZZ TARGET** |
| qualification-gate aggregator | second independent path-filtered subsystem qualifier | **TRIGGER CROSSED — ACTIVE NEXT** |
| controlled target-hardware section Pareto qualification | representative section corpus policy ready | **ACTIVE — #19** |
| R2C resident-world target baseline | green resident lifecycle qualifier + controlled target runner | **ACTIVE NEXT** |

A gate must not be marked complete because a nearby test exists. Completion means the stated failure class is actually exercised and the evidence is retained at the appropriate level.
