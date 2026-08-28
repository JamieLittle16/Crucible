#!/usr/bin/env python3
"""Run one R2C cold import -> residency target-hardware qualification process.

The benchmark binary always emits neutral raw evidence. This wrapper is the explicit operator action
that marks one full, CPU-pinned process as eligible for cross-process target-hardware combination.
Hosted CI never calls this tool and ordinary benchmark artifacts remain ineligible.
"""
from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Sequence

SCHEMA = 1
KIND = "r2c-cold-import-residency-reference-baseline"
RUNNER_ID = "r2c-cold-import-residency-target-run-v1"


class TargetRunError(RuntimeError):
    """Fail-closed target-run preparation error."""


def parse_cpu(value: str) -> str:
    if not value.isdecimal():
        raise TargetRunError(f"CPU must be one logical CPU index, got {value!r}")
    return value


def benchmark_command(cpu: str, raw_output: Path) -> list[str]:
    cpu = parse_cpu(cpu)
    return [
        "taskset",
        "-c",
        cpu,
        "cargo",
        "run",
        "--release",
        "--locked",
        "--package",
        "helve-r2c-cold-path-qualification",
        "--bin",
        "r2c_cold_path_bench",
        "--",
        "--full",
        "--require-single-cpu",
        "--output",
        str(raw_output),
    ]


def _positive_int(value: object, label: str) -> int:
    if type(value) is not int or value <= 0:
        raise TargetRunError(f"benchmark artifact {label} must be a positive integer")
    return value


def _validate_summary(value: object, label: str) -> None:
    if not isinstance(value, dict):
        raise TargetRunError(f"benchmark artifact summary_ns.{label} must be an object")
    fields = []
    for field in ("p50", "p95", "p99", "p999", "max"):
        fields.append(_positive_int(value.get(field), f"summary_ns.{label}.{field}"))
    if fields != sorted(fields):
        raise TargetRunError(f"benchmark artifact summary_ns.{label} is not monotone")


def validate_raw_artifact(data: dict[str, Any], cpu: str) -> None:
    cpu = parse_cpu(cpu)
    expected = {
        "schema": SCHEMA,
        "kind": KIND,
        "mode": "full",
        "reference_section_builder": True,
        "production_section_policy_selected": False,
        "performance_admitted": False,
    }
    for field, value in expected.items():
        if data.get(field) != value:
            raise TargetRunError(
                f"benchmark artifact {field} must be {value!r}, got {data.get(field)!r}"
            )

    measured_rounds = _positive_int(data.get("measured_rounds"), "measured_rounds")
    _positive_int(data.get("warmup_rounds"), "warmup_rounds")
    expected_state = data.get("expected_state_id")
    if type(expected_state) is not int or expected_state < 0:
        raise TargetRunError("benchmark artifact expected_state_id must be a non-negative integer")

    hardware = data.get("hardware")
    if not isinstance(hardware, dict):
        raise TargetRunError("benchmark artifact hardware metadata is missing")
    observed = hardware.get("cpus_allowed_list")
    if observed != cpu:
        raise TargetRunError(
            f"benchmark did not remain pinned to requested CPU {cpu}; observed {observed!r}"
        )

    summaries = data.get("summary_ns")
    if not isinstance(summaries, dict):
        raise TargetRunError("benchmark artifact summary_ns is missing")
    for phase in ("import", "install", "combined"):
        _validate_summary(summaries.get(phase), phase)

    counters = data.get("builder_counters")
    if not isinstance(counters, dict):
        raise TargetRunError("benchmark artifact builder_counters is missing")
    expected_uniform = measured_rounds * 2
    if counters.get("uniform_sections") != expected_uniform:
        raise TargetRunError(
            "benchmark artifact reference-builder uniform section count mismatch: "
            f"expected {expected_uniform}, got {counters.get('uniform_sections')!r}"
        )
    if counters.get("dense_sections") != 0 or counters.get("dense_cell_writes") != 0:
        raise TargetRunError("benchmark artifact unexpectedly exercised dense reference construction")

    samples = data.get("samples")
    if not isinstance(samples, list) or len(samples) != measured_rounds:
        raise TargetRunError("benchmark artifact samples length does not match measured_rounds")
    for index, sample in enumerate(samples):
        if not isinstance(sample, dict):
            raise TargetRunError(f"benchmark sample {index} must be an object")
        for phase in ("import_ns", "install_ns", "combined_ns"):
            _positive_int(sample.get(phase), f"samples[{index}].{phase}")

    if "target_qualification" in data:
        raise TargetRunError("benchmark artifact already contains target_qualification metadata")


def annotate_artifact(data: dict[str, Any], cpu: str) -> dict[str, Any]:
    validate_raw_artifact(data, cpu)
    annotated = dict(data)
    annotated["target_qualification"] = {
        "schema": SCHEMA,
        "runner": RUNNER_ID,
        "explicit_operator_action": True,
        "single_cpu_requirement_enforced": True,
        "requested_cpu": parse_cpu(cpu),
        "hosted_ci_eligible": False,
    }
    return annotated


def run_target_process(cpu: str, output: Path, command_runner: Any = subprocess.run) -> None:
    cpu = parse_cpu(cpu)
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        raise TargetRunError(f"refusing to overwrite existing target evidence: {output}")

    with tempfile.TemporaryDirectory(prefix="helve-r2c-cold-import-") as temporary:
        raw = Path(temporary) / "raw.json"
        command = benchmark_command(cpu, raw)
        try:
            command_runner(command, check=True)
        except (OSError, subprocess.CalledProcessError) as error:
            raise TargetRunError(f"benchmark command failed: {error}") from error
        try:
            data = json.loads(raw.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise TargetRunError(f"benchmark did not produce valid JSON: {error}") from error
        if not isinstance(data, dict):
            raise TargetRunError("benchmark artifact must be a JSON object")
        annotated = annotate_artifact(data, cpu)
        output.write_text(json.dumps(annotated, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cpu", required=True, help="single logical CPU index used by taskset")
    parser.add_argument("--output", required=True, type=Path, help="new target evidence JSON path")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        run_target_process(args.cpu, args.output)
    except TargetRunError as error:
        print(f"R2C cold import target run rejected: {error}")
        return 2
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
