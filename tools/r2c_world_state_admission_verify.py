#!/usr/bin/env python3
"""Verify committed R2C world-state source-admission evidence without official source access.

This verifier is intentionally source-free. Once the real admitted bundle exists in the repository,
CI can use its content-addressed manifest to detect any later drift in promoted VAR, SEM, gate, or
source-gate report bytes. Re-running the official-source gate is still required when the source pin,
semantic contract, selected source set, or fingerprints change.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path, PurePosixPath
from typing import Sequence

try:
    from . import r2c_world_state_admission_promote as promote
    from . import r2c_world_state_source_review_finalize as review
except ImportError:  # Direct ``python3 tools/...`` execution.
    import r2c_world_state_admission_promote as promote  # type: ignore[no-redef]
    import r2c_world_state_source_review_finalize as review  # type: ignore[no-redef]


class VerifyError(RuntimeError):
    """Fail-closed committed-admission verification error."""


def _read(path: Path, label: str) -> bytes:
    if path.is_symlink() or not path.is_file():
        raise VerifyError(f"{label} must be a real non-symlink file: {path}")
    try:
        return path.read_bytes()
    except OSError as error:
        raise VerifyError(f"cannot read {label}: {error}") from error


def _json(path: Path, label: str) -> tuple[dict[str, object], bytes]:
    raw = _read(path, label)
    try:
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise VerifyError(f"cannot decode {label}: {error}") from error
    if not isinstance(value, dict):
        raise VerifyError(f"{label} must be a JSON object")
    return value, raw


def _sha(value: object, label: str) -> str:
    if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
        raise VerifyError(f"{label} must be lowercase SHA-256")
    return value


def _safe_path(value: object, label: str) -> PurePosixPath:
    if not isinstance(value, str) or not value:
        raise VerifyError(f"{label} must be a non-empty path")
    path = PurePosixPath(value)
    if path.is_absolute() or value != path.as_posix() or any(part in {"", ".", ".."} for part in path.parts):
        raise VerifyError(f"{label} must be a canonical safe relative POSIX path")
    return path


def _allowed_path(path: PurePosixPath) -> bool:
    if path in {promote.SEMANTICS_PATH, promote.GATE_PATH, promote.REPORT_PATH}:
        return True
    if path.parent != promote.RECORD_ROOT:
        return False
    return path.name.startswith("VAR-") and path.suffix == ".json"


def verify_repository(repo_root: Path) -> dict[str, object]:
    if repo_root.is_symlink() or not repo_root.is_dir():
        raise VerifyError(f"repository root must be a real non-symlink directory: {repo_root}")
    manifest_path = repo_root.joinpath(*promote.MANIFEST_PATH.parts)
    manifest, manifest_raw = _json(manifest_path, "R2C admitted-bundle manifest")

    expected = {
        "schema": promote.SCHEMA,
        "kind": promote.KIND,
        "id": promote.ID,
        "commit_policy": promote.COMMIT_POLICY,
        "source_archive_sha256": review.EXPECTED_SOURCE_SHA256,
        "gate_id": promote.materialize.GATE_ID,
        "contains_official_source_text": False,
        "source_admitted": True,
        "production_implementation_authorized": True,
        "runtime_behavior_implemented": False,
    }
    mismatches = {key: {"expected": wanted, "actual": manifest.get(key)} for key, wanted in expected.items() if manifest.get(key) != wanted}
    if mismatches:
        raise VerifyError(f"admitted-bundle manifest identity mismatch: {json.dumps(mismatches, sort_keys=True)}")

    for field in (
        "materialization_manifest_sha256",
        "review_result_sha256",
        "admission_worksheet_sha256",
        "gate_sha256",
        "source_gate_report_sha256",
    ):
        _sha(manifest.get(field), f"manifest.{field}")
    var_records = manifest.get("var_records")
    semantic_rules = manifest.get("semantic_rules")
    if type(var_records) is not int or var_records < 1 or type(semantic_rules) is not int or semantic_rules < 1:
        raise VerifyError("admitted-bundle manifest must contain positive VAR and SEM counts")

    raw_files = manifest.get("files")
    if not isinstance(raw_files, list):
        raise VerifyError("manifest.files must be an array")
    seen: set[PurePosixPath] = set()
    record_count = 0
    report_digest = None
    gate_digest = None
    for index, raw_entry in enumerate(raw_files):
        if not isinstance(raw_entry, dict) or set(raw_entry) != {"path", "size", "sha256"}:
            raise VerifyError(f"manifest.files[{index}] fields are not canonical")
        relative = _safe_path(raw_entry["path"], f"manifest.files[{index}].path")
        if not _allowed_path(relative):
            raise VerifyError(f"manifest contains non-canonical promotion path: {relative}")
        if relative in seen:
            raise VerifyError(f"manifest contains duplicate path: {relative}")
        seen.add(relative)
        size = raw_entry["size"]
        if type(size) is not int or size < 0:
            raise VerifyError(f"manifest.files[{index}].size is invalid")
        expected_digest = _sha(raw_entry["sha256"], f"manifest.files[{index}].sha256")
        path = repo_root.joinpath(*relative.parts)
        raw = _read(path, f"promoted file {relative}")
        if len(raw) != size or promote._sha256(raw) != expected_digest:
            raise VerifyError(f"promoted file drift: {relative}")
        if relative.parent == promote.RECORD_ROOT:
            record_count += 1
        elif relative == promote.REPORT_PATH:
            report_digest = expected_digest
        elif relative == promote.GATE_PATH:
            gate_digest = expected_digest

    required_non_records = {promote.SEMANTICS_PATH, promote.GATE_PATH, promote.REPORT_PATH}
    if not required_non_records.issubset(seen):
        raise VerifyError("admitted-bundle manifest is missing canonical non-record artifacts")
    if record_count != var_records or len(seen) != var_records + len(required_non_records):
        raise VerifyError("admitted-bundle manifest VAR/file counts disagree")
    if report_digest != manifest["source_gate_report_sha256"]:
        raise VerifyError("source-gate report digest does not match admitted manifest binding")
    if gate_digest != manifest["gate_sha256"]:
        raise VerifyError("gate digest does not match admitted manifest binding")

    return {
        "id": promote.ID,
        "manifest_sha256": promote._sha256(manifest_raw),
        "var_records": var_records,
        "semantic_rules": semantic_rules,
        "source_admitted": True,
        "runtime_behavior_implemented": False,
        "verified": True,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = verify_repository(args.repo_root)
    except VerifyError as error:
        print(f"R2C world-state admitted-bundle verification failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
