#!/usr/bin/env python3
"""Orchestrate candidate-isolated section benchmarks on controlled target hardware."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

SCHEMA = 1
KIND = "section-target-hardware-orchestration"
ARTIFACT_SCHEMA = 1
ARTIFACT_KIND = "section-target-hardware-artifact"
PACK_SCHEMA = 1
PACK_KIND = "section-target-benchmark-pack-set"
REPRESENTATIVE_POLICY = "vanilla-section-representative-v1"
CHILD_SCHEMA = 1
CHILD_VERSION = "section-population-bench-v1"
RUST_TOOLCHAIN = "1.97.1"
RUSTC_COMMIT = "8bab26f4f68e0e26f0bb7960be334d5b520ea452"
BUILD_PROFILE = "release"
CODEGEN_POLICY = "lto=thin,codegen-units=1,panic=abort,strip=debuginfo"
RSS_PROTOCOL = "candidate-delta-after-explicit-prefaulted-common-scratch"
DECISION_SCOPE = "dimension-separated-only"
CANDIDATES = (
    "direct-reference",
    "direct",
    "adaptive",
    "fast-local",
    "packed-local",
)
PRODUCTION_CANDIDATES = CANDIDATES[1:]
DIMENSIONS = (
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
)
WORKLOADS = (
    "random-read",
    "sequential-full-read",
    "small-volume-read",
    "maybe-contains-positive",
    "maybe-contains-negative",
    "control-integer-loop",
)
STEADY_WORKLOADS = WORKLOADS[:-1]
CONTROL_MAX_RELATIVE_MAD_PPM = 50_000
WORKLOAD_MAX_RELATIVE_MAD_PPM = 100_000
RSS_MAX_RELATIVE_MAD_PPM = 100_000
OUTLIER_GUARD_MULTIPLIER = 3
CONTROL_MAX_RELATIVE_DEVIATION_PPM = CONTROL_MAX_RELATIVE_MAD_PPM * OUTLIER_GUARD_MULTIPLIER
WORKLOAD_MAX_RELATIVE_DEVIATION_PPM = WORKLOAD_MAX_RELATIVE_MAD_PPM * OUTLIER_GUARD_MULTIPLIER
RSS_MAX_RELATIVE_DEVIATION_PPM = RSS_MAX_RELATIVE_MAD_PPM * OUTLIER_GUARD_MULTIPLIER
LOWER_SHA256 = re.compile(r"[0-9a-f]{64}\Z")


class QualificationError(RuntimeError):
    """Raised when evidence cannot safely qualify."""


@dataclass(frozen=True)
class PackEntry:
    dimension: str
    path: Path
    section_count: int
    size: int
    sha256: str


@dataclass(frozen=True)
class PackSet:
    manifest_path: Path
    manifest_sha256: str
    policy: str
    population_sha256: str
    admission_sha256: str
    source_artifact_manifest_sha256: str
    target: dict[str, object]
    entries: dict[str, PackEntry]


@dataclass(frozen=True)
class BuildIdentity:
    head_sha: str
    executable: Path
    executable_sha256: str
    rustc_verbose: str
    cargo_version: str
    cargo_toml_sha256: str
    cargo_lock_sha256: str
    cargo_config_sha256: str
    parent_cargo_configs: tuple[dict[str, str], ...]


@dataclass(frozen=True)
class ScheduledChild:
    round_index: int
    dimension_position: int
    candidate_position: int
    dimension: str
    candidate: str


def canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise QualificationError(f"{path} must contain a JSON object")
    return value


def _sha(value: object, label: str) -> str:
    if not isinstance(value, str) or LOWER_SHA256.fullmatch(value) is None:
        raise QualificationError(f"{label} must be canonical lowercase SHA-256")
    return value


def _integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise QualificationError(f"{label} must be an integer")
    return value


def _safe_basename(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise QualificationError(f"{label} must be a non-empty path")
    path = Path(value)
    if path.is_absolute() or len(path.parts) != 1 or path.name != value:
        raise QualificationError(f"{label} must be a safe basename")
    return value


def _verify_digest_field(record: dict[str, Any], field: str, label: str) -> str:
    expected = _sha(record.get(field), f"{label}.{field}")
    payload = dict(record)
    payload.pop(field)
    actual = canonical_digest(payload)
    if actual != expected:
        raise QualificationError(
            f"{label} digest mismatch: expected {expected}, got {actual}"
        )
    return expected


def _target_contract(repo_root: Path) -> dict[str, object]:
    manifest = _load_json(repo_root / "vanilla/state-data/26.2-state-data-manifest.json")
    target = manifest.get("target")
    if not isinstance(target, dict):
        raise QualificationError("state-data target must be an object")
    return {
        "minecraft_version": target.get("minecraft_version"),
        "protocol_version": target.get("protocol_version"),
        "data_version": target.get("data_version"),
        "state_count": manifest.get("state_count"),
        "state_data_generation_sha256": manifest.get("generation_digest"),
        "state_data_input_sha256": manifest.get("input_digest"),
    }


def verify_pack_set(pack_root: Path, repo_root: Path) -> PackSet:
    manifest_path = pack_root / "pack-manifest.json"
    manifest = _load_json(manifest_path)
    if manifest.get("schema") != PACK_SCHEMA or manifest.get("kind") != PACK_KIND:
        raise QualificationError("benchmark pack manifest schema/kind mismatch")
    manifest_sha = _verify_digest_field(manifest, "manifest_sha256", "pack manifest")
    policy = manifest.get("policy")
    if policy != REPRESENTATIVE_POLICY:
        raise QualificationError(
            f"unsupported representative policy: expected {REPRESENTATIVE_POLICY}, got {policy!r}"
        )
    if manifest.get("decision_scope") != DECISION_SCOPE:
        raise QualificationError("benchmark pack decision scope drifted")
    if manifest.get("cross_dimension_score_allowed") is not False:
        raise QualificationError("benchmark pack illegally enables cross-dimension scoring")

    target = manifest.get("target")
    if not isinstance(target, dict) or target != _target_contract(repo_root):
        raise QualificationError("benchmark pack target identity does not match repository target")
    population_sha = _sha(manifest.get("population_sha256"), "population_sha256")
    admission_sha = _sha(manifest.get("admission_sha256"), "admission_sha256")
    source_artifact_sha = _sha(
        manifest.get("source_artifact_manifest_sha256"),
        "source_artifact_manifest_sha256",
    )

    members = manifest.get("members")
    if not isinstance(members, list) or len(members) != 4:
        raise QualificationError("benchmark pack manifest must retain exactly four members")
    seen_seeds: set[int] = set()
    seen_corpora: set[str] = set()
    member_dimension_totals = {dimension: 0 for dimension in DIMENSIONS}
    for index, raw_member in enumerate(members):
        if not isinstance(raw_member, dict):
            raise QualificationError(f"pack member {index} must be an object")
        seed_index = _integer(raw_member.get("seed_index"), f"member[{index}].seed_index")
        corpus_sha = _sha(raw_member.get("corpus_sha256"), f"member[{index}].corpus_sha256")
        if seed_index in seen_seeds or corpus_sha in seen_corpora:
            raise QualificationError("pack members must have distinct seed/corpus identities")
        seen_seeds.add(seed_index)
        seen_corpora.add(corpus_sha)
        per_dimension = raw_member.get("per_dimension_sections")
        if not isinstance(per_dimension, dict) or set(per_dimension) != set(DIMENSIONS):
            raise QualificationError("pack member dimension set drifted")
        for dimension in DIMENSIONS:
            count = _integer(per_dimension.get(dimension), f"member[{index}] {dimension}")
            if count < 0:
                raise QualificationError("member section count cannot be negative")
            member_dimension_totals[dimension] += count
    if seen_seeds != {0, 1, 2, 3}:
        raise QualificationError(f"pack seed indices must be exactly 0..3; got {seen_seeds}")

    raw_packs = manifest.get("packs")
    if not isinstance(raw_packs, dict) or set(raw_packs) != set(DIMENSIONS):
        raise QualificationError("pack manifest must contain exactly the three standard dimensions")
    entries: dict[str, PackEntry] = {}
    for dimension in DIMENSIONS:
        raw = raw_packs[dimension]
        if not isinstance(raw, dict):
            raise QualificationError(f"pack entry {dimension} must be an object")
        name = _safe_basename(raw.get("path"), f"{dimension}.path")
        path = pack_root / name
        if not path.is_file():
            raise QualificationError(f"pack file is missing: {path}")
        section_count = _integer(raw.get("section_count"), f"{dimension}.section_count")
        total_cells = _integer(raw.get("total_cells"), f"{dimension}.total_cells")
        size = _integer(raw.get("size"), f"{dimension}.size")
        expected_sha = _sha(raw.get("sha256"), f"{dimension}.sha256")
        if section_count <= 0 or total_cells != section_count * 4096:
            raise QualificationError(f"invalid section/cell count for {dimension}")
        if member_dimension_totals[dimension] != section_count:
            raise QualificationError(f"member/pack section count mismatch for {dimension}")
        if path.stat().st_size != size:
            raise QualificationError(f"pack size mismatch for {dimension}")
        actual_sha = sha256_file(path)
        if actual_sha != expected_sha:
            raise QualificationError(
                f"pack SHA mismatch for {dimension}: expected {expected_sha}, got {actual_sha}"
            )
        entries[dimension] = PackEntry(
            dimension=dimension,
            path=path,
            section_count=section_count,
            size=size,
            sha256=expected_sha,
        )
    return PackSet(
        manifest_path=manifest_path,
        manifest_sha256=manifest_sha,
        policy=policy,
        population_sha256=population_sha,
        admission_sha256=admission_sha,
        source_artifact_manifest_sha256=source_artifact_sha,
        target=dict(target),
        entries=entries,
    )


def _run_text(args: list[str], *, cwd: Path, env: dict[str, str] | None = None) -> str:
    result = subprocess.run(
        args,
        cwd=cwd,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        raise QualificationError(
            f"command failed ({' '.join(args)}):\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result.stdout.strip()


def repository_identity(repo_root: Path) -> str:
    top = Path(_run_text(["git", "rev-parse", "--show-toplevel"], cwd=repo_root)).resolve()
    if top != repo_root.resolve():
        raise QualificationError(f"repo root mismatch: expected {repo_root.resolve()}, got {top}")
    status = _run_text(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"], cwd=repo_root
    )
    if status:
        raise QualificationError("target-hardware qualification requires a completely clean checkout")
    return _run_text(["git", "rev-parse", "HEAD"], cwd=repo_root)


def _is_within(path: Path, root: Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
    except ValueError:
        return False
    return True


def require_external_evidence_paths(repo_root: Path, pack_root: Path, output_dir: Path) -> None:
    if _is_within(pack_root, repo_root):
        raise QualificationError("pack root must live outside the repository for a clean qualification run")
    if _is_within(output_dir, repo_root):
        raise QualificationError("qualification output must live outside the repository")
    if output_dir.exists() and any(output_dir.iterdir()):
        raise QualificationError("qualification output directory must be absent or empty")


def forbidden_environment(environment: dict[str, str]) -> list[str]:
    forbidden_exact = {
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTC_BOOTSTRAP",
        "CARGO_BUILD_TARGET",
        "CARGO_BUILD_RUSTFLAGS",
    }
    found: list[str] = []
    for key, value in environment.items():
        if not value:
            continue
        if key in forbidden_exact:
            found.append(key)
        elif key.startswith("CARGO_PROFILE_RELEASE_"):
            found.append(key)
        elif key.startswith("CARGO_TARGET_") and key.endswith("_RUSTFLAGS"):
            found.append(key)
    return sorted(found)


def _parent_cargo_configs(repo_root: Path) -> tuple[dict[str, str], ...]:
    records: list[dict[str, str]] = []
    for ancestor in repo_root.resolve().parents:
        for name in ("config.toml", "config"):
            path = ancestor / ".cargo" / name
            if not path.is_file():
                continue
            try:
                parsed = tomllib.loads(path.read_text(encoding="utf-8"))
            except (OSError, tomllib.TOMLDecodeError) as error:
                raise QualificationError(f"could not safely inspect parent Cargo config {path}: {error}") from error
            _reject_semantic_parent_cargo_config(path, parsed)
            records.append({"path": str(path), "sha256": sha256_file(path)})
    return tuple(records)


def _reject_semantic_parent_cargo_config(path: Path, parsed: dict[str, Any]) -> None:
    build = parsed.get("build")
    if isinstance(build, dict):
        forbidden = {"rustflags", "rustc", "rustc-wrapper", "target", "incremental"}
        found = forbidden.intersection(build)
        if found:
            raise QualificationError(
                f"parent Cargo config {path} contains build-affecting keys: {sorted(found)}"
            )
    target_table = parsed.get("target")
    if isinstance(target_table, dict):
        for target_name, settings in target_table.items():
            if isinstance(settings, dict):
                found = {"rustflags", "linker"}.intersection(settings)
                if found:
                    raise QualificationError(
                        f"parent Cargo config {path} target {target_name} affects binary generation: {sorted(found)}"
                    )
    env_table = parsed.get("env")
    if isinstance(env_table, dict) and env_table:
        raise QualificationError(
            f"parent Cargo config {path} injects environment variables; qualification requires explicit environment"
        )


def verify_toolchain(repo_root: Path) -> tuple[str, str]:
    rustc = _run_text(["rustc", f"+{RUST_TOOLCHAIN}", "--version", "--verbose"], cwd=repo_root)
    fields = dict(line.split(": ", 1) for line in rustc.splitlines() if ": " in line)
    if fields.get("release") != RUST_TOOLCHAIN or fields.get("commit-hash") != RUSTC_COMMIT:
        raise QualificationError("pinned rustc identity mismatch")
    cargo = _run_text(["cargo", f"+{RUST_TOOLCHAIN}", "--version"], cwd=repo_root)
    if not cargo.startswith(f"cargo {RUST_TOOLCHAIN} "):
        raise QualificationError(f"pinned cargo identity mismatch: {cargo}")
    return rustc, cargo


def controlled_environment(scratch_dir: Path) -> dict[str, str]:
    blocked = forbidden_environment(dict(os.environ))
    if blocked:
        raise QualificationError(
            "qualification refuses compiler/profile overrides: " + ", ".join(blocked)
        )
    environment = dict(os.environ)
    for key in list(environment):
        if key.startswith("CARGO_PROFILE_RELEASE_"):
            environment.pop(key, None)
        if key.startswith("CARGO_TARGET_") and key.endswith("_RUSTFLAGS"):
            environment.pop(key, None)
    for key in (
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTC_BOOTSTRAP",
        "CARGO_BUILD_TARGET",
        "CARGO_BUILD_RUSTFLAGS",
    ):
        environment.pop(key, None)
    cargo_home = scratch_dir / "cargo-home"
    target_dir = scratch_dir / "target"
    cargo_home.mkdir(parents=True, exist_ok=True)
    target_dir.mkdir(parents=True, exist_ok=True)
    environment["CARGO_HOME"] = str(cargo_home)
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    environment["CARGO_INCREMENTAL"] = "0"
    environment["RUSTFLAGS"] = ""
    environment["CARGO_ENCODED_RUSTFLAGS"] = ""
    return environment


def _controlled_build(
    repo_root: Path,
    output_dir: Path,
    scratch_dir: Path,
    head_sha: str,
    environment: dict[str, str],
) -> BuildIdentity:
    rustc, cargo = verify_toolchain(repo_root)
    parent_configs = _parent_cargo_configs(repo_root)
    result = subprocess.run(
        [
            "cargo",
            f"+{RUST_TOOLCHAIN}",
            "build",
            "--offline",
            "--release",
            "--locked",
            "-p",
            "crucible-section-qualification",
            "--bin",
            "section_bench",
        ],
        cwd=repo_root,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    (output_dir / "build.stdout").write_text(result.stdout, encoding="utf-8")
    (output_dir / "build.stderr").write_text(result.stderr, encoding="utf-8")
    if result.returncode != 0:
        raise QualificationError("controlled offline benchmark build failed; see build logs")
    built = scratch_dir / "target" / "release" / "section_bench"
    if not built.is_file():
        raise QualificationError("controlled build did not produce section_bench")
    if repository_identity(repo_root) != head_sha:
        raise QualificationError("repository changed during controlled build")
    evidence_binary = output_dir / "benchmark-executable"
    shutil.copy2(built, evidence_binary)
    if not evidence_binary.is_file():
        raise QualificationError("could not retain exact benchmark executable")
    config = repo_root / ".cargo/config.toml"
    return BuildIdentity(
        head_sha=head_sha,
        executable=evidence_binary,
        executable_sha256=sha256_file(evidence_binary),
        rustc_verbose=rustc,
        cargo_version=cargo,
        cargo_toml_sha256=sha256_file(repo_root / "Cargo.toml"),
        cargo_lock_sha256=sha256_file(repo_root / "Cargo.lock"),
        cargo_config_sha256=sha256_file(config),
        parent_cargo_configs=parent_configs,
    )


def _read_trimmed(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8").strip()
    except OSError:
        return "unknown"


def _status_field(name: str) -> str:
    try:
        lines = Path("/proc/self/status").read_text(encoding="utf-8").splitlines()
    except OSError:
        return "unknown"
    prefix = f"{name}:"
    for line in lines:
        if line.startswith(prefix):
            return line[len(prefix) :].strip()
    return "unknown"


def allowed_cpus() -> set[int]:
    if not hasattr(os, "sched_getaffinity"):
        raise QualificationError("target-hardware qualification requires Linux CPU affinity support")
    return set(os.sched_getaffinity(0))


def _cpu_numa_node(root: Path) -> str:
    nodes = sorted(path.name for path in root.glob("node[0-9]*") if path.is_dir())
    return ",".join(nodes) if nodes else "unknown"


def cpu_topology(cpu: int) -> dict[str, str]:
    root = Path(f"/sys/devices/system/cpu/cpu{cpu}")
    return {
        "physical_package_id": _read_trimmed(root / "topology/physical_package_id"),
        "core_id": _read_trimmed(root / "topology/core_id"),
        "thread_siblings_list": _read_trimmed(root / "topology/thread_siblings_list"),
        "numa_node": _cpu_numa_node(root),
    }


def environment_snapshot(cpu: int) -> dict[str, object]:
    cpufreq = Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq")
    thermal = {
        zone.name: _read_trimmed(zone / "temp")
        for zone in sorted(Path("/sys/class/thermal").glob("thermal_zone*"))
    }
    return {
        "monotonic_ns": time.monotonic_ns(),
        "loadavg": _read_trimmed(Path("/proc/loadavg")),
        "mems_allowed_list": _status_field("Mems_allowed_list"),
        "cpu_governor": _read_trimmed(cpufreq / "scaling_governor"),
        "cpu_current_khz": _read_trimmed(cpufreq / "scaling_cur_freq"),
        "cpu_min_khz": _read_trimmed(cpufreq / "scaling_min_freq"),
        "cpu_max_khz": _read_trimmed(cpufreq / "scaling_max_freq"),
        "intel_pstate_no_turbo": _read_trimmed(
            Path("/sys/devices/system/cpu/intel_pstate/no_turbo")
        ),
        "thermal_millicelsius": thermal,
    }


def rotate(values: tuple[str, ...], offset: int) -> tuple[str, ...]:
    if not values:
        return values
    offset %= len(values)
    return values[offset:] + values[:offset]


def schedule(rounds: int) -> list[ScheduledChild]:
    if rounds <= 0:
        raise QualificationError("round count must be positive")
    result: list[ScheduledChild] = []
    for round_index in range(rounds):
        for dimension_position, dimension in enumerate(rotate(DIMENSIONS, round_index)):
            ordinal = DIMENSIONS.index(dimension)
            for candidate_position, candidate in enumerate(
                rotate(CANDIDATES, round_index + ordinal)
            ):
                result.append(
                    ScheduledChild(
                        round_index=round_index,
                        dimension_position=dimension_position,
                        candidate_position=candidate_position,
                        dimension=dimension,
                        candidate=candidate,
                    )
                )
    return result


def _validate_summary(summary: object, expected_samples: int, label: str) -> None:
    if not isinstance(summary, dict):
        raise QualificationError(f"{label} must be an object")
    samples = summary.get("samples_ns")
    if not isinstance(samples, list) or len(samples) != expected_samples:
        raise QualificationError(f"{label} sample count mismatch")
    if any(isinstance(value, bool) or not isinstance(value, int) or value < 0 for value in samples):
        raise QualificationError(f"{label} contains invalid timing samples")
    operations = _integer(summary.get("operations_per_sample"), f"{label}.operations_per_sample")
    if operations <= 0:
        raise QualificationError(f"{label} operation count must be positive")
    for field in ("p50_ns", "p95_ns", "p99_ns", "max_ns", "p50_ps_per_op"):
        if _integer(summary.get(field), f"{label}.{field}") < 0:
            raise QualificationError(f"{label}.{field} cannot be negative")


def validate_child_record(
    record: dict[str, Any],
    *,
    scheduled: ScheduledChild,
    pack_set: PackSet,
    head_sha: str,
    cpu: int,
    smoke: bool,
) -> None:
    if record.get("schema") != CHILD_SCHEMA or record.get("harness_version") != CHILD_VERSION:
        raise QualificationError("child report schema/version mismatch")
    expected_mode = "smoke" if smoke else "qualification"
    if record.get("mode") != expected_mode or record.get("candidate") != scheduled.candidate:
        raise QualificationError("child mode/candidate identity mismatch")
    if record.get("production_candidate") is not (scheduled.candidate != "direct-reference"):
        raise QualificationError("child production-candidate flag mismatch")
    if record.get("build_profile") != BUILD_PROFILE or record.get("codegen_policy") != CODEGEN_POLICY:
        raise QualificationError("child build/codegen provenance mismatch")
    if record.get("commit_sha") != head_sha:
        raise QualificationError("child source commit does not match controlled build")
    if record.get("rustflags") not in ("", None) or record.get("cargo_encoded_rustflags") not in ("", None):
        raise QualificationError("child observed unexpected Rust flags")
    if record.get("cpus_allowed_list") != str(cpu):
        raise QualificationError(
            f"child escaped required CPU affinity: {record.get('cpus_allowed_list')!r}"
        )
    if not isinstance(record.get("mems_allowed_list"), str) or not record["mems_allowed_list"]:
        raise QualificationError("child memory-node affinity provenance is missing")
    for key, expected in pack_set.target.items():
        if record.get(key) != expected:
            raise QualificationError(f"child target identity mismatch at {key}")
    if record.get("population_sha256") != pack_set.population_sha256:
        raise QualificationError("child population identity mismatch")
    if record.get("admission_sha256") != pack_set.admission_sha256:
        raise QualificationError("child admission identity mismatch")
    if record.get("dimension") != scheduled.dimension:
        raise QualificationError("child dimension identity mismatch")
    entry = pack_set.entries[scheduled.dimension]
    if record.get("section_count") != entry.section_count:
        raise QualificationError("child section-count identity mismatch")

    memory = record.get("memory")
    if not isinstance(memory, dict) or memory.get("rss_protocol") != RSS_PROTOCOL:
        raise QualificationError("child RSS protocol mismatch")
    baseline = _integer(memory.get("rss_baseline_kib"), "rss_baseline_kib")
    loaded = _integer(memory.get("rss_loaded_kib"), "rss_loaded_kib")
    delta = _integer(memory.get("rss_loaded_delta_kib"), "rss_loaded_delta_kib")
    if delta != loaded - baseline:
        raise QualificationError("child RSS signed-delta arithmetic mismatch")
    representations = record.get("representations")
    if not isinstance(representations, dict) or sum(
        _integer(value, "representation count") for value in representations.values()
    ) != entry.section_count:
        raise QualificationError("child representation counts do not reconstruct section count")
    _validate_summary(record.get("construction"), entry.section_count, "construction")

    timings = record.get("timings")
    if not isinstance(timings, list):
        raise QualificationError("child timings must be an array")
    by_name: dict[str, dict[str, Any]] = {}
    expected_samples = 3 if smoke else 21
    for raw in timings:
        if not isinstance(raw, dict) or not isinstance(raw.get("workload"), str):
            raise QualificationError("child timing entry malformed")
        workload = raw["workload"]
        if workload in by_name:
            raise QualificationError(f"duplicate child workload: {workload}")
        by_name[workload] = raw
        _validate_summary(raw.get("timing"), expected_samples, f"timing {workload}")
    if set(by_name) != set(WORKLOADS):
        raise QualificationError(f"child workload set mismatch: {set(by_name)}")


def _runtime_environment(build_environment: dict[str, str]) -> dict[str, str]:
    environment = dict(build_environment)
    environment["RUSTFLAGS"] = ""
    environment["CARGO_ENCODED_RUSTFLAGS"] = ""
    return environment


def run_child(
    *,
    scheduled: ScheduledChild,
    pack_set: PackSet,
    build: BuildIdentity,
    repo_root: Path,
    output_dir: Path,
    build_environment: dict[str, str],
    cpu: int,
    smoke: bool,
    timeout_seconds: int,
) -> dict[str, object]:
    entry = pack_set.entries[scheduled.dimension]
    pack_sha_before = sha256_file(entry.path)
    binary_sha_before = sha256_file(build.executable)
    if pack_sha_before != entry.sha256:
        raise QualificationError("pack changed immediately before child launch")
    if binary_sha_before != build.executable_sha256:
        raise QualificationError("benchmark executable changed immediately before child launch")

    child_dir = output_dir / "children" / f"round-{scheduled.round_index:02d}"
    child_dir.mkdir(parents=True, exist_ok=True)
    stem = (
        f"{scheduled.dimension.replace(':', '_')}-"
        f"{scheduled.candidate}-p{scheduled.candidate_position}"
    )
    json_path = child_dir / f"{stem}.json"
    stdout_path = child_dir / f"{stem}.stdout"
    stderr_path = child_dir / f"{stem}.stderr"
    if any(path.exists() for path in (json_path, stdout_path, stderr_path)):
        raise QualificationError("child evidence path already exists")

    before = environment_snapshot(cpu)
    command = [
        "taskset",
        "-c",
        str(cpu),
        str(build.executable),
        "--population-pack",
        str(entry.path),
        "--candidate",
        scheduled.candidate,
        "--population-smoke" if smoke else "--population-qualification",
        "--output",
        str(json_path),
    ]
    start = time.monotonic_ns()
    result = subprocess.run(
        command,
        cwd=repo_root,
        env=_runtime_environment(build_environment),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout_seconds,
        check=False,
    )
    elapsed_ns = time.monotonic_ns() - start
    after = environment_snapshot(cpu)
    stdout_path.write_text(result.stdout, encoding="utf-8")
    stderr_path.write_text(result.stderr, encoding="utf-8")
    if result.returncode != 0:
        raise QualificationError(f"benchmark child failed: {' '.join(command)}")
    if not json_path.is_file():
        raise QualificationError("successful benchmark child did not emit evidence JSON")

    pack_sha_after = sha256_file(entry.path)
    binary_sha_after = sha256_file(build.executable)
    if pack_sha_after != entry.sha256 or pack_sha_after != pack_sha_before:
        raise QualificationError("pack changed during child execution")
    if binary_sha_after != build.executable_sha256 or binary_sha_after != binary_sha_before:
        raise QualificationError("benchmark executable changed during child execution")
    record = _load_json(json_path)
    validate_child_record(
        record,
        scheduled=scheduled,
        pack_set=pack_set,
        head_sha=build.head_sha,
        cpu=cpu,
        smoke=smoke,
    )
    return {
        "round": scheduled.round_index,
        "dimension_position": scheduled.dimension_position,
        "candidate_position": scheduled.candidate_position,
        "dimension": scheduled.dimension,
        "candidate": scheduled.candidate,
        "pack_sha256": entry.sha256,
        "benchmark_executable_sha256": build.executable_sha256,
        "child_evidence_path": json_path.relative_to(output_dir).as_posix(),
        "child_evidence_sha256": sha256_file(json_path),
        "stdout_sha256": sha256_file(stdout_path),
        "stderr_sha256": sha256_file(stderr_path),
        "elapsed_ns": elapsed_ns,
        "environment_before": before,
        "environment_after": after,
        "rss_loaded_delta_kib": record["memory"]["rss_loaded_delta_kib"],
        "construction_p99_ns": record["construction"]["p99_ns"],
        "timing_p50_ps_per_op": {
            timing["workload"]: timing["timing"]["p50_ps_per_op"]
            for timing in record["timings"]
        },
    }


def median_int(values: Iterable[int]) -> int:
    ordered = sorted(values)
    if not ordered:
        raise QualificationError("cannot summarize an empty sample set")
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) // 2


def aggregate_int(values: list[int]) -> dict[str, int]:
    median = median_int(values)
    deviations = [abs(value - median) for value in values]
    mad = median_int(deviations)
    max_deviation = max(deviations)
    scale = max(abs(median), 1)
    return {
        "count": len(values),
        "median": median,
        "mad": mad,
        "relative_mad_ppm": mad * 1_000_000 // scale,
        "max_deviation": max_deviation,
        "max_relative_deviation_ppm": max_deviation * 1_000_000 // scale,
        "min": min(values),
        "max": max(values),
    }


def aggregate_children(children: list[dict[str, object]]) -> dict[str, object]:
    dimensions: dict[str, object] = {}
    for dimension in DIMENSIONS:
        candidates: dict[str, object] = {}
        for candidate in CANDIDATES:
            selected = [
                child
                for child in children
                if child["dimension"] == dimension and child["candidate"] == candidate
            ]
            workloads = {
                workload: aggregate_int(
                    [
                        int(child["timing_p50_ps_per_op"][workload])  # type: ignore[index]
                        for child in selected
                    ]
                )
                for workload in WORKLOADS
            }
            candidates[candidate] = {
                "workloads_p50_ps_per_op": workloads,
                "rss_loaded_delta_kib": aggregate_int(
                    [int(child["rss_loaded_delta_kib"]) for child in selected]
                ),
                "construction_p99_ns": aggregate_int(
                    [int(child["construction_p99_ns"]) for child in selected]
                ),
            }
        dimensions[dimension] = {"candidates": candidates}
    controls = [
        int(child["timing_p50_ps_per_op"]["control-integer-loop"])  # type: ignore[index]
        for child in children
    ]
    return {
        "dimensions": dimensions,
        "global_control_p50_ps_per_op": aggregate_int(controls),
    }


def _noise_ok(summary: dict[str, object], mad_limit: int, deviation_limit: int) -> tuple[bool, bool]:
    mad_ok = int(summary["relative_mad_ppm"]) <= mad_limit
    deviation_ok = int(summary["max_relative_deviation_ppm"]) <= deviation_limit
    return mad_ok, deviation_ok


def classify_noise(aggregates: dict[str, object], *, smoke: bool, rounds: int) -> dict[str, object]:
    reasons: list[str] = []
    protocol_eligible = not smoke and rounds >= 5 and rounds % 5 == 0
    if not protocol_eligible:
        reasons.append("population qualification requires at least five rounds and a multiple of five")

    control = dict(aggregates["global_control_p50_ps_per_op"])  # type: ignore[arg-type]
    control_mad_ok, control_deviation_ok = _noise_ok(
        control,
        CONTROL_MAX_RELATIVE_MAD_PPM,
        CONTROL_MAX_RELATIVE_DEVIATION_PPM,
    )
    control_ok = control_mad_ok and control_deviation_ok
    if not control_mad_ok:
        reasons.append("candidate-independent control workload exceeded MAD noise threshold")
    if not control_deviation_ok:
        reasons.append("candidate-independent control workload exceeded isolated-excursion threshold")

    workload_ok = True
    rss_ok = True
    dimensions = aggregates["dimensions"]  # type: ignore[index]
    for dimension in DIMENSIONS:
        for candidate in PRODUCTION_CANDIDATES:
            candidate_data = dimensions[dimension]["candidates"][candidate]  # type: ignore[index]
            for workload in STEADY_WORKLOADS:
                summary = dict(candidate_data["workloads_p50_ps_per_op"][workload])
                mad_ok, deviation_ok = _noise_ok(
                    summary,
                    WORKLOAD_MAX_RELATIVE_MAD_PPM,
                    WORKLOAD_MAX_RELATIVE_DEVIATION_PPM,
                )
                if not mad_ok:
                    workload_ok = False
                    reasons.append(
                        f"timing MAD noise exceeded threshold: {dimension}/{candidate}/{workload}"
                    )
                if not deviation_ok:
                    workload_ok = False
                    reasons.append(
                        f"timing isolated excursion exceeded threshold: {dimension}/{candidate}/{workload}"
                    )
            rss = dict(candidate_data["rss_loaded_delta_kib"])
            if int(rss["median"]) <= 0:
                rss_ok = False
                reasons.append(f"RSS evidence nonpositive: {dimension}/{candidate}")
                continue
            mad_ok, deviation_ok = _noise_ok(
                rss,
                RSS_MAX_RELATIVE_MAD_PPM,
                RSS_MAX_RELATIVE_DEVIATION_PPM,
            )
            if not mad_ok:
                rss_ok = False
                reasons.append(f"RSS MAD noise exceeded threshold: {dimension}/{candidate}")
            if not deviation_ok:
                rss_ok = False
                reasons.append(
                    f"RSS isolated excursion exceeded threshold: {dimension}/{candidate}"
                )

    population_eligible = protocol_eligible and control_ok and workload_ok and rss_ok
    return {
        "protocol_eligible": protocol_eligible,
        "control_noise_eligible": control_ok,
        "workload_noise_eligible": workload_ok,
        "rss_noise_eligible": rss_ok,
        "population_evidence_eligible": population_eligible,
        "thresholds_ppm": {
            "control_relative_mad": CONTROL_MAX_RELATIVE_MAD_PPM,
            "control_max_relative_deviation": CONTROL_MAX_RELATIVE_DEVIATION_PPM,
            "workload_relative_mad": WORKLOAD_MAX_RELATIVE_MAD_PPM,
            "workload_max_relative_deviation": WORKLOAD_MAX_RELATIVE_DEVIATION_PPM,
            "rss_relative_mad": RSS_MAX_RELATIVE_MAD_PPM,
            "rss_max_relative_deviation": RSS_MAX_RELATIVE_DEVIATION_PPM,
        },
        "reasons": sorted(set(reasons)),
    }


def _artifact_manifest(output_dir: Path, orchestration_sha: str) -> dict[str, object]:
    files: list[dict[str, object]] = []
    for path in sorted(output_dir.rglob("*")):
        if not path.is_file() or path.name == "artifact-manifest.json":
            continue
        files.append(
            {
                "path": path.relative_to(output_dir).as_posix(),
                "size": path.stat().st_size,
                "sha256": sha256_file(path),
            }
        )
    manifest: dict[str, object] = {
        "schema": ARTIFACT_SCHEMA,
        "kind": ARTIFACT_KIND,
        "orchestration_sha256": orchestration_sha,
        "files": files,
    }
    manifest["manifest_sha256"] = canonical_digest(manifest)
    return manifest


def orchestrate(
    *,
    repo_root: Path,
    pack_root: Path,
    output_dir: Path,
    cpu: int,
    rounds: int,
    smoke: bool,
    timeout_seconds: int,
) -> dict[str, object]:
    if os.name != "posix" or not Path("/proc/self/status").is_file():
        raise QualificationError("target-hardware orchestration is Linux-first and requires /proc")
    if shutil.which("taskset") is None:
        raise QualificationError("target-hardware orchestration requires taskset")
    repo_root = repo_root.resolve()
    pack_root = pack_root.resolve()
    output_dir = output_dir.resolve()
    require_external_evidence_paths(repo_root, pack_root, output_dir)
    if cpu not in allowed_cpus():
        raise QualificationError(f"requested CPU {cpu} is outside current process affinity")
    head_sha = repository_identity(repo_root)
    pack_set = verify_pack_set(pack_root, repo_root)

    output_dir.mkdir(parents=True, exist_ok=True)
    scratch_dir = output_dir.parent / f".{output_dir.name}.build-scratch"
    if scratch_dir.exists():
        raise QualificationError(f"build scratch already exists: {scratch_dir}")
    scratch_dir.mkdir(parents=True)
    build_environment = controlled_environment(scratch_dir)
    try:
        build = _controlled_build(
            repo_root, output_dir, scratch_dir, head_sha, build_environment
        )
        scheduled = schedule(rounds)
        children = [
            run_child(
                scheduled=item,
                pack_set=pack_set,
                build=build,
                repo_root=repo_root,
                output_dir=output_dir,
                build_environment=build_environment,
                cpu=cpu,
                smoke=smoke,
                timeout_seconds=timeout_seconds,
            )
            for item in scheduled
        ]
        if repository_identity(repo_root) != head_sha:
            raise QualificationError("repository changed during benchmark rounds")
        if sha256_file(build.executable) != build.executable_sha256:
            raise QualificationError("benchmark executable changed after rounds")
        for entry in pack_set.entries.values():
            if sha256_file(entry.path) != entry.sha256:
                raise QualificationError(f"pack changed after rounds: {entry.dimension}")

        aggregates = aggregate_children(children)
        noise = classify_noise(aggregates, smoke=smoke, rounds=rounds)
        orchestration: dict[str, object] = {
            "schema": SCHEMA,
            "kind": KIND,
            "mode": "smoke" if smoke else "qualification",
            "qualification_complete": True,
            "population_evidence_eligible": noise["population_evidence_eligible"],
            "decision_evidence_eligible": False,
            "decision_blockers": [
                "candidate-isolated synthetic mutation/promotion evidence not attached",
                "Pareto selection record not assembled",
            ],
            "decision_scope": DECISION_SCOPE,
            "cross_dimension_score_allowed": False,
            "rounds": rounds,
            "cpu": cpu,
            "cpu_topology": cpu_topology(cpu),
            "initial_mems_allowed_list": _status_field("Mems_allowed_list"),
            "candidates": list(CANDIDATES),
            "production_candidates": list(PRODUCTION_CANDIDATES),
            "dimensions": list(DIMENSIONS),
            "identities": {
                "repository_commit_sha": head_sha,
                "benchmark_executable_sha256": build.executable_sha256,
                "pack_manifest_sha256": pack_set.manifest_sha256,
                "representative_policy": pack_set.policy,
                "population_sha256": pack_set.population_sha256,
                "admission_sha256": pack_set.admission_sha256,
                "source_artifact_manifest_sha256": pack_set.source_artifact_manifest_sha256,
            },
            "build": {
                "toolchain": RUST_TOOLCHAIN,
                "rustc_commit": RUSTC_COMMIT,
                "rustc_verbose": build.rustc_verbose,
                "cargo_version": build.cargo_version,
                "profile": BUILD_PROFILE,
                "codegen_policy": CODEGEN_POLICY,
                "cargo_toml_sha256": build.cargo_toml_sha256,
                "cargo_lock_sha256": build.cargo_lock_sha256,
                "cargo_config_sha256": build.cargo_config_sha256,
                "parent_cargo_configs": list(build.parent_cargo_configs),
                "cargo_incremental": "0",
                "offline": True,
                "isolated_cargo_home": True,
                "isolated_target_dir": True,
            },
            "packs": {
                dimension: {
                    "sha256": entry.sha256,
                    "size": entry.size,
                    "section_count": entry.section_count,
                }
                for dimension, entry in pack_set.entries.items()
            },
            "schedule": [
                {
                    "round": item.round_index,
                    "dimension_position": item.dimension_position,
                    "candidate_position": item.candidate_position,
                    "dimension": item.dimension,
                    "candidate": item.candidate,
                }
                for item in scheduled
            ],
            "children": children,
            "aggregates": aggregates,
            "noise_qualification": noise,
        }
        orchestration["evidence_sha256"] = canonical_digest(orchestration)
        (output_dir / "orchestration.json").write_text(
            json.dumps(orchestration, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
    finally:
        if scratch_dir.exists():
            shutil.rmtree(scratch_dir)

    orchestration_sha = str(orchestration["evidence_sha256"])
    artifact = _artifact_manifest(output_dir, orchestration_sha)
    (output_dir / "artifact-manifest.json").write_text(
        json.dumps(artifact, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return orchestration


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path("."))
    parser.add_argument("--pack-root", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--rounds", type=int)
    parser.add_argument("--child-timeout-seconds", type=int, default=900)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--smoke", action="store_true")
    mode.add_argument("--qualification", action="store_true")
    return parser


def main() -> int:
    args = _parser().parse_args()
    rounds = args.rounds if args.rounds is not None else (1 if args.smoke else 5)
    try:
        result = orchestrate(
            repo_root=args.repo_root,
            pack_root=args.pack_root,
            output_dir=args.output_dir,
            cpu=args.cpu,
            rounds=rounds,
            smoke=args.smoke,
            timeout_seconds=args.child_timeout_seconds,
        )
    except (QualificationError, OSError, json.JSONDecodeError, subprocess.TimeoutExpired) as error:
        print(f"section target-hardware qualification error: {error}")
        return 1
    print(
        "section target-hardware orchestration: "
        f"mode={result['mode']} rounds={result['rounds']} "
        f"population_eligible={result['population_evidence_eligible']} "
        f"decision_eligible={result['decision_evidence_eligible']} "
        f"evidence={result['evidence_sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
