#!/usr/bin/env python3
"""Combine independent R2C cold import -> residency target-hardware runs.

Inputs must be full single-CPU artifacts produced by `r2c_cold_import_target_run.py`. The combiner
fails closed on code/toolchain/hardware/workload drift and computes cross-process distribution
summaries without setting a performance threshold or admitting a production mechanism.
"""
from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path
from typing import Any, Iterable

SCHEMA = 1
BENCHMARK_KIND = "r2c-cold-import-residency-reference-baseline"
OUTPUT_KIND = "r2c-cold-import-residency-cross-process-evidence"
TARGET_RUNNER_ID = "r2c-cold-import-residency-target-run-v1"
MIN_RUNS = 3

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
EMPTY_ALLOWED_HARDWARE_FIELDS = frozenset({"rustflags", "cargo_encoded_rustflags"})
PHASES = ("import", "install", "combined")


class EvidenceError(RuntimeError):
    """Fail-closed evidence-combination error."""


def _positive_int(value: object, label: str) -> int:
    if type(value) is not int or value <= 0:
        raise EvidenceError(f"{label} must be a positive integer")
    return value


def validate_summary(path: Path, phase: str, value: Any) -> dict[str, int]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{path}: summary_ns.{phase} must be an object")
    result = {
        field: _positive_int(value.get(field), f"{path}: summary_ns.{phase}.{field}")
        for field in ("p50", "p95", "p99", "p999", "max")
    }
    ordered = [result[field] for field in ("p50", "p95", "p99", "p999", "max")]
    if ordered != sorted(ordered):
        raise EvidenceError(f"{path}: summary_ns.{phase} is not monotone")
    return result


def validate_target_qualification(path: Path, data: dict[str, Any], hardware: dict[str, Any]) -> None:
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
            f"{path}: requested CPU {requested_cpu} differs from observed affinity "
            f"{hardware.get('cpus_allowed_list')!r}"
        )


def validate_artifact(path: Path, data: dict[str, Any]) -> None:
    expected = {
        "schema": SCHEMA,
        "kind": BENCHMARK_KIND,
        "mode": "full",
        "reference_section_builder": True,
        "production_section_policy_selected": False,
        "performance_admitted": False,
    }
    for field, value in expected.items():
        if data.get(field) != value:
            raise EvidenceError(f"{path}: {field} must be {value!r}, got {data.get(field)!r}")

    hardware = data.get("hardware")
    if not isinstance(hardware, dict):
        raise EvidenceError(f"{path}: hardware metadata missing")
    validate_target_qualification(path, data, hardware)

    rounds = _positive_int(data.get("measured_rounds"), f"{path}: measured_rounds")
    _positive_int(data.get("warmup_rounds"), f"{path}: warmup_rounds")
    expected_state = data.get("expected_state_id")
    if type(expected_state) is not int or expected_state < 0:
        raise EvidenceError(f"{path}: expected_state_id must be a non-negative integer")

    summaries = data.get("summary_ns")
    if not isinstance(summaries, dict):
        raise EvidenceError(f"{path}: summary_ns missing")
    for phase in PHASES:
        validate_summary(path, phase, summaries.get(phase))

    counters = data.get("builder_counters")
    if not isinstance(counters, dict):
        raise EvidenceError(f"{path}: builder_counters missing")
    if counters.get("uniform_sections") != rounds * 2:
        raise EvidenceError(f"{path}: uniform reference-section count mismatch")
    if counters.get("dense_sections") != 0 or counters.get("dense_cell_writes") != 0:
        raise EvidenceError(f"{path}: dense reference construction unexpectedly exercised")

    samples = data.get("samples")
    if not isinstance(samples, list) or len(samples) != rounds:
        raise EvidenceError(f"{path}: samples length does not match measured_rounds")
    for index, sample in enumerate(samples):
        if not isinstance(sample, dict):
            raise EvidenceError(f"{path}: sample {index} must be an object")
        for field in ("import_ns", "install_ns", "combined_ns"):
            _positive_int(sample.get(field), f"{path}: sample {index}.{field}")


def load_artifact(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read benchmark artifact {path}: {error}") from error
    if not isinstance(data, dict):
        raise EvidenceError(f"benchmark artifact must be an object: {path}")
    validate_artifact(path, data)
    return data


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
    if not result["cpus_allowed_list"].isdecimal():
        raise EvidenceError(
            "target-hardware evidence must be pinned to exactly one logical CPU; "
            f"cpus_allowed_list={result['cpus_allowed_list']!r}"
        )
    return result


def target_witness(data: dict[str, Any]) -> dict[str, Any]:
    target = data["target_qualification"]
    assert isinstance(target, dict)
    return dict(target)


def workload_witness(data: dict[str, Any]) -> dict[str, Any]:
    return {
        "mode": data["mode"],
        "reference_section_builder": data["reference_section_builder"],
        "production_section_policy_selected": data["production_section_policy_selected"],
        "warmup_rounds": data["warmup_rounds"],
        "measured_rounds": data["measured_rounds"],
        "expected_state_id": data["expected_state_id"],
        "builder_counters": data["builder_counters"],
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


def summarize_phase(phase: str, runs: list[dict[str, Any]]) -> dict[str, Any]:
    rows: list[dict[str, int]] = []
    for run_index, run in enumerate(runs, start=1):
        summary = run["summary_ns"][phase]
        p50 = int(summary["p50"])
        p95 = int(summary["p95"])
        p99 = int(summary["p99"])
        p999 = int(summary["p999"])
        maximum = int(summary["max"])
        rows.append(
            {
                "run": run_index,
                "p50_ns": p50,
                "p95_ns": p95,
                "p99_ns": p99,
                "p999_ns": p999,
                "max_ns": maximum,
                "p99_over_p50_ppm": relative_ppm(p99, p50),
                "p999_over_p50_ppm": relative_ppm(p999, p50),
            }
        )

    p50_values = [row["p50_ns"] for row in rows]
    p99_values = [row["p99_ns"] for row in rows]
    p999_values = [row["p999_ns"] for row in rows]
    center = median_int(p50_values)
    mad = mad_int(p50_values)
    return {
        "phase": phase,
        "runs": rows,
        "cross_process": {
            "p50_median_ns": center,
            "p50_mad_ns": mad,
            "p50_relative_mad_ppm": relative_ppm(mad, center),
            "p50_min_ns": min(p50_values),
            "p50_max_ns": max(p50_values),
            "p99_median_ns": median_int(p99_values),
            "p999_median_ns": median_int(p999_values),
            "p99_over_p50_median_ppm": median_int([row["p99_over_p50_ppm"] for row in rows]),
            "p999_over_p50_median_ppm": median_int([row["p999_over_p50_ppm"] for row in rows]),
            "run_count": len(rows),
        },
    }


def combine(paths: list[Path]) -> dict[str, Any]:
    if len(paths) < MIN_RUNS:
        raise EvidenceError(f"at least {MIN_RUNS} independent runs are required; got {len(paths)}")
    normalized = [path.resolve() for path in paths]
    if len(normalized) != len(set(normalized)):
        raise EvidenceError("input paths must be unique")

    runs = [load_artifact(path) for path in paths]
    hardware = stable_hardware(runs[0])
    target = target_witness(runs[0])
    witness = workload_witness(runs[0])
    for path, run in zip(paths[1:], runs[1:]):
        if stable_hardware(run) != hardware:
            raise EvidenceError(f"{path}: stable hardware/toolchain metadata differs from first run")
        if target_witness(run) != target:
            raise EvidenceError(f"{path}: target-qualification witness differs from first run")
        if workload_witness(run) != witness:
            raise EvidenceError(f"{path}: workload/structural witness differs from first run")

    return {
        "schema": SCHEMA,
        "kind": OUTPUT_KIND,
        "benchmark": BENCHMARK_KIND,
        "run_count": len(runs),
        "mechanically_consistent": True,
        "performance_admitted": False,
        "human_baseline_review_required": True,
        "timing_threshold_selected": False,
        "source_artifacts": [str(path) for path in paths],
        "target_qualification": target,
        "stable_hardware": hardware,
        "workload_witness": witness,
        "phases": [summarize_phase(phase, runs) for phase in PHASES],
        "notes": [
            "Cross-process consistency is mechanical; machine quietness and operating conditions still require human review.",
            "Ordinary hosted/full artifacts are rejected because only the explicit target runner stamps target_qualification.",
            "This reference-section baseline cannot select the production section mechanism.",
            "No timing threshold or production performance admission is made by this report.",
        ],
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", nargs="+", type=Path, help="target-runner benchmark JSON files")
    parser.add_argument("--output", type=Path, help="optional combined source-free report path")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        report = combine(args.artifacts)
    except EvidenceError as error:
        print(f"R2C cold import evidence rejected: {error}")
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
