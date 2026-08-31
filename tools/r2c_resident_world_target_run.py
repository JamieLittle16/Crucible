#!/usr/bin/env python3
"""Run one R2C resident-world target-hardware qualification process.

This wrapper is intentionally separate from the benchmark binary. The benchmark always records raw
mechanism evidence; this wrapper is the explicit operator action that marks one full, CPU-pinned run
as eligible for later target-hardware baseline combination. Hosted CI never calls this tool.
"""
from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Sequence

SCHEMA = 1
BENCHMARK = "r2c-resident-world-lifecycle"
RUNNER_ID = "r2c-resident-world-target-run-v1"


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
        "helve-world-access-qualification",
        "--bin",
        "resident_world_lifecycle_bench",
        "--",
        "--full",
        "--require-single-cpu",
        "--output",
        str(raw_output),
    ]


def annotate_artifact(data: dict[str, Any], cpu: str) -> dict[str, Any]:
    cpu = parse_cpu(cpu)
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
            raise TargetRunError(
                f"benchmark artifact {field} must be {value!r}, got {data.get(field)!r}"
            )
    hardware = data.get("hardware")
    if not isinstance(hardware, dict):
        raise TargetRunError("benchmark artifact hardware metadata is missing")
    observed = hardware.get("cpus_allowed_list")
    if observed != cpu:
        raise TargetRunError(
            f"benchmark did not remain pinned to requested CPU {cpu}; observed {observed!r}"
        )
    if "target_qualification" in data:
        raise TargetRunError("benchmark artifact already contains target_qualification metadata")

    annotated = dict(data)
    annotated["target_qualification"] = {
        "schema": SCHEMA,
        "runner": RUNNER_ID,
        "explicit_operator_action": True,
        "single_cpu_requirement_enforced": True,
        "requested_cpu": cpu,
        "hosted_ci_eligible": False,
    }
    return annotated


def run_target_process(cpu: str, output: Path, command_runner: Any = subprocess.run) -> None:
    cpu = parse_cpu(cpu)
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        raise TargetRunError(f"refusing to overwrite existing target evidence: {output}")

    with tempfile.TemporaryDirectory(prefix="helve-r2c-resident-world-") as temporary:
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
        print(f"R2C resident-world target run rejected: {error}")
        return 2
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
