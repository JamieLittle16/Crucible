#!/usr/bin/env python3
"""Combine independent R2C import-to-resident target runs without cherry-picking.

Inputs must be artifacts produced by `r2c_import_resident_target_run.py`. The combiner verifies exact
code/toolchain/hardware, stored-world byte identity, workload shape, semantic/accounting witness and
single-CPU provenance before reporting cross-process timing summaries. It never selects a production
threshold or mechanism automatically.
"""
from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path
from typing import Any, Iterable

SCHEMA = 1
BENCHMARK_KIND = "r2c-import-resident-cpu-qualification"
RAW_MODE = "hosted-diagnostic"
OUTPUT_KIND = "r2c-import-resident-cross-process-evidence"
TARGET_RUNNER_ID = "r2c-import-resident-target-run-v1"
WORLD_ID_POLICY = "r2c-dimension-region-bytes-sha256-v1"
MIN_RUNS = 3
SUMMARY_NAMES = (
    "dimension_setup",
    "region_open",
    "import",
    "install",
    "whole_chunk",
    "round",
)
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


class EvidenceError(RuntimeError):
    """Fail-closed evidence-combination error."""


def load_artifact(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise EvidenceError(f"cannot read target artifact {path}: {error}") from error
    if not isinstance(data, dict):
        raise EvidenceError(f"target artifact must be a JSON object: {path}")
    validate_artifact(path, data)
    return data


def _int_field(value: Any, label: str, *, positive: bool = False) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise EvidenceError(f"{label} must be an integer")
    if positive and value <= 0:
        raise EvidenceError(f"{label} must be positive")
    if not positive and value < 0:
        raise EvidenceError(f"{label} must be non-negative")
    return value


def validate_summary(path: Path, name: str, value: Any) -> None:
    if not isinstance(value, dict):
        raise EvidenceError(f"{path}: missing samples_ns.{name}")
    count = _int_field(value.get("count"), f"{path}: samples_ns.{name}.count", positive=True)
    p50 = _int_field(value.get("p50"), f"{path}: samples_ns.{name}.p50")
    p95 = _int_field(value.get("p95"), f"{path}: samples_ns.{name}.p95")
    p99 = _int_field(value.get("p99"), f"{path}: samples_ns.{name}.p99")
    maximum = _int_field(value.get("max"), f"{path}: samples_ns.{name}.max")
    if count <= 0 or not (p50 <= p95 <= p99 <= maximum):
        raise EvidenceError(f"{path}: non-monotone samples_ns.{name}")


def validate_target_qualification(path: Path, data: dict[str, Any], hardware: dict[str, Any]) -> None:
    target = data.get("target_qualification")
    if not isinstance(target, dict):
        raise EvidenceError(
            f"{path}: target_qualification missing; ordinary benchmark artifacts are not baseline-eligible"
        )
    expected = {
        "schema": SCHEMA,
        "runner": TARGET_RUNNER_ID,
        "explicit_operator_action": True,
        "single_cpu_requirement_enforced": True,
        "raw_benchmark_self_promoted": False,
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
            f"{path}: target runner requested CPU {requested_cpu} but artifact observed "
            f"{hardware.get('cpus_allowed_list')!r}"
        )


def validate_artifact(path: Path, data: dict[str, Any]) -> None:
    expected = {
        "schema": SCHEMA,
        "kind": BENCHMARK_KIND,
        "mode": RAW_MODE,
        "production_decision_eligible": False,
    }
    for field, value in expected.items():
        if data.get(field) != value:
            raise EvidenceError(f"{path}: {field} must be {value!r}, got {data.get(field)!r}")

    hardware = data.get("hardware")
    if not isinstance(hardware, dict):
        raise EvidenceError(f"{path}: hardware metadata missing")
    validate_target_qualification(path, data, hardware)

    input_world = data.get("input_world")
    if not isinstance(input_world, dict):
        raise EvidenceError(f"{path}: input_world evidence missing")
    if input_world.get("policy") != WORLD_ID_POLICY:
        raise EvidenceError(f"{path}: unexpected input_world policy")
    digest = input_world.get("sha256")
    if not isinstance(digest, str) or len(digest) != 64:
        raise EvidenceError(f"{path}: input_world.sha256 must be a SHA-256 hex digest")
    try:
        bytes.fromhex(digest)
    except ValueError as error:
        raise EvidenceError(f"{path}: input_world.sha256 is not hexadecimal") from error
    for name in (
        "region_files",
        "region_file_bytes",
        "external_payload_files",
        "external_payload_bytes",
        "total_bytes",
    ):
        _int_field(input_world.get(name), f"{path}: input_world.{name}")
    if input_world["region_files"] <= 0:
        raise EvidenceError(f"{path}: input_world must contain at least one region file")
    if input_world["total_bytes"] != (
        input_world["region_file_bytes"] + input_world["external_payload_bytes"]
    ):
        raise EvidenceError(f"{path}: input_world byte accounting mismatch")

    world = data.get("world")
    if not isinstance(world, dict):
        raise EvidenceError(f"{path}: world metadata missing")
    chunks = _int_field(world.get("chunks"), f"{path}: world.chunks", positive=True)
    region_files = _int_field(world.get("region_files"), f"{path}: world.region_files", positive=True)
    region_file_bytes = _int_field(
        world.get("region_file_bytes"), f"{path}: world.region_file_bytes", positive=True
    )
    _int_field(world.get("compressed_payload_bytes"), f"{path}: world.compressed_payload_bytes")
    if region_files != input_world["region_files"] or region_file_bytes != input_world["region_file_bytes"]:
        raise EvidenceError(f"{path}: benchmark world metadata disagrees with input_world identity")

    config = data.get("config")
    if not isinstance(config, dict):
        raise EvidenceError(f"{path}: config metadata missing")
    measured_rounds = _int_field(
        config.get("measured_rounds"), f"{path}: config.measured_rounds", positive=True
    )
    _int_field(config.get("warmup_rounds"), f"{path}: config.warmup_rounds", positive=True)
    expected_config = {
        "filesystem_io_timed": False,
        "dimension_setup_timed_separately": True,
        "round_excludes_dimension_setup": True,
    }
    for field, value in expected_config.items():
        if config.get(field) != value:
            raise EvidenceError(f"{path}: config.{field} must be {value!r}")

    builder = data.get("builder")
    if not isinstance(builder, dict):
        raise EvidenceError(f"{path}: builder accounting missing")
    uniform = _int_field(builder.get("uniform_sections"), f"{path}: builder.uniform_sections")
    dense = _int_field(builder.get("dense_sections"), f"{path}: builder.dense_sections")
    dense_cells = _int_field(
        builder.get("dense_cells_copied"), f"{path}: builder.dense_cells_copied"
    )
    retained = _int_field(
        builder.get("retained_cells_written"), f"{path}: builder.retained_cells_written"
    )
    built = uniform + dense
    if built <= 0:
        raise EvidenceError(f"{path}: builder materialized no sections")
    if dense_cells != dense * 4096:
        raise EvidenceError(f"{path}: dense section copy accounting mismatch")
    if retained != built * 4096:
        raise EvidenceError(f"{path}: retained section write accounting mismatch")

    scratch = data.get("scratch")
    if not isinstance(scratch, dict):
        raise EvidenceError(f"{path}: scratch metadata missing")
    if scratch.get("grew_during_measurement") is not False:
        raise EvidenceError(f"{path}: importer scratch grew during measurement")
    if scratch.get("decoder_grew_during_measurement") is not False:
        raise EvidenceError(f"{path}: decoder scratch grew during measurement")

    state_data = data.get("state_data")
    mechanism = data.get("mechanism")
    profile = data.get("profile")
    if not isinstance(state_data, dict) or not isinstance(mechanism, dict) or not isinstance(profile, dict):
        raise EvidenceError(f"{path}: state_data/mechanism/profile metadata missing")

    samples = data.get("samples_ns")
    if not isinstance(samples, dict):
        raise EvidenceError(f"{path}: samples_ns metadata missing")
    for name in SUMMARY_NAMES:
        validate_summary(path, name, samples.get(name))
    if samples["dimension_setup"]["count"] != measured_rounds:
        raise EvidenceError(f"{path}: dimension_setup sample count mismatch")
    if samples["round"]["count"] != measured_rounds:
        raise EvidenceError(f"{path}: round sample count mismatch")
    if samples["region_open"]["count"] != region_files * measured_rounds:
        raise EvidenceError(f"{path}: region_open sample count mismatch")
    expected_chunks = chunks * measured_rounds
    for name in ("import", "install", "whole_chunk"):
        if samples[name]["count"] != expected_chunks:
            raise EvidenceError(f"{path}: {name} sample count mismatch")


def stable_hardware(data: dict[str, Any]) -> dict[str, str]:
    hardware = data["hardware"]
    assert isinstance(hardware, dict)
    result: dict[str, str] = {}
    for field in STABLE_HARDWARE_FIELDS:
        value = hardware.get(field)
        if not isinstance(value, str):
            raise EvidenceError(f"hardware.{field} must be a string")
        if not value and field not in EMPTY_ALLOWED_HARDWARE_FIELDS:
            raise EvidenceError(f"hardware.{field} must be non-empty")
        result[field] = value
    if not result["cpus_allowed_list"].isdecimal():
        raise EvidenceError("target evidence must be pinned to exactly one logical CPU")
    return result


def target_witness(data: dict[str, Any]) -> dict[str, Any]:
    target = data["target_qualification"]
    assert isinstance(target, dict)
    return dict(target)


def workload_witness(data: dict[str, Any]) -> dict[str, Any]:
    return {
        "input_world": data["input_world"],
        "world": data["world"],
        "profile": data["profile"],
        "config": data["config"],
        "mechanism": data["mechanism"],
        "state_data": data["state_data"],
        "builder": data["builder"],
        "scratch": data["scratch"],
        "empty_sections_synthesized": data.get("empty_sections_synthesized"),
        "semantic_checksum": data.get("semantic_checksum"),
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
        return 0 if numerator == 0 else 1_000_000_000
    return (numerator * 1_000_000 + denominator // 2) // denominator


def summarize_stage(name: str, runs: list[dict[str, Any]]) -> dict[str, Any]:
    rows = []
    for index, run in enumerate(runs, start=1):
        summary = run["samples_ns"][name]
        p50 = int(summary["p50"])
        p99 = int(summary["p99"])
        rows.append(
            {
                "run": index,
                "count": int(summary["count"]),
                "p50_ns": p50,
                "p95_ns": int(summary["p95"]),
                "p99_ns": p99,
                "max_ns": int(summary["max"]),
                "p99_over_p50_ppm": relative_ppm(p99, p50),
            }
        )
    p50s = [row["p50_ns"] for row in rows]
    p99s = [row["p99_ns"] for row in rows]
    tails = [row["p99_over_p50_ppm"] for row in rows]
    center = median_int(p50s)
    mad = mad_int(p50s)
    return {
        "name": name,
        "runs": rows,
        "cross_process": {
            "p50_median_ns": center,
            "p50_mad_ns": mad,
            "p50_relative_mad_ppm": relative_ppm(mad, center),
            "p50_min_ns": min(p50s),
            "p50_max_ns": max(p50s),
            "p99_median_ns": median_int(p99s),
            "p99_tail_amplification_median_ppm": median_int(tails),
            "run_count": len(rows),
        },
    }


def combine(paths: list[Path]) -> dict[str, Any]:
    if len(paths) < MIN_RUNS:
        raise EvidenceError(f"at least {MIN_RUNS} independent runs are required; got {len(paths)}")
    resolved = [path.resolve() for path in paths]
    if len(resolved) != len(set(resolved)):
        raise EvidenceError("input paths must be unique")

    runs = [load_artifact(path) for path in resolved]
    hardware = stable_hardware(runs[0])
    target = target_witness(runs[0])
    witness = workload_witness(runs[0])
    for path, run in zip(resolved[1:], runs[1:]):
        if stable_hardware(run) != hardware:
            raise EvidenceError(f"{path}: stable hardware/toolchain metadata differs from first run")
        if target_witness(run) != target:
            raise EvidenceError(f"{path}: target-qualification witness differs from first run")
        if workload_witness(run) != witness:
            raise EvidenceError(f"{path}: input world/workload/semantic witness differs from first run")

    return {
        "schema": SCHEMA,
        "kind": OUTPUT_KIND,
        "benchmark": BENCHMARK_KIND,
        "run_count": len(runs),
        "mechanically_consistent": True,
        "performance_admitted": False,
        "human_baseline_review_required": True,
        "timing_threshold_selected": False,
        "source_artifacts": [str(path) for path in resolved],
        "target_qualification": target,
        "stable_hardware": hardware,
        "workload_witness": witness,
        "stages": [summarize_stage(name, runs) for name in SUMMARY_NAMES],
        "notes": [
            "Only artifacts stamped by the explicit target runner are accepted.",
            "The exact dimension-region byte set must match across every process.",
            "Mechanical consistency does not replace human review of machine quietness and operating conditions.",
            "No timing threshold, section policy, decompression policy or production mechanism is selected automatically.",
        ],
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", nargs="+", type=Path, help="target-runner evidence JSON files")
    parser.add_argument("--output", type=Path, help="optional combined source-free evidence JSON")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        report = combine(args.artifacts)
    except EvidenceError as error:
        print(f"R2C import-resident evidence rejected: {error}")
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
