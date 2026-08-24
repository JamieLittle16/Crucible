#!/usr/bin/env python3
"""Seal existing Crucible full benchmark artifacts into one target-hardware session.

This tool does not compare mechanisms or make performance decisions. It executes the existing full
benchmark harnesses from one clean commit, validates their semantic/evidence guards, retains every
raw artifact byte-for-byte, and emits a content-addressed session manifest.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Mapping, Sequence

SCHEMA = 1
MANIFEST_NAME = "session-manifest.json"
SIDECAR_NAME = "session-manifest.sha256"

# These describe transient placement/state and are retained in each raw artifact rather than
# required to match across sequential runs in one physical-machine session.
DYNAMIC_HARDWARE_FIELDS = frozenset({"cpu_current_khz", "load_average", "cpus_allowed_list"})


class SessionError(RuntimeError):
    """Raised when a controlled-hardware session cannot be admitted fail-closed."""


@dataclass(frozen=True)
class BenchmarkSpec:
    key: str
    benchmark: str
    filename: str
    package: str
    binary: str

    def argv(self, output: str) -> list[str]:
        return [
            "cargo",
            "run",
            "--release",
            "--locked",
            "--package",
            self.package,
            "--bin",
            self.binary,
            "--",
            "--full",
            "--output",
            output,
        ]


BENCHMARKS = (
    BenchmarkSpec(
        key="composition_hot",
        benchmark="composition-hot-tax",
        filename="composition-hot.json",
        package="crucible-composition-qualification",
        binary="composition_hot_bench",
    ),
    BenchmarkSpec(
        key="world_access",
        benchmark="resolved-chunk-window",
        filename="world-access.json",
        package="crucible-world-access-qualification",
        binary="world_access_bench",
    ),
    BenchmarkSpec(
        key="executor_baseline",
        benchmark="executor-worker-memory-baseline",
        filename="executor-baseline.json",
        package="crucible-executor-baseline",
        binary="executor_baseline_bench",
    ),
    BenchmarkSpec(
        key="fused_outbound",
        benchmark="fused-outbound-construction",
        filename="fused-outbound.json",
        package="crucible-client-spine-qualification",
        binary="fused_outbound_bench",
    ),
)

Runner = Callable[[Sequence[str], Path], subprocess.CompletedProcess[str]]


def _run(argv: Sequence[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(argv),
        cwd=cwd,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def _command_output(runner: Runner, cwd: Path, argv: Sequence[str]) -> str:
    result = runner(argv, cwd)
    if result.returncode != 0:
        stderr = (result.stderr or "").strip()
        detail = f": {stderr}" if stderr else ""
        raise SessionError(f"command failed ({' '.join(argv)}){detail}")
    return (result.stdout or "").strip()


def require_clean_repository(repo_root: Path, runner: Runner = _run) -> str:
    """Return exact HEAD after proving `repo_root` is a clean Git worktree."""
    repo_root = repo_root.resolve()
    top = _command_output(runner, repo_root, ["git", "rev-parse", "--show-toplevel"])
    if Path(top).resolve() != repo_root:
        raise SessionError("--repo-root must be the Git worktree root")
    status = _command_output(
        runner,
        repo_root,
        ["git", "status", "--porcelain", "--untracked-files=all"],
    )
    if status:
        raise SessionError("target-hardware session requires a clean worktree")
    head = _command_output(runner, repo_root, ["git", "rev-parse", "HEAD"])
    if len(head) != 40 or any(character not in "0123456789abcdef" for character in head):
        raise SessionError("Git HEAD is not a canonical SHA-1 identity")
    return head


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise SessionError(f"{label} must be a JSON object")
    return value


def _nonempty_list(value: object, label: str) -> list[Any]:
    if not isinstance(value, list) or not value:
        raise SessionError(f"{label} must be a non-empty JSON array")
    return value


def _require_common(payload: Mapping[str, Any], spec: BenchmarkSpec, head: str) -> dict[str, Any]:
    if type(payload.get("schema")) is not int or payload["schema"] != 1:
        raise SessionError(f"{spec.key}: unsupported artifact schema")
    if payload.get("benchmark") != spec.benchmark:
        raise SessionError(f"{spec.key}: wrong benchmark identity")
    if payload.get("mode") != "full":
        raise SessionError(f"{spec.key}: target-hardware session requires full mode")
    if payload.get("hosted_ci_is_diagnostic_only") is not True:
        raise SessionError(f"{spec.key}: missing hosted-CI evidence disclaimer")
    hardware = _object(payload.get("hardware"), f"{spec.key}.hardware")
    if hardware.get("commit_sha") != head:
        raise SessionError(f"{spec.key}: embedded commit does not match session HEAD")
    return hardware


def _positive_int(value: object, label: str) -> int:
    if type(value) is not int or value <= 0:
        raise SessionError(f"{label} must be a positive integer")
    return value


def validate_artifact(spec: BenchmarkSpec, payload: Mapping[str, Any], head: str) -> dict[str, Any]:
    """Validate one full benchmark artifact and return its embedded hardware object."""
    hardware = _require_common(payload, spec, head)

    if spec.key == "composition_hot":
        structural = _object(payload.get("structural"), "composition_hot.structural")
        if structural.get("exact_type_identity") is not True:
            raise SessionError("composition_hot: generated provider is not exact hand-wired type")
        _nonempty_list(payload.get("paired_rounds"), "composition_hot.paired_rounds")
        _positive_int(payload.get("semantic_checksum"), "composition_hot.semantic_checksum")
    elif spec.key == "world_access":
        cases = _nonempty_list(payload.get("cases"), "world_access.cases")
        for index, raw_case in enumerate(cases):
            case = _object(raw_case, f"world_access.cases[{index}]")
            _positive_int(case.get("semantic_checksum"), f"world_access.cases[{index}].semantic_checksum")
            _nonempty_list(case.get("paired_rounds"), f"world_access.cases[{index}].paired_rounds")
            _nonempty_list(
                case.get("setup_samples_ns"), f"world_access.cases[{index}].setup_samples_ns"
            )
            _nonempty_list(
                case.get("whole_cost_samples"), f"world_access.cases[{index}].whole_cost_samples"
            )
    elif spec.key == "executor_baseline":
        reference = _object(payload.get("semantic_reference"), "executor_baseline.semantic_reference")
        _positive_int(reference.get("stage_count"), "executor_baseline.stage_count")
        _positive_int(reference.get("useful_operations"), "executor_baseline.useful_operations")
        _positive_int(reference.get("work_checksum"), "executor_baseline.work_checksum")
        candidates = _nonempty_list(payload.get("candidates"), "executor_baseline.candidates")
        workers = [
            _object(candidate, "executor candidate").get("workers") for candidate in candidates
        ]
        if workers != [1, 2, 4]:
            raise SessionError("executor_baseline: candidates must be exactly 1/2/4 workers")
        _nonempty_list(payload.get("rounds"), "executor_baseline.rounds")
    elif spec.key == "fused_outbound":
        if payload.get("production_path_unchanged") is not True:
            raise SessionError("fused_outbound: production path changed during experiment")
        cases = _nonempty_list(payload.get("cases"), "fused_outbound.cases")
        for index, raw_case in enumerate(cases):
            case = _object(raw_case, f"fused_outbound.cases[{index}]")
            if case.get("byte_equivalent") is not True:
                raise SessionError(f"fused_outbound.cases[{index}]: byte equivalence failed")
            _positive_int(case.get("semantic_checksum"), f"fused_outbound.cases[{index}].semantic_checksum")
            _nonempty_list(case.get("paired_rounds"), f"fused_outbound.cases[{index}].paired_rounds")
    else:  # pragma: no cover - BENCHMARKS is closed in this module.
        raise SessionError(f"unknown benchmark spec: {spec.key}")

    return hardware


def stable_hardware_identity(hardware: Mapping[str, Any]) -> dict[str, Any]:
    """Return fields expected to remain invariant across sequential harnesses on one machine."""
    return {
        key: hardware[key]
        for key in sorted(hardware)
        if key not in DYNAMIC_HARDWARE_FIELDS and key != "commit_sha"
    }


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_json_bytes(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode(
        "utf-8"
    )


def _parse_artifact(path: Path, spec: BenchmarkSpec) -> tuple[dict[str, Any], bytes]:
    if path.is_symlink() or not path.is_file():
        raise SessionError(f"{spec.key}: benchmark did not create a real artifact file")
    raw = path.read_bytes()
    try:
        payload = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise SessionError(f"{spec.key}: artifact is not valid UTF-8 JSON: {error}") from error
    return _object(payload, f"{spec.key} artifact"), raw


def build_manifest(
    *,
    head: str,
    stable_hardware: Mapping[str, Any],
    artifacts: Sequence[Mapping[str, Any]],
) -> dict[str, Any]:
    return {
        "schema": SCHEMA,
        "session_kind": "m0-controlled-hardware-evidence",
        "commit_sha": head,
        "decision_made": False,
        "decision_policy": "raw subsystem artifacts only; this session does not select mechanisms",
        "stable_hardware_identity": dict(stable_hardware),
        "artifacts": list(artifacts),
    }


def run_session(
    *,
    repo_root: Path,
    output_dir: Path,
    runner: Runner = _run,
    environment: Mapping[str, str] | None = None,
) -> tuple[Path, str]:
    """Run and seal the complete controlled-hardware session."""
    env = os.environ if environment is None else environment
    if env.get("GITHUB_ACTIONS", "").lower() == "true":
        raise SessionError("authoritative target-hardware sessions are forbidden in GitHub Actions")

    repo_root = repo_root.resolve()
    if output_dir.is_absolute() or ".." in output_dir.parts:
        raise SessionError("--output-dir must be a repository-relative path without '..'")
    output_root = (repo_root / output_dir).resolve()
    try:
        output_root.relative_to(repo_root)
    except ValueError as error:  # defensive if Path semantics change.
        raise SessionError("--output-dir escapes repository root") from error
    if output_root.exists():
        raise SessionError("session output directory already exists; evidence is never overwritten")

    head = require_clean_repository(repo_root, runner)
    output_root.mkdir(parents=True)

    records: list[dict[str, Any]] = []
    expected_stable: dict[str, Any] | None = None
    try:
        for spec in BENCHMARKS:
            relative_artifact = output_dir / spec.filename
            artifact_path = repo_root / relative_artifact
            argv = spec.argv(relative_artifact.as_posix())
            result = runner(argv, repo_root)
            if result.returncode != 0:
                stderr = (result.stderr or "").strip()
                detail = f": {stderr}" if stderr else ""
                raise SessionError(f"{spec.key}: benchmark command failed{detail}")

            payload, raw = _parse_artifact(artifact_path, spec)
            hardware = validate_artifact(spec, payload, head)
            stable = stable_hardware_identity(hardware)
            if expected_stable is None:
                expected_stable = stable
            elif stable != expected_stable:
                raise SessionError(f"{spec.key}: stable machine/toolchain identity changed mid-session")

            records.append(
                {
                    "key": spec.key,
                    "benchmark": spec.benchmark,
                    "path": spec.filename,
                    "sha256": sha256_bytes(raw),
                    "bytes": len(raw),
                    "argv": argv,
                }
            )

        if expected_stable is None:  # pragma: no cover - BENCHMARKS is non-empty.
            raise SessionError("session contains no benchmarks")
        manifest = build_manifest(
            head=head,
            stable_hardware=expected_stable,
            artifacts=records,
        )
        manifest_bytes = canonical_json_bytes(manifest)
        manifest_path = output_root / MANIFEST_NAME
        manifest_path.write_bytes(manifest_bytes)
        digest = sha256_bytes(manifest_bytes)
        (output_root / SIDECAR_NAME).write_text(f"{digest}  {MANIFEST_NAME}\n", encoding="ascii")
        return manifest_path, digest
    except Exception:
        # Partial raw evidence is useful for diagnosing the failed experiment, but a failed session
        # must not look sealed. No manifest/sidecar is written before every artifact validates.
        raise


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path("."))
    parser.add_argument(
        "--output-dir",
        type=Path,
        required=True,
        help="new repository-relative directory, normally below target/qualification/",
    )
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    try:
        manifest, digest = run_session(repo_root=args.repo_root, output_dir=args.output_dir)
    except (SessionError, OSError) as error:
        print(f"target-hardware session error: {error}", file=sys.stderr)
        return 1
    print(f"sealed {manifest}: sha256={digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
