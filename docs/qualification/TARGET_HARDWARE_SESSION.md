# Controlled-Hardware Evidence Session

Status: **QUALIFICATION INFRASTRUCTURE**  
Tracker: #128  
Normative parent: [Performance Qualification Standard](PERFORMANCE_QUALIFICATION_STANDARD.md)

## Purpose

Crucible has several independent performance laboratories. Their benchmark harnesses answer different
questions and retain their own raw samples/statistics. A production qualification session must not
turn those outputs into loose JSON files whose commit, machine, or provenance later becomes unclear.

`tools/target_hardware_session.py` provides one narrow envelope:

```text
one clean repository commit
        ↓
existing full harnesses, unchanged
        ↓
raw JSON artifacts
        ↓
schema + semantic guard validation
        ↓
stable machine/toolchain cross-check
        ↓
SHA-256 each raw artifact
        ↓
canonical session-manifest.json + SHA-256 sidecar
```

The session driver performs **no cross-benchmark scoring and makes no production decision**.

## Initial benchmark set

The v1 session runs, in fixed order:

1. composition HOT dispatch tax (`composition-hot-tax`);
2. resolved world-access routing (`resolved-chunk-window`);
3. 1/2/4-worker + memory baseline (`executor-worker-memory-baseline`);
4. fused outbound construction experiment (`fused-outbound-construction`).

M0.3D section-policy selection deliberately keeps its stronger dedicated final driver because that
workflow additionally consumes sealed correctness and representative-vanilla evidence and performs
a dimension-separated Pareto review. Do not duplicate that logic here.

## Invocation

From a clean checkout of the exact commit to qualify:

```bash
python3 tools/target_hardware_session.py \
  --repo-root . \
  --output-dir target/qualification/m0-hardware-session
```

The output directory must be repository-relative, must not contain `..`, and must not already exist.
Evidence is never overwritten in place.

The driver invokes every existing harness using:

```text
cargo run --release --locked --package <package> --bin <binary> -- --full --output <artifact>
```

The exact argv is retained in the session manifest.

## Machine preparation remains explicit

This tool does not attempt to turn an uncontrolled machine into controlled hardware. Before running,
follow the Performance Qualification Standard:

- stabilize unrelated system load;
- choose and record CPU placement appropriate to the specific harness;
- avoid accidental core/SMT contention;
- preserve intended governor/turbo/NUMA/THP configuration;
- collect PMU evidence where it answers the experiment's hypothesis;
- retain cold/warm/steady/transition distinctions from the underlying harnesses.

Different harnesses may intentionally use different CPU affinity. For example, a single-thread HOT
loop may be pinned to one logical CPU while the executor baseline needs four. Therefore
`cpus_allowed_list`, instantaneous `cpu_current_khz`, and `load_average` remain raw per-artifact
observations and are not required to be identical across the session.

Stable physical/toolchain identity *is* cross-checked, including fields such as CPU model/family,
microcode, cache topology, kernel, governor/range policy, SMT state, memory-node allowance, target
triple, rustc identity and Rust flags.

## Fail-closed rules

The driver refuses to seal a session when:

- the repository is dirty;
- `--repo-root` is not the Git root;
- an output directory already exists;
- a benchmark command fails;
- an artifact is missing, a symlink, malformed, wrong-schema, wrong-benchmark, or not `full` mode;
- an artifact's embedded commit differs from session `HEAD`;
- a benchmark's permanent semantic/equivalence guards are not green;
- stable machine/toolchain identity changes between artifacts.

A failed run may retain partial raw files for diagnosis, but it never writes
`session-manifest.json` or `session-manifest.sha256`. Partial evidence therefore cannot masquerade as
a sealed session.

The CLI also refuses to create an authoritative session when `GITHUB_ACTIONS=true`. Hosted CI tests
the tool using synthetic artifacts; it does not emit production performance evidence.

## Manifest

A successful session contains the raw artifacts plus:

```text
session-manifest.json
session-manifest.sha256
```

The canonical manifest records:

- schema/session kind;
- exact Git commit;
- explicit `decision_made: false`;
- stable hardware/toolchain identity;
- fixed artifact order;
- benchmark identity;
- artifact filename;
- artifact byte length;
- artifact SHA-256;
- exact invoked argv.

The sidecar is the SHA-256 of the exact canonical manifest bytes.

## What this does not prove

A sealed session does not mean every candidate is good, a benchmark result is statistically
significant, or a production architecture is selected. Each subsystem's own qualification record
still decides:

- whether machine preparation was sufficient;
- whether noise/confidence gates passed;
- whether the materiality threshold was crossed;
- whether CPU/tail/memory/startup trade-offs are acceptable;
- which candidate, if any, should be admitted.

The envelope exists to make those decisions auditable and repeatable, not automatic.
