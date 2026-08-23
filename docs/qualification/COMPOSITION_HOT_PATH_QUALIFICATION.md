# Composition HOT-path qualification

**Status:** M0.1B qualification protocol  
**Parent:** issue #75 / M0.1 issue #2

Crucible resolves package/profile generality before HOT execution. The generated composition boundary must therefore be a zero-cost static wiring boundary, not a runtime service locator.

The governing requirement is:

> **Package modularity may choose the concrete type before execution; the inner loop must see the concrete type directly.**

## Structural gate

For the current `reference` profile, the generated composition contains:

```rust
pub use crucible_world_reference::DirectBlockSection as SectionStore;
```

`crucible-composition-qualification` permanently checks that:

- `crucible_composition::SectionStore<BlockStateId>` and the direct hand-wired `DirectBlockSection<BlockStateId>` have the exact same `TypeId`;
- their size and alignment are identical;
- a value named through the generated provider is directly accepted where the hand-wired concrete type is required;
- the resolver regression suite continues to reject generated executable wiring containing `dyn`, `HashMap`, or a service lookup.

A future generator change that introduces wrapper state or runtime dispatch must fail this gate before performance evidence is considered.

## HOT-loop regression experiment

The release benchmark compares two entry points over one shared section object:

1. `hand_wired_get(&DirectBlockSection<BlockStateId>, pos)`;
2. `generated_get(&SectionStore<BlockStateId>, pos)`.

The fixture populates all 4096 cells with deterministic valid target-version block states. A deterministic irregular position stream is generated outside the timed region. Both paths execute the same operation count over the same semantic object and must produce the same checksum before timing.

Measured rounds alternate which path runs first. Warm-up rounds are retained as protocol settings. Raw paired round timings remain in the JSON evidence artifact.

## Hosted CI

Hosted CI runs only a release-mode smoke:

```bash
cargo run --release --locked \
  --package crucible-composition-qualification \
  --bin composition_hot_bench -- \
  --smoke \
  --output target/composition-hot-smoke.json
```

This proves the harness compiles, executes the intended work, retains structural identity and emits parseable evidence. GitHub-hosted timing **cannot** admit or reject the architecture.

## Controlled target-hardware run

Pin the process to one intended logical CPU and preserve the resulting artifact, for example:

```bash
taskset -c <cpu> cargo run --release --locked \
  --package crucible-composition-qualification \
  --bin composition_hot_bench -- \
  --full \
  --require-single-cpu \
  --output artifacts/composition-hot-tax.json
```

The artifact records:

- exact Git commit;
- composition SHA;
- profile and Minecraft version;
- generated state-data input/generation digests;
- Rust compiler/target/flags;
- CPU model, topology, affinity, SMT, cache information, governor/frequency/turbo state and other available machine provenance;
- operation/warm-up/sample counts;
- semantic checksum;
- raw paired timings and p50/p95/p99/max summaries.

Apply `PERFORMANCE_QUALIFICATION_STANDARD.md` to machine stabilization, candidate ordering, frequency/thermal interpretation and noise.

## Decision rule

The structural gate is primary: with an exact concrete re-export there is no architectural dispatch layer to charge to the HOT operation.

The benchmark is a permanent regression guard. Any reproducible material generated-path slowdown on controlled target hardware is treated as a **composition design failure**, not an acceptable modularity tax. Investigate generated code/codegen and remove the cause rather than relaxing the requirement.

Do not use this experiment to select the future production section representation. Balanced/performance/memory profiles remain unresolved until the section Pareto decision in #19.
