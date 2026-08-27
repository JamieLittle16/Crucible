#!/usr/bin/env python3
"""Admit the final unmodified-client R0 probe against a runnable Crucible commit."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Callable, Sequence

from tools.protocol_capture_admission import EvidenceConvergenceError, crosscheck_capture

SCHEMA = 1
KIND = "r0-external-client-probe-admission-v1"
OBSERVATION_SCHEMA = 1
OBSERVATION_KIND = "r0-unmodified-client-observation-v1"
EXPECTED_MINECRAFT = "26.2"
EXPECTED_PROTOCOL = 776
EXPECTED_CONTRACT_ID = "PROTO-NET-STATUS-26_2-001"
EXPECTED_ENDPOINT = "127.0.0.1:25566"
EXPECTED_CLIENT_DISTRIBUTION = "official"
EXPECTED_CLIENT_TO_SERVER_FRAMES = 3
EXPECTED_SERVER_TO_CLIENT_FRAMES = 2
EXPECTED_MATCHED_FRAMES = 5
HEX_40 = re.compile(r"^[0-9a-f]{40}$")
HEX_64 = re.compile(r"^[0-9a-f]{64}$")
SERVER_SESSION = re.compile(
    r'pub const R0_ADMISSION_SESSION_SHA256: &str =\s*"([0-9a-f]{64})";'
)
MAX_UI_EVIDENCE_BYTES = 32 * 1024 * 1024


class ExternalProbeError(ValueError):
    """Raised when the final R0 client probe cannot be admitted fail-closed."""


Runner = Callable[[Sequence[str], Path], subprocess.CompletedProcess[str]]
Convergence = Callable[..., dict[str, object]]


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
        raise ExternalProbeError(f"command failed ({' '.join(argv)}){detail}")
    return (result.stdout or "").strip()


def require_clean_commit(repo_root: Path, runner: Runner = _run) -> str:
    """Return exact HEAD after proving the probe checkout is the clean Git worktree root."""
    root = repo_root.resolve()
    top = _command_output(runner, root, ["git", "rev-parse", "--show-toplevel"])
    if Path(top).resolve() != root:
        raise ExternalProbeError("--repo-root must be the Git worktree root")
    status = _command_output(
        runner,
        root,
        ["git", "status", "--porcelain", "--untracked-files=all"],
    )
    if status:
        raise ExternalProbeError("external R0 probe requires a clean worktree")
    head = _command_output(runner, root, ["git", "rev-parse", "HEAD"])
    if HEX_40.fullmatch(head) is None:
        raise ExternalProbeError("Git HEAD is not a canonical SHA-1 identity")
    return head


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ExternalProbeError(f"{label} must be a JSON object")
    return value


def _keys(
    value: dict[str, Any], *, allowed: set[str], required: set[str], label: str
) -> None:
    unknown = sorted(set(value) - allowed)
    missing = sorted(required - set(value))
    if unknown:
        raise ExternalProbeError(f"{label} contains unknown keys: {', '.join(unknown)}")
    if missing:
        raise ExternalProbeError(f"{label} is missing required keys: {', '.join(missing)}")


def _read_json(path: Path, label: str) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise ExternalProbeError(f"{label} must be a real non-symlink file: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ExternalProbeError(f"could not read {label} {path}: {error}") from error
    return _object(value, label)


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ExternalProbeError(f"{label} must be a non-empty string")
    return value


def _sha256(value: object, label: str) -> str:
    digest = _string(value, label)
    if HEX_64.fullmatch(digest) is None:
        raise ExternalProbeError(f"{label} must be canonical lowercase SHA-256")
    return digest


def _exact_bool(value: object, expected: bool, label: str) -> None:
    if type(value) is not bool or value is not expected:
        state = "true" if expected else "false"
        raise ExternalProbeError(f"{label} must be exactly {state}")


def validate_observation(path: Path) -> dict[str, object]:
    """Validate the explicit operator-observed UI/client facts that TCP evidence cannot prove."""
    observation = _read_json(path, "client observation")
    required = {
        "schema",
        "kind",
        "minecraft",
        "client_distribution",
        "modified",
        "endpoint",
        "server_list_visible",
        "status_rendered",
        "ping_completed_without_protocol_error",
    }
    _keys(observation, allowed=required, required=required, label="client observation")
    if type(observation["schema"]) is not int or observation["schema"] != OBSERVATION_SCHEMA:
        raise ExternalProbeError("client observation has unsupported schema")
    if _string(observation["kind"], "client observation.kind") != OBSERVATION_KIND:
        raise ExternalProbeError("client observation has unsupported kind")
    if _string(observation["minecraft"], "client observation.minecraft") != EXPECTED_MINECRAFT:
        raise ExternalProbeError("client observation must identify Minecraft Java 26.2")
    if (
        _string(observation["client_distribution"], "client observation.client_distribution")
        != EXPECTED_CLIENT_DISTRIBUTION
    ):
        raise ExternalProbeError("R0 requires the official client distribution")
    _exact_bool(observation["modified"], False, "client observation.modified")
    if _string(observation["endpoint"], "client observation.endpoint") != EXPECTED_ENDPOINT:
        raise ExternalProbeError(f"client observation endpoint must be {EXPECTED_ENDPOINT}")
    _exact_bool(
        observation["server_list_visible"], True, "client observation.server_list_visible"
    )
    _exact_bool(observation["status_rendered"], True, "client observation.status_rendered")
    _exact_bool(
        observation["ping_completed_without_protocol_error"],
        True,
        "client observation.ping_completed_without_protocol_error",
    )
    return {key: observation[key] for key in sorted(required)}


def _read_admission(path: Path) -> dict[str, str]:
    admission = _read_json(path, "R0 status admission report")
    if type(admission.get("schema")) is not int or admission["schema"] != 1:
        raise ExternalProbeError("R0 status admission report has unsupported schema")
    if admission.get("kind") != "r0-status-admission-v1":
        raise ExternalProbeError("R0 status admission report has unsupported kind")
    session = _sha256(admission.get("session_sha256"), "R0 status admission session_sha256")
    contract = _object(admission.get("contract"), "R0 status admission contract")
    contract_id = _string(contract.get("id"), "R0 status admission contract.id")
    if contract_id != EXPECTED_CONTRACT_ID:
        raise ExternalProbeError("R0 status admission report names the wrong protocol contract")
    generated = _object(admission.get("generated_rust"), "R0 status admission generated_rust")
    generated_sha = _sha256(
        generated.get("sha256"), "R0 status admission generated_rust.sha256"
    )
    return {
        "session_sha256": session,
        "contract_id": contract_id,
        "generated_rust_sha256": generated_sha,
    }


def _server_session(repo_root: Path) -> str:
    path = repo_root / "crates/helve-server/src/lib.rs"
    if path.is_symlink() or not path.is_file():
        raise ExternalProbeError("current checkout does not contain the R0 Crucible server")
    text = path.read_text(encoding="utf-8")
    matches = SERVER_SESSION.findall(text)
    if len(matches) != 1:
        raise ExternalProbeError("could not uniquely resolve the server R0 admission session")
    return matches[0]


def _generated_digest(repo_root: Path) -> str:
    path = repo_root / "crates/network/helve-target-26-2/src/generated/status_26_2.rs"
    if path.is_symlink() or not path.is_file():
        raise ExternalProbeError("current checkout is missing generated Minecraft 26.2 packet facts")
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _ui_evidence(path: Path) -> dict[str, object]:
    if path.is_symlink() or not path.is_file():
        raise ExternalProbeError(f"UI evidence must be a real non-symlink file: {path}")
    size = path.stat().st_size
    if not 0 < size <= MAX_UI_EVIDENCE_BYTES:
        raise ExternalProbeError(
            f"UI evidence must contain 1..{MAX_UI_EVIDENCE_BYTES} bytes; got {size}"
        )
    raw = path.read_bytes()
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}


def _canonical_bytes(value: object) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n"
    ).encode("ascii")


def admit_external_probe(
    *,
    repo_root: Path,
    contract_path: Path,
    capture_path: Path,
    observation_path: Path,
    ui_evidence_path: Path,
    admission_path: Path,
    lock_path: Path,
    records_root: Path,
    runner: Runner = _run,
    convergence: Convergence = crosscheck_capture,
) -> dict[str, object]:
    """Return a deterministic report after all final R0 product/client evidence passes."""
    root = repo_root.resolve()
    commit = require_clean_commit(root, runner)
    observation = validate_observation(observation_path)
    admission = _read_admission(admission_path)

    server_session = _server_session(root)
    if server_session != admission["session_sha256"]:
        raise ExternalProbeError("server R0 session does not match admitted source/capture session")
    generated_sha = _generated_digest(root)
    if generated_sha != admission["generated_rust_sha256"]:
        raise ExternalProbeError("current generated 26.2 packet facts do not match R0 admission")

    try:
        convergence_summary = convergence(
            contract_path,
            capture_path,
            lock_path=lock_path,
            records_root=records_root,
        )
    except (EvidenceConvergenceError, OSError) as error:
        raise ExternalProbeError(f"Crucible client capture failed convergence: {error}") from error

    expected_summary = {
        "contract_id": EXPECTED_CONTRACT_ID,
        "minecraft": EXPECTED_MINECRAFT,
        "protocol": EXPECTED_PROTOCOL,
        "client_to_server_frames": EXPECTED_CLIENT_TO_SERVER_FRAMES,
        "server_to_client_frames": EXPECTED_SERVER_TO_CLIENT_FRAMES,
        "frames_matched": EXPECTED_MATCHED_FRAMES,
    }
    for key, expected in expected_summary.items():
        if convergence_summary.get(key) != expected:
            raise ExternalProbeError(
                f"Crucible client convergence {key} mismatch: expected {expected!r}, "
                f"got {convergence_summary.get(key)!r}"
            )
    capture_sha = _sha256(
        convergence_summary.get("capture_sha256"), "Crucible client convergence capture_sha256"
    )

    report: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "server_commit": commit,
        "admission_session_sha256": admission["session_sha256"],
        "generated_rust_sha256": generated_sha,
        "contract_id": EXPECTED_CONTRACT_ID,
        "minecraft": EXPECTED_MINECRAFT,
        "protocol": EXPECTED_PROTOCOL,
        "capture_sha256": capture_sha,
        "client_to_server_frames": EXPECTED_CLIENT_TO_SERVER_FRAMES,
        "server_to_client_frames": EXPECTED_SERVER_TO_CLIENT_FRAMES,
        "frames_matched": EXPECTED_MATCHED_FRAMES,
        "observation": observation,
        "observation_sha256": hashlib.sha256(_canonical_bytes(observation)).hexdigest(),
        "ui_evidence": _ui_evidence(ui_evidence_path),
        "admitted": True,
    }
    report["report_sha256"] = hashlib.sha256(_canonical_bytes(report)).hexdigest()
    return report


def write_report(path: Path, report: dict[str, object]) -> None:
    if path.exists() and path.is_symlink():
        raise ExternalProbeError(f"refusing to replace symlink output: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(_canonical_bytes(report))


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path("."))
    parser.add_argument(
        "--contract",
        type=Path,
        default=Path("vanilla/protocol/PROTO-NET-STATUS-26_2-001.json"),
    )
    parser.add_argument("--capture", type=Path, required=True)
    parser.add_argument("--observation", type=Path, required=True)
    parser.add_argument("--ui-evidence", type=Path, required=True)
    parser.add_argument(
        "--admission",
        type=Path,
        default=Path("vanilla/reports/r0-status-admission-26.2.json"),
    )
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--records-root", type=Path, default=Path("vanilla/records"))
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    try:
        report = admit_external_probe(
            repo_root=args.repo_root,
            contract_path=args.contract,
            capture_path=args.capture,
            observation_path=args.observation,
            ui_evidence_path=args.ui_evidence,
            admission_path=args.admission,
            lock_path=args.lock,
            records_root=args.records_root,
        )
        write_report(args.output, report)
    except (ExternalProbeError, OSError, UnicodeError) as error:
        print(f"R0 external probe admission error: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "kind": KIND,
                "server_commit": report["server_commit"],
                "capture_sha256": report["capture_sha256"],
                "report_sha256": report["report_sha256"],
                "output": str(args.output),
            },
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
