#!/usr/bin/env python3
"""Combine independent R2C resident-world benchmark runs without cherry-picking.

Inputs must be full single-CPU artifacts produced through the explicit
`r2c_resident_world_target_run.py` operator path. This tool verifies that all runs describe the same
code, machine/toolchain configuration, workload shape and semantic witness before computing
cross-process summaries. It never upgrades timing into semantic admission and never declares a
production performance threshold.
"""
from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path
from typing import Any, Iterable

SCHEMA = 1
BENCHMARK = "r2c-resident-world-lifecycle"
OUTPUT_KIND = "r2c-resident-world-cross-process-evidence"
TARGET_RUNNER_ID = "r2c-resident-world-target-run-v1"
MIN_RUNS = 3

# Fields that should remain invariant across independent processes in one controlled baseline set.
# Deliberately exclude dynamic observations such as current frequency and load average.
STABLE_HARDWARE_FIELDS = (
    "commit_sha",
    "rustc_verbose",
    "target_triple",
    "cpu_model",
    "cpu_vendor",
    "cpu_family",
    "cpu_model_id",
    "cpu_stepping",
    "cpu_microcode",
    "kernel",
    "cpu_governor",
    "cpu_min_khz",
    "cpu_max_khz",
    "cpus_allowed_list",
    "mems_allowed_list",
    "online_cpus",
    "smt_active",
    "cache_topology",
    "perf_event_paranoid",
    "transparent_hugepage",
    "memory_total_kib",
    "no_turbo",
    "rustflags",
    "cargo_encoded_rustflags",
)

# Empty Rust flag variables are meaningful: they prove the benchmark was not built with an
# unrecorded caller-supplied flag set. All physical/toolchain identity fields remain non-empty.
EMPTY_ALLOWED_HARDWARE_FIELDS = frozenset({"rustflags", "cargo_encoded_rustflags"})


class EvidenceError(RuntimeError):
    """Fail-closed evidence-combination error."""


def load_artifact(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read benchmark artifact {path}: {error}") from error
    if not isinstance(data, dict):
        raise EvidenceError(f"benchmark artifact must be an object: {path}")
    validate_artifact(path, data)
    return data


def validate_artifact(path: Path, data: dict[str, Any]) -> None:
    expected = {
        "schema": SCHEMA,
        "benchmark": BENCHMARK,
        "mode": "full",
        "hosted_ci_is_diagnostic_only": True,
        "timing_threshold_selected": False,
        "production_path_unchanged": True,
    }
    for field, value in expected.items():
        if data.get(field) != value:
            raise EvidenceError(f"{path}: {field} must be {value!r}, got {data.get(field)!r}")

    hardware = data.get("hardware")
    if not isinstance(hardware, dict):
        raise EvidenceError(f"{path}: hardware metadata missing")
    if not isinstance(data.get("structural"), dict):
        raise EvidenceError(f"{path}: structural metadata missing")
    validate_target_qualification(path, data, hardware)

    cases = data.get("cases")
    if not isinstance(cases, list) or not cases:
        raise EvidenceError(f"{path}: cases must be a non-empty array")
    if data.get("measured_rounds", 0) <= 0 or data.get("hot_reads", 0) <= 0:
        raise EvidenceError(f"{path}: measured_rounds and hot_reads must be positive")

    names: set[str] = set()
    for case in cases:
        if not isinstance(case, dict) or not isinstance(case.get("name"), str):
            raise EvidenceError(f"{path}: malformed case")
        name = case["name"]
        if name in names:
            raise EvidenceError(f"{path}: duplicate case {name}")
        names.add(name)
        if case.get("semantic_equivalent") is not True:
            raise EvidenceError(f"{path}: case {name} failed semantic equivalence")
        if case.get("chunk_count") != case.get("side", 0) ** 2:
            raise EvidenceError(f"{path}: case {name} chunk_count/side mismatch")
        if case.get("stale_rejections_per_lifecycle") != case.get("chunk_count"):
            raise EvidenceError(f"{path}: case {name} stale-rejection count mismatch")
        if not case.get("lifecycle_checksum") or not case.get("hot_checksum"):
            raise EvidenceError(f"{path}: case {name} has zero semantic checksum")
        for summary_name in (
            "lifecycle_summary_ns",
            "repeated_resolve_summary_ns",
            "resolve_once_summary_ns",
        ):
            validate_summary(path, name, summary_name, case.get(summary_name))


def validate_target_qualification(
    path: Path,
    data: dict[str, Any],
    hardware: dict[str, Any],
) -> None:
    target = data.get("target_qualification")
    if not isinstance(target, dict):
        raise EvidenceError(
            f"{path}: target_qualification missing; ordinary hosted/full artifacts are not baseline-eligible"
        )
    expected = {
        "schema": SCHEMA,
        "runner": TARGET_RUNNER_ID,
        "explicit_operator_action": True,
        "single_cpu_requirement_enforced": True,
        "hosted_ci_eligible": False,
    }
    for field, value in expected.items():
        if target.get(field) != value:
            raise EvidenceError(
                f"{path}: target_qualification.{field} must be {value!r}, got {target.get(field)!r}"
            )
    requested_cpu = target.get("requested_cpu")
    if not isinstance(requested_cpu, str) or not requested_cpu.isdecimal():
        raise EvidenceError(f"{path}: target_qualification.requested_cpu must be one logical CPU")
    if hardware.get("cpus_allowed_list") != requested_cpu:
        raise EvidenceError(
            f"{path}: target qualification requested CPU {requested_cpu} but artifact observed "
            f"{hardware.get('cpus_allowed_list')!r}"
        )


def validate_summary(path: Path, case: str, name: str, value: Any) -> None:
    if not isinstance(value, dict):
        raise EvidenceError(f"{path}: {case} missing {name}")
    try:
        p50 = int(value["p50"])
        p95 = int(value["p95"])
        p99 = int(value["p99"])
        maximum = int(value["max"])
    except (KeyError, TypeError, ValueError) as error:
        raise EvidenceError(f"{path}: {case} malformed {name}") from error
    if not (0 < p50 <= p95 <= p99 <= maximum):
        raise EvidenceError(f"{path}: {case} non-monotone {name}")


def stable_hardware(data: dict[str, Any]) -> dict[str, str]:
    hardware = data["hardware"]
    assert isinstance(hardware, dict)
    result: dict[str, str] = {}
    for field in STABLE_HARDWARE_FIELDS:
        value = hardware.get(field)
        if not isinstance(value, str):
            raise EvidenceError(f"hardware.{field} must be a string")
        if not value and field not in EMPTY_ALLOWED_HARDWARE_FIELDS:
            raise EvidenceError(f"hardware.{field} must be a non-empty string")
        result[field] = value
    allowed = result["cpus_allowed_list"]
    if not allowed.isdecimal():
        raise EvidenceError(
            "target-hardware evidence must be pinned to exactly one logical CPU; "
            f"cpus_allowed_list={allowed!r}"
        )
    return result


def target_witness(data: dict[str, Any]) -> dict[str, Any]:
    target = data["target_qualification"]
    assert isinstance(target, dict)
    return dict(target)


def semantic_witness(data: dict[str, Any]) -> dict[str, Any]:
    cases = data["cases"]
    assert isinstance(cases, list)
    return {
        "structural": data["structural"],
        "measured_rounds": data["measured_rounds"],
        "hot_reads": data["hot_reads"],
        "cases": [
            {
                "name": case["name"],
                "side": case["side"],
                "chunk_count": case["chunk_count"],
                "stale_rejections_per_lifecycle": case["stale_rejections_per_lifecycle"],
                "lifecycle_checksum": case["lifecycle_checksum"],
                "hot_checksum": case["hot_checksum"],
            }
            for case in cases
        ],
    }


def median_int(values: Iterable[int]) -> int:
    ordered = sorted(values)
    if not ordered:
        raise EvidenceError("cannot summarize an empty sample set")
    return int(statistics.median(ordered))


def mad_int(values: list[int]) -> int:
    center = median_int(values)
    return median_int(abs(value - center) for value in values)


def relative_ppm(numerator: int, denominator: int) -> int:
    if denominator <= 0:
        raise EvidenceError("ratio denominator must be positive")
    return (numerator * 1_000_000 + denominator // 2) // denominator


def summarize_case(name: str, runs: list[dict[str, Any]]) -> dict[str, Any]:
    case_rows = []
    for run_index, run in enumerate(runs, start=1):
        case = next(case for case in run["cases"] if case["name"] == name)
        lifecycle = case["lifecycle_summary_ns"]
        repeated = case["repeated_resolve_summary_ns"]
        resolved = case["resolve_once_summary_ns"]
        case_rows.append(
            {
                "run": run_index,
                "lifecycle_p50_ns": int(lifecycle["p50"]),
                "lifecycle_p99_ns": int(lifecycle["p99"]),
                "lifecycle_tail_amplification_ppm": relative_ppm(
                    int(lifecycle["p99"]), int(lifecycle["p50"])
                ),
                "repeated_resolve_p50_ns": int(repeated["p50"]),
                "resolve_once_p50_ns": int(resolved["p50"]),
                "repeated_over_resolve_once_p50_ppm": relative_ppm(
                    int(repeated["p50"]), int(resolved["p50"])
                ),
                "resolve_once_faster": int(resolved["p50"]) < int(repeated["p50"]),
            }
        )

    lifecycle_p50 = [row["lifecycle_p50_ns"] for row in case_rows]
    lifecycle_p99 = [row["lifecycle_p99_ns"] for row in case_rows]
    hot_ratio = [row["repeated_over_resolve_once_p50_ppm"] for row in case_rows]
    center = median_int(lifecycle_p50)
    mad = mad_int(lifecycle_p50)
    return {
        "name": name,
        "runs": case_rows,
        "cross_process": {
            "lifecycle_p50_median_ns": center,
            "lifecycle_p50_mad_ns": mad,
            "lifecycle_p50_relative_mad_ppm": relative_ppm(mad, center),
            "lifecycle_p50_min_ns": min(lifecycle_p50),
            "lifecycle_p50_max_ns": max(lifecycle_p50),
            "lifecycle_p99_median_ns": median_int(lifecycle_p99),
            "hot_ratio_median_ppm": median_int(hot_ratio),
            "resolve_once_faster_runs": sum(bool(row["resolve_once_faster"]) for row in case_rows),
            "run_count": len(case_rows),
        },
    }


def combine(paths: list[Path]) -> dict[str, Any]:
    if len(paths) < MIN_RUNS:
        raise EvidenceError(f"at least {MIN_RUNS} independent runs are required; got {len(paths)}")
    if len(paths) != len(set(paths)):
        raise EvidenceError("input paths must be unique")

    runs = [load_artifact(path) for path in paths]
    hardware = stable_hardware(runs[0])
    target = target_witness(runs[0])
    witness = semantic_witness(runs[0])
    for path, run in zip(paths[1:], runs[1:]):
        if stable_hardware(run) != hardware:
            raise EvidenceError(f"{path}: stable hardware/toolchain metadata differs from first run")
        if target_witness(run) != target:
            raise EvidenceError(f"{path}: target-qualification witness differs from first run")
        if semantic_witness(run) != witness:
            raise EvidenceError(f"{path}: workload or semantic witness differs from first run")

    case_names = [case["name"] for case in runs[0]["cases"]]
    return {
        "schema": SCHEMA,
        "kind": OUTPUT_KIND,
        "benchmark": BENCHMARK,
        "run_count": len(runs),
        "mechanically_consistent": True,
        "performance_admitted": False,
        "human_baseline_review_required": True,
        "timing_threshold_selected": False,
        "source_artifacts": [str(path) for path in paths],
        "target_qualification": target,
        "stable_hardware": hardware,
        "semantic_witness": witness,
        "cases": [summarize_case(name, runs) for name in case_names],
        "notes": [
            "Cross-process consistency is mechanical; suitability of machine quietness and benchmark operating conditions still requires human review.",
            "Ordinary hosted/full artifacts are rejected because only the explicit target runner stamps target_qualification.",
            "No timing threshold or production mechanism selection is made by this report.",
        ],
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", nargs="+", type=Path, help="target-runner benchmark JSON files")
    parser.add_argument("--output", type=Path, help="optional source-free combined report")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        report = combine(args.artifacts)
    except EvidenceError as error:
        print(f"R2C resident-world evidence rejected: {error}")
        return 2
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output is None:
        print(rendered, end="")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
        print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
