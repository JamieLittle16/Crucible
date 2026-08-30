#!/usr/bin/env python3
"""Promote an independently admitted R2C world-state bundle into canonical repository paths.

This step is source-free and performs no semantic inference. It consumes the exact external staging
bundle produced by ``r2c_world_state_admission_materialize.py`` plus the exact JSON report emitted by
``vanilla_source_gate.py``. Promotion is allowed only when the report admits the byte-identical staged
gate and every byte-identical staged VAR record against the pinned Minecraft 26.2 source identity.

All validation and destination-collision checks happen before the first repository file is written.
The staged VAR records, semantic Markdown, gate, and source-gate report are copied verbatim. Only the
content-addressed admitted-bundle manifest is newly generated here.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path, PurePosixPath
from typing import Mapping, Sequence

try:
    from . import r2c_world_state_admission_materialize as materialize
    from . import r2c_world_state_source_review_finalize as review
except ImportError:  # Direct ``python3 tools/...`` execution.
    import r2c_world_state_admission_materialize as materialize  # type: ignore[no-redef]
    import r2c_world_state_source_review_finalize as review  # type: ignore[no-redef]

SCHEMA = 1
KIND = "r2c-world-state-admitted-bundle"
ID = "ADMIT-NET-R2C-WORLD-STATE-26_2-001"
COMMIT_POLICY = "SOURCE_GATE_ADMITTED_SOURCE_FREE_REPOSITORY_PROMOTION"
RECORD_ROOT = PurePosixPath("vanilla/records/network/r2/world-state")
SEMANTICS_PATH = PurePosixPath("vanilla/semantics/network/R2C_WORLD_STATE_SEMANTICS.md")
GATE_PATH = PurePosixPath("vanilla/gates/network/GATE-NET-R2C-WORLD-STATE-26_2-001.json")
REPORT_PATH = PurePosixPath("vanilla/reports/r2c-world-state-source-admission-26.2.json")
MANIFEST_PATH = PurePosixPath("vanilla/reports/r2c-world-state-admitted-bundle-manifest.json")
EXPECTED_FINGERPRINT = "java-token-v2-literal-sensitive"
EXPECTED_MINECRAFT = "26.2"
EXPECTED_PROTOCOL = "776"
EXPECTED_WORLD_VERSION = "4903"


class PromoteError(RuntimeError):
    """Fail-closed world-state admission promotion error."""


def _pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _read_bytes(path: Path, label: str) -> bytes:
    if path.is_symlink() or not path.is_file():
        raise PromoteError(f"{label} must be a real non-symlink file: {path}")
    try:
        return path.read_bytes()
    except OSError as error:
        raise PromoteError(f"cannot read {label}: {error}") from error


def _read_json(path: Path, label: str) -> tuple[dict[str, object], bytes]:
    raw = _read_bytes(path, label)
    try:
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PromoteError(f"cannot decode {label}: {error}") from error
    if not isinstance(value, dict):
        raise PromoteError(f"{label} must be a JSON object")
    return value, raw


def _object(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise PromoteError(f"{label} must be an object")
    return value


def _array(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise PromoteError(f"{label} must be an array")
    return value


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise PromoteError(f"{label} must be a non-empty string")
    return value


def _lower_sha(value: object, label: str) -> str:
    digest = _string(value, label)
    if len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
        raise PromoteError(f"{label} must be lowercase SHA-256")
    return digest


def _safe_relative(value: object, label: str) -> PurePosixPath:
    raw = _string(value, label)
    path = PurePosixPath(raw)
    if path.is_absolute() or raw != path.as_posix() or not path.parts or any(part in {"", ".", ".."} for part in path.parts):
        raise PromoteError(f"{label} must be a canonical safe relative POSIX path")
    return path


def _validate_staging(staging_dir: Path) -> tuple[dict[str, bytes], bytes, dict[str, object]]:
    if staging_dir.is_symlink() or not staging_dir.is_dir():
        raise PromoteError(f"staging directory must be a real non-symlink directory: {staging_dir}")
    manifest, manifest_raw = _read_json(staging_dir / "manifest.json", "materialization manifest")
    expected = {
        "schema": SCHEMA,
        "kind": materialize.KIND,
        "id": materialize.ID,
        "commit_policy": materialize.COMMIT_POLICY,
        "source_archive_sha256": review.EXPECTED_SOURCE_SHA256,
        "contains_official_source_text": False,
        "gate_id": materialize.GATE_ID,
        "independent_gate_required": True,
        "production_admitted": False,
    }
    mismatches = {key: {"expected": wanted, "actual": manifest.get(key)} for key, wanted in expected.items() if manifest.get(key) != wanted}
    if mismatches:
        raise PromoteError(f"materialization manifest identity mismatch: {json.dumps(mismatches, sort_keys=True)}")
    _lower_sha(manifest.get("review_result_sha256"), "manifest.review_result_sha256")
    _lower_sha(manifest.get("admission_worksheet_sha256"), "manifest.admission_worksheet_sha256")
    var_count = manifest.get("var_records")
    rule_count = manifest.get("semantic_rules")
    if type(var_count) is not int or var_count < 1 or type(rule_count) is not int or rule_count < 1:
        raise PromoteError("materialization manifest must contain positive VAR and SEM counts")

    entries = _array(manifest.get("files"), "manifest.files")
    files: dict[str, bytes] = {}
    for index, raw_entry in enumerate(entries):
        entry = _object(raw_entry, f"manifest.files[{index}]")
        if set(entry) != {"path", "size", "sha256"}:
            raise PromoteError(f"manifest.files[{index}] fields are not canonical")
        relative = _safe_relative(entry["path"], f"manifest.files[{index}].path")
        relative_text = relative.as_posix()
        if relative_text in files:
            raise PromoteError(f"duplicate staged manifest path: {relative_text}")
        size = entry["size"]
        if type(size) is not int or size < 0:
            raise PromoteError(f"manifest.files[{index}].size must be a non-negative integer")
        digest = _lower_sha(entry["sha256"], f"manifest.files[{index}].sha256")
        path = staging_dir.joinpath(*relative.parts)
        raw = _read_bytes(path, f"staged file {relative_text}")
        if len(raw) != size or _sha256(raw) != digest:
            raise PromoteError(f"staged file differs from materialization manifest: {relative_text}")
        files[relative_text] = raw

    record_paths = sorted(path for path in files if path.startswith("records/") and path.endswith(".json"))
    expected_paths = set(record_paths) | {f"semantics/{materialize.SEMANTICS_FILE}", "gate.json"}
    if set(files) != expected_paths or len(record_paths) != var_count:
        raise PromoteError("staged file set/count differs from canonical materialization layout")

    actual_files: set[str] = set()
    for path in staging_dir.rglob("*"):
        if path.is_symlink():
            raise PromoteError(f"staging bundle contains a symlink: {path}")
        if path.is_file():
            actual_files.add(path.relative_to(staging_dir).as_posix())
    if actual_files != set(files) | {"manifest.json"}:
        extra = sorted(actual_files - set(files) - {"manifest.json"})
        missing = sorted((set(files) | {"manifest.json"}) - actual_files)
        raise PromoteError(f"staging bundle file inventory mismatch: extra={extra} missing={missing}")

    return files, manifest_raw, manifest


def _record_identity(raw: bytes, label: str) -> tuple[str, dict[str, object]]:
    try:
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PromoteError(f"cannot decode {label}: {error}") from error
    record = _object(value, label)
    var_id = _string(record.get("id"), f"{label}.id")
    if record.get("schema") != 1 or record.get("status") != "VAR_REVIEWED":
        raise PromoteError(f"{label} is not a canonical reviewed VAR record")
    source = _object(record.get("source"), f"{label}.source")
    if source.get("fingerprint_algorithm") != EXPECTED_FINGERPRINT:
        raise PromoteError(f"{label} fingerprint algorithm mismatch")
    _lower_sha(source.get("normalized_sha256"), f"{label}.source.normalized_sha256")
    _lower_sha(source.get("body_sha256"), f"{label}.source.body_sha256")
    semantic = record.get("semantic_rules")
    hazards = record.get("hazards_reviewed")
    if not isinstance(semantic, list) or not semantic or any(not isinstance(item, str) or not item for item in semantic):
        raise PromoteError(f"{label}.semantic_rules must contain non-empty rule ids")
    if not isinstance(hazards, list) or any(not isinstance(item, str) or not item for item in hazards):
        raise PromoteError(f"{label}.hazards_reviewed must contain strings")
    return var_id, record


def _validate_gate_report(report: Mapping[str, object], report_raw: bytes, staged: Mapping[str, bytes]) -> dict[str, dict[str, object]]:
    if report.get("schema") != 1 or report.get("gate_id") != materialize.GATE_ID:
        raise PromoteError("source-gate report identity mismatch")
    if report.get("admitted") is not True or report.get("failures") != []:
        raise PromoteError("source-gate report is not admitted cleanly")
    if report.get("minimum_status") != "VAR_REVIEWED":
        raise PromoteError("source-gate report minimum status mismatch")
    if report.get("gate_sha256") != _sha256(staged["gate.json"]):
        raise PromoteError("source-gate report was not evaluated against the exact staged gate")

    source = _object(report.get("source"), "source-gate report source")
    expected_source = {
        "minecraft_version": EXPECTED_MINECRAFT,
        "protocol_version": EXPECTED_PROTOCOL,
        "world_version": EXPECTED_WORLD_VERSION,
        "archive_sha256": review.EXPECTED_SOURCE_SHA256,
        "fingerprint_algorithm": EXPECTED_FINGERPRINT,
    }
    for key, expected in expected_source.items():
        if str(source.get(key)) != expected:
            raise PromoteError(f"source-gate report source.{key} mismatch")
    _string(source.get("atlas_version"), "source-gate report source.atlas_version")
    _string(source.get("schema_version"), "source-gate report source.schema_version")

    frontier = _object(report.get("frontier"), "source-gate report frontier")
    if frontier.get("name") != materialize.FRONTIER:
        raise PromoteError("source-gate report frontier mismatch")
    _lower_sha(frontier.get("config_sha256"), "source-gate report frontier.config_sha256")

    staged_records: dict[str, tuple[bytes, dict[str, object]]] = {}
    for path, raw in staged.items():
        if not path.startswith("records/"):
            continue
        var_id, record = _record_identity(raw, path)
        if var_id in staged_records:
            raise PromoteError(f"duplicate staged VAR id: {var_id}")
        staged_records[var_id] = (raw, record)

    required = _array(report.get("required_methods"), "source-gate report required_methods")
    admitted: dict[str, dict[str, object]] = {}
    for index, raw_method in enumerate(required):
        method = _object(raw_method, f"required_methods[{index}]")
        var_id = _string(method.get("var_id"), f"required_methods[{index}].var_id")
        if var_id in admitted:
            raise PromoteError(f"duplicate admitted VAR id: {var_id}")
        staged_entry = staged_records.get(var_id)
        if staged_entry is None:
            raise PromoteError(f"source-gate report admits a VAR absent from staging: {var_id}")
        raw_record, record = staged_entry
        if method.get("record_sha256") != _sha256(raw_record):
            raise PromoteError(f"source-gate report record digest mismatch for {var_id}")
        if method.get("status") != "VAR_REVIEWED":
            raise PromoteError(f"source-gate report status mismatch for {var_id}")
        source_record = _object(record.get("source"), f"{var_id}.source")
        source_identity = f"{source_record.get('type')}#{source_record.get('signature')}"
        if method.get("source") != source_identity:
            raise PromoteError(f"source-gate report source identity mismatch for {var_id}")
        if method.get("normalized_sha256") != source_record.get("normalized_sha256") or method.get("body_sha256") != source_record.get("body_sha256"):
            raise PromoteError(f"source-gate report source fingerprints mismatch for {var_id}")
        for report_key, record_key in (("semantic_rules", "semantic_rules"), ("reviewed_hazards", "hazards_reviewed")):
            observed = method.get(report_key)
            expected = record.get(record_key)
            if not isinstance(observed, list) or sorted(str(item) for item in observed) != sorted(str(item) for item in expected if isinstance(expected, list)):
                raise PromoteError(f"source-gate report {report_key} mismatch for {var_id}")
        observed_hazards = method.get("observed_hazards")
        reviewed_hazards = method.get("reviewed_hazards")
        if not isinstance(observed_hazards, list) or not isinstance(reviewed_hazards, list) or not set(map(str, observed_hazards)).issubset(set(map(str, reviewed_hazards))):
            raise PromoteError(f"source-gate report hazard closure mismatch for {var_id}")
        admitted[var_id] = method

    if set(admitted) != set(staged_records):
        raise PromoteError("source-gate report did not admit exactly the staged VAR set")
    if _sha256(report_raw) == _sha256(b""):
        raise PromoteError("unreachable source-gate report digest state")
    return admitted


def _destination_files(staged: Mapping[str, bytes], report_raw: bytes) -> dict[PurePosixPath, bytes]:
    result: dict[PurePosixPath, bytes] = {
        SEMANTICS_PATH: staged[f"semantics/{materialize.SEMANTICS_FILE}"],
        GATE_PATH: staged["gate.json"],
        REPORT_PATH: report_raw,
    }
    for path, raw in staged.items():
        if path.startswith("records/"):
            name = PurePosixPath(path).name
            result[RECORD_ROOT / name] = raw
    return result


def promote(staging_dir: Path, gate_report: Path, repo_root: Path) -> dict[str, object]:
    staged, materialization_manifest_raw, materialization_manifest = _validate_staging(staging_dir)
    report, report_raw = _read_json(gate_report, "source-gate report")
    admitted = _validate_gate_report(report, report_raw, staged)

    if repo_root.is_symlink() or not repo_root.is_dir():
        raise PromoteError(f"repository root must be a real non-symlink directory: {repo_root}")
    if not (repo_root / "vanilla").is_dir() or not (repo_root / "tools").is_dir():
        raise PromoteError("repository root does not contain the expected vanilla/ and tools/ boundaries")

    destination = _destination_files(staged, report_raw)
    for relative in list(destination) + [MANIFEST_PATH]:
        path = repo_root.joinpath(*relative.parts)
        if path.exists() or path.is_symlink():
            raise PromoteError(f"promotion refuses to overwrite existing repository path: {relative}")
        cursor = path.parent
        while cursor != repo_root and cursor != cursor.parent:
            if cursor.exists() and cursor.is_symlink():
                raise PromoteError(f"promotion destination parent is a symlink: {cursor}")
            cursor = cursor.parent

    file_entries = [
        {"path": path.as_posix(), "size": len(raw), "sha256": _sha256(raw)}
        for path, raw in sorted(destination.items(), key=lambda item: item[0].as_posix())
    ]
    manifest = {
        "schema": SCHEMA,
        "kind": KIND,
        "id": ID,
        "commit_policy": COMMIT_POLICY,
        "source_archive_sha256": review.EXPECTED_SOURCE_SHA256,
        "materialization_manifest_sha256": _sha256(materialization_manifest_raw),
        "review_result_sha256": materialization_manifest["review_result_sha256"],
        "admission_worksheet_sha256": materialization_manifest["admission_worksheet_sha256"],
        "gate_id": materialize.GATE_ID,
        "gate_sha256": _sha256(staged["gate.json"]),
        "source_gate_report_sha256": _sha256(report_raw),
        "var_records": len(admitted),
        "semantic_rules": materialization_manifest["semantic_rules"],
        "contains_official_source_text": False,
        "source_admitted": True,
        "production_implementation_authorized": True,
        "runtime_behavior_implemented": False,
        "files": file_entries,
    }
    manifest_raw = _pretty_bytes(manifest)

    try:
        for relative, raw in destination.items():
            path = repo_root.joinpath(*relative.parts)
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("xb") as stream:
                stream.write(raw)
        manifest_path = repo_root.joinpath(*MANIFEST_PATH.parts)
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        with manifest_path.open("xb") as stream:
            stream.write(manifest_raw)
    except OSError as error:
        raise PromoteError(f"repository promotion write failed after successful preflight: {error}") from error

    return {
        "id": ID,
        "manifest_path": MANIFEST_PATH.as_posix(),
        "manifest_sha256": _sha256(manifest_raw),
        "source_gate_report_sha256": _sha256(report_raw),
        "var_records": len(admitted),
        "semantic_rules": materialization_manifest["semantic_rules"],
        "contains_official_source_text": False,
        "source_admitted": True,
        "production_implementation_authorized": True,
        "runtime_behavior_implemented": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--staging-dir", type=Path, required=True)
    parser.add_argument("--gate-report", type=Path, required=True)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        summary = promote(args.staging_dir, args.gate_report, args.repo_root)
    except PromoteError as error:
        print(f"R2C world-state admission promotion failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
