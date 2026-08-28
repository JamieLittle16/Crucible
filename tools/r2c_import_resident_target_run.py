#!/usr/bin/env python3
"""Run one controlled R2C import-to-resident target-hardware qualification process.

The benchmark binary deliberately cannot declare itself decision-grade. This wrapper is the explicit
operator action that binds one CPU-pinned run to an exact stored-world byte identity and stamps a
separate target-qualification witness. The raw benchmark's conservative diagnostic flags remain
unchanged; later cross-process evidence still requires human review before any mechanism decision.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Sequence

SCHEMA = 1
BENCHMARK_KIND = "r2c-import-resident-cpu-qualification"
RAW_MODE = "hosted-diagnostic"
RUNNER_ID = "r2c-import-resident-target-run-v1"
WORLD_ID_POLICY = "r2c-dimension-region-bytes-sha256-v1"
DEFAULT_WARMUP_ROUNDS = 3
DEFAULT_MEASURED_ROUNDS = 12
HASH_CHUNK_BYTES = 1024 * 1024
EXPECTED_PROFILE = {
    "dimension": "minecraft:overworld",
    "min_block_y": -64,
    "height": 384,
    "section_count": 24,
}
SUMMARY_NAMES = (
    "dimension_setup",
    "region_open",
    "import",
    "install",
    "whole_chunk",
    "round",
)


class TargetRunError(RuntimeError):
    """Fail-closed target-run preparation error."""


def parse_cpu(value: str) -> str:
    if not value.isdecimal():
        raise TargetRunError(f"CPU must be one logical CPU index, got {value!r}")
    return value


def parse_positive(value: int, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise TargetRunError(f"{label} must be a positive integer")
    return value


def _nonnegative_int(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise TargetRunError(f"{label} must be a non-negative integer")
    return value


def _hash_world_member(digest: Any, root: Path, path: Path) -> int:
    if path.is_symlink() or not path.is_file():
        raise TargetRunError(f"world evidence member must be a regular non-symlink file: {path}")
    try:
        size = path.stat().st_size
    except OSError as error:
        raise TargetRunError(f"cannot stat world evidence member {path}: {error}") from error
    if size < 0:
        raise TargetRunError(f"world evidence member has invalid size: {path}")

    relative = path.relative_to(root).as_posix().encode("utf-8")
    digest.update(len(relative).to_bytes(4, "big"))
    digest.update(relative)
    digest.update(size.to_bytes(8, "big"))
    try:
        with path.open("rb") as stream:
            while chunk := stream.read(HASH_CHUNK_BYTES):
                digest.update(chunk)
    except OSError as error:
        raise TargetRunError(f"cannot hash world evidence member {path}: {error}") from error

    try:
        after_size = path.stat().st_size
    except OSError as error:
        raise TargetRunError(f"cannot restat world evidence member {path}: {error}") from error
    if after_size != size:
        raise TargetRunError(f"world evidence member changed while hashing: {path}")
    return size


def dimension_world_identity(dimension_root: Path) -> dict[str, Any]:
    root = dimension_root.resolve()
    region_dir = root / "region"
    if not region_dir.is_dir():
        raise TargetRunError(f"dimension region directory does not exist: {region_dir}")

    region_files = sorted(region_dir.glob("r.*.*.mca"), key=lambda path: path.name)
    external_files = sorted(region_dir.glob("c.*.*.mcc"), key=lambda path: path.name)
    if not region_files:
        raise TargetRunError(f"dimension contains no Anvil region files: {region_dir}")

    digest = hashlib.sha256()
    digest.update(WORLD_ID_POLICY.encode("ascii"))
    digest.update(b"\0")
    region_bytes = 0
    external_bytes = 0
    for path in region_files:
        region_bytes += _hash_world_member(digest, root, path)
    for path in external_files:
        external_bytes += _hash_world_member(digest, root, path)

    return {
        "policy": WORLD_ID_POLICY,
        "sha256": digest.hexdigest(),
        "region_files": len(region_files),
        "external_payload_files": len(external_files),
        "region_file_bytes": region_bytes,
        "external_payload_bytes": external_bytes,
        "total_bytes": region_bytes + external_bytes,
    }


def benchmark_command(
    cpu: str,
    dimension_root: Path,
    raw_output: Path,
    warmup_rounds: int,
    measured_rounds: int,
) -> list[str]:
    cpu = parse_cpu(cpu)
    warmup_rounds = parse_positive(warmup_rounds, "warmup_rounds")
    measured_rounds = parse_positive(measured_rounds, "measured_rounds")
    return [
        "taskset",
        "-c",
        cpu,
        "cargo",
        "run",
        "--release",
        "--locked",
        "--package",
        "helve-world-load-qualification",
        "--bin",
        "r2c_import_resident_bench",
        "--",
        "--world",
        str(dimension_root),
        "--warmup-rounds",
        str(warmup_rounds),
        "--measured-rounds",
        str(measured_rounds),
        "--require-single-cpu",
        "--output",
        str(raw_output),
    ]


def _require_summary(data: dict[str, Any], name: str) -> int:
    samples = data.get("samples_ns")
    if not isinstance(samples, dict):
        raise TargetRunError("benchmark artifact samples_ns is missing")
    summary = samples.get(name)
    if not isinstance(summary, dict):
        raise TargetRunError(f"benchmark artifact missing samples_ns.{name}")
    count = _nonnegative_int(summary.get("count"), f"samples_ns.{name}.count")
    p50 = _nonnegative_int(summary.get("p50"), f"samples_ns.{name}.p50")
    p95 = _nonnegative_int(summary.get("p95"), f"samples_ns.{name}.p95")
    p99 = _nonnegative_int(summary.get("p99"), f"samples_ns.{name}.p99")
    maximum = _nonnegative_int(summary.get("max"), f"samples_ns.{name}.max")
    if count == 0 or not (p50 <= p95 <= p99 <= maximum):
        raise TargetRunError(f"benchmark artifact has invalid samples_ns.{name}")
    return count


def annotate_artifact(
    data: dict[str, Any],
    cpu: str,
    world_identity: dict[str, Any],
    warmup_rounds: int,
    measured_rounds: int,
) -> dict[str, Any]:
    cpu = parse_cpu(cpu)
    warmup_rounds = parse_positive(warmup_rounds, "warmup_rounds")
    measured_rounds = parse_positive(measured_rounds, "measured_rounds")
    expected = {
        "schema": SCHEMA,
        "kind": BENCHMARK_KIND,
        "mode": RAW_MODE,
        "production_decision_eligible": False,
    }
    for field, value in expected.items():
        if data.get(field) != value:
            raise TargetRunError(
                f"benchmark artifact {field} must be {value!r}, got {data.get(field)!r}"
            )
    if data.get("profile") != EXPECTED_PROFILE:
        raise TargetRunError("benchmark artifact target profile drifted")
    if "target_qualification" in data or "input_world" in data:
        raise TargetRunError("benchmark artifact is already annotated with target evidence")

    hardware = data.get("hardware")
    if not isinstance(hardware, dict):
        raise TargetRunError("benchmark artifact hardware metadata is missing")
    observed_cpu = hardware.get("cpus_allowed_list")
    if observed_cpu != cpu:
        raise TargetRunError(
            f"benchmark did not remain pinned to requested CPU {cpu}; observed {observed_cpu!r}"
        )

    config = data.get("config")
    if not isinstance(config, dict):
        raise TargetRunError("benchmark artifact config is missing")
    expected_config = {
        "warmup_rounds": warmup_rounds,
        "measured_rounds": measured_rounds,
        "filesystem_io_timed": False,
        "dimension_setup_timed_separately": True,
        "round_excludes_dimension_setup": True,
    }
    for field, value in expected_config.items():
        if config.get(field) != value:
            raise TargetRunError(
                f"benchmark artifact config.{field} must be {value!r}, got {config.get(field)!r}"
            )

    world = data.get("world")
    if not isinstance(world, dict):
        raise TargetRunError("benchmark artifact world metadata is missing")
    region_files = _nonnegative_int(world.get("region_files"), "world.region_files")
    region_bytes = _nonnegative_int(world.get("region_file_bytes"), "world.region_file_bytes")
    chunks = _nonnegative_int(world.get("chunks"), "world.chunks")
    if region_files == 0 or chunks == 0:
        raise TargetRunError("benchmark artifact must contain region files and resident chunks")
    if region_files != world_identity.get("region_files"):
        raise TargetRunError("benchmark/world identity region-file count mismatch")
    if region_bytes != world_identity.get("region_file_bytes"):
        raise TargetRunError("benchmark/world identity region-byte count mismatch")

    scratch = data.get("scratch")
    if not isinstance(scratch, dict):
        raise TargetRunError("benchmark artifact scratch metadata is missing")
    if scratch.get("grew_during_measurement") is not False:
        raise TargetRunError("importer section scratch grew during target measurement")
    if scratch.get("decoder_grew_during_measurement") is not False:
        raise TargetRunError("decompression output scratch grew during target measurement")

    builder = data.get("builder")
    if not isinstance(builder, dict):
        raise TargetRunError("benchmark artifact builder accounting is missing")
    uniform_sections = _nonnegative_int(builder.get("uniform_sections"), "builder.uniform_sections")
    dense_sections = _nonnegative_int(builder.get("dense_sections"), "builder.dense_sections")
    dense_cells = _nonnegative_int(builder.get("dense_cells_copied"), "builder.dense_cells_copied")
    retained_cells = _nonnegative_int(
        builder.get("retained_cells_written"), "builder.retained_cells_written"
    )
    built_sections = uniform_sections + dense_sections
    if built_sections == 0:
        raise TargetRunError("benchmark artifact builder materialized no sections")
    if dense_cells != dense_sections * 4096:
        raise TargetRunError("benchmark artifact dense section copy accounting mismatch")
    if retained_cells != built_sections * 4096:
        raise TargetRunError("benchmark artifact retained section write accounting mismatch")

    counts = {name: _require_summary(data, name) for name in SUMMARY_NAMES}
    expected_counts = {
        "dimension_setup": measured_rounds,
        "region_open": region_files * measured_rounds,
        "import": chunks * measured_rounds,
        "install": chunks * measured_rounds,
        "whole_chunk": chunks * measured_rounds,
        "round": measured_rounds,
    }
    for name, expected_count in expected_counts.items():
        if counts[name] != expected_count:
            raise TargetRunError(
                f"benchmark artifact samples_ns.{name}.count must be {expected_count}, got {counts[name]}"
            )

    annotated = dict(data)
    annotated["input_world"] = dict(world_identity)
    annotated["target_qualification"] = {
        "schema": SCHEMA,
        "runner": RUNNER_ID,
        "explicit_operator_action": True,
        "single_cpu_requirement_enforced": True,
        "requested_cpu": cpu,
        "raw_benchmark_self_promoted": False,
        "hosted_ci_eligible": False,
    }
    return annotated


def run_target_process(
    cpu: str,
    dimension_root: Path,
    output: Path,
    warmup_rounds: int = DEFAULT_WARMUP_ROUNDS,
    measured_rounds: int = DEFAULT_MEASURED_ROUNDS,
    command_runner: Any = subprocess.run,
) -> None:
    cpu = parse_cpu(cpu)
    warmup_rounds = parse_positive(warmup_rounds, "warmup_rounds")
    measured_rounds = parse_positive(measured_rounds, "measured_rounds")
    dimension_root = dimension_root.resolve()
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        raise TargetRunError(f"refusing to overwrite existing target evidence: {output}")

    world_identity = dimension_world_identity(dimension_root)
    with tempfile.TemporaryDirectory(prefix="helve-r2c-import-resident-") as temporary:
        raw = Path(temporary) / "raw.json"
        command = benchmark_command(cpu, dimension_root, raw, warmup_rounds, measured_rounds)
        try:
            command_runner(command, check=True)
        except (OSError, subprocess.CalledProcessError) as error:
            raise TargetRunError(f"benchmark command failed: {error}") from error

        after_identity = dimension_world_identity(dimension_root)
        if after_identity != world_identity:
            raise TargetRunError("dimension world changed during target measurement")

        try:
            data = json.loads(raw.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise TargetRunError(f"benchmark did not produce valid JSON: {error}") from error
        if not isinstance(data, dict):
            raise TargetRunError("benchmark artifact must be a JSON object")
        annotated = annotate_artifact(
            data,
            cpu,
            world_identity,
            warmup_rounds,
            measured_rounds,
        )
        output.write_text(json.dumps(annotated, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cpu", required=True, help="single logical CPU index used by taskset")
    parser.add_argument(
        "--world",
        required=True,
        type=Path,
        help="Minecraft 26.2 dimension root containing region/ (for example dimensions/minecraft/overworld)",
    )
    parser.add_argument("--output", required=True, type=Path, help="new target evidence JSON path")
    parser.add_argument("--warmup-rounds", type=int, default=DEFAULT_WARMUP_ROUNDS)
    parser.add_argument("--measured-rounds", type=int, default=DEFAULT_MEASURED_ROUNDS)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        run_target_process(
            args.cpu,
            args.world,
            args.output,
            args.warmup_rounds,
            args.measured_rounds,
        )
    except TargetRunError as error:
        print(f"R2C import-resident target run rejected: {error}")
        return 2
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
