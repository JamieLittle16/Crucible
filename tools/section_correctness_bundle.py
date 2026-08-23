#!/usr/bin/env python3
"""Validate, seal, and reopen the four full M0.3C section correctness records."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any

SCHEMA = 1
KIND = "section-full-correctness-bundle"
MANIFEST_NAME = "bundle-manifest.json"
CANDIDATES = ("direct", "adaptive", "fast-local", "packed-local")
TRACE_SCHEMA = 1
TRACE_COUNT = 16
TRACE_OPERATIONS = 2_013_879
SYNTHETIC_OPERATIONS = 4_112
TRACE_FINGERPRINT = "6a4814a1551a9e5a"
SEM_IDS = (
    "SEM-WORLD-SECTION-001",
    "SEM-WORLD-SECTION-002",
    "SEM-WORLD-SECTION-005",
    "SEM-WORLD-SECTION-006",
    "SEM-WORLD-SECTION-007",
    "SEM-WORLD-SECTION-008",
    "SEM-WORLD-SECTION-009",
    "SEM-WORLD-SECTION-010",
    "SEM-WORLD-SECTION-011",
    "SEM-WORLD-SECTION-012",
    "SEM-WORLD-SECTION-013",
    "SEM-WORLD-SECTION-014",
)
LOWER_SHA256 = re.compile(r"[0-9a-f]{64}\Z")
LOWER_GIT_SHA1 = re.compile(r"[0-9a-f]{40}\Z")


class CorrectnessBundleError(RuntimeError):
    """Raised when full section correctness evidence cannot be bundled safely."""


def canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise CorrectnessBundleError(f"{path} must contain a JSON object")
    return value


def integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise CorrectnessBundleError(f"{label} must be an integer")
    return value


def git_sha(value: object, label: str) -> str:
    if not isinstance(value, str) or LOWER_GIT_SHA1.fullmatch(value) is None:
        raise CorrectnessBundleError(f"{label} must be a canonical 40-hex Git SHA")
    return value


def sha256(value: object, label: str) -> str:
    if not isinstance(value, str) or LOWER_SHA256.fullmatch(value) is None:
        raise CorrectnessBundleError(f"{label} must be canonical lowercase SHA-256")
    return value


def target_contract(repo_root: Path) -> dict[str, object]:
    manifest = load_json(repo_root / "vanilla/state-data/26.2-state-data-manifest.json")
    target = manifest.get("target")
    if not isinstance(target, dict):
        raise CorrectnessBundleError("state-data target is malformed")
    result = {
        "minecraft_version": target.get("minecraft_version"),
        "protocol_version": target.get("protocol_version"),
        "data_version": target.get("data_version"),
        "state_count": manifest.get("state_count"),
        "state_data_input_sha256": manifest.get("input_digest"),
        "state_data_generation_sha256": manifest.get("generation_digest"),
    }
    if result["minecraft_version"] != "26.2":
        raise CorrectnessBundleError("full correctness bundle is pinned to Minecraft 26.2")
    for key in ("protocol_version", "data_version", "state_count"):
        integer(result[key], f"target {key}")
    sha256(result["state_data_input_sha256"], "target input digest")
    sha256(result["state_data_generation_sha256"], "target generation digest")
    return result


def _validate_candidate_directory(root: Path, candidate: str) -> Path:
    directory = root / candidate
    if directory.is_symlink() or not directory.is_dir():
        raise CorrectnessBundleError(
            f"correctness candidate path must be a real directory: {directory}"
        )
    children = sorted(child.name for child in directory.iterdir())
    if children != ["full.json"]:
        raise CorrectnessBundleError(
            f"{candidate} evidence directory must contain exactly full.json; got {children}"
        )
    evidence = directory / "full.json"
    if evidence.is_symlink() or not evidence.is_file():
        raise CorrectnessBundleError(
            f"{candidate} full correctness evidence must be a real file: {evidence}"
        )
    return evidence


def validate_input_inventory(input_root: Path) -> None:
    """Require one closed, symlink-free evidence directory for each production candidate."""
    entries = sorted(entry.name for entry in input_root.iterdir())
    expected = sorted(CANDIDATES)
    if entries != expected:
        missing = sorted(set(expected) - set(entries))
        unexpected = sorted(set(entries) - set(expected))
        raise CorrectnessBundleError(
            "correctness input root must contain exactly the production candidates; "
            f"missing={missing} unexpected={unexpected}"
        )
    for candidate in CANDIDATES:
        _validate_candidate_directory(input_root, candidate)


def validate_sealed_inventory(bundle_root: Path) -> None:
    """Require the exact sealed on-disk shape consumed by later qualification layers."""
    entries = sorted(entry.name for entry in bundle_root.iterdir())
    expected = sorted((*CANDIDATES, MANIFEST_NAME))
    if entries != expected:
        missing = sorted(set(expected) - set(entries))
        unexpected = sorted(set(entries) - set(expected))
        raise CorrectnessBundleError(
            "sealed correctness bundle has a noncanonical inventory; "
            f"missing={missing} unexpected={unexpected}"
        )
    for candidate in CANDIDATES:
        _validate_candidate_directory(bundle_root, candidate)
    manifest = bundle_root / MANIFEST_NAME
    if manifest.is_symlink() or not manifest.is_file():
        raise CorrectnessBundleError("sealed correctness bundle manifest must be a real file")


def validate_candidate(
    path: Path,
    candidate: str,
    *,
    target: dict[str, object],
    expected_commit: str | None,
) -> tuple[str, dict[str, object]]:
    raw = load_json(path)
    if raw.get("schema") != 1 or raw.get("qualification") != "section" or raw.get("mode") != "full":
        raise CorrectnessBundleError(f"{candidate} is not full section qualification evidence")
    commit = git_sha(raw.get("commit_sha"), f"{candidate}.commit_sha")
    if expected_commit is not None and commit != expected_commit:
        raise CorrectnessBundleError(
            f"{candidate} evidence commit mismatch: expected {expected_commit}, got {commit}"
        )
    for key, value in target.items():
        if raw.get(key) != value:
            raise CorrectnessBundleError(f"{candidate} target identity drift at {key}")
    if raw.get("trace_schema") != TRACE_SCHEMA or raw.get("sem_ids") != list(SEM_IDS):
        raise CorrectnessBundleError(f"{candidate} trace/SEM surface drifted")
    records = raw.get("records")
    if not isinstance(records, list) or len(records) != 1 or not isinstance(records[0], dict):
        raise CorrectnessBundleError(f"{candidate} must contain exactly one evidence record")
    record = records[0]
    if record.get("candidate") != candidate:
        raise CorrectnessBundleError(
            f"candidate/path mismatch: expected {candidate}, got {record.get('candidate')!r}"
        )
    expected_id = f"EQUIV-WORLD-SECTION-FULL-{candidate.upper().replace('-', '_')}"
    if record.get("id") != expected_id:
        raise CorrectnessBundleError(f"{candidate} evidence ID drifted")
    checks = (
        ("trace_count", TRACE_COUNT),
        ("trace_operations", TRACE_OPERATIONS),
        ("synthetic_operations", SYNTHETIC_OPERATIONS),
    )
    for field, expected in checks:
        if integer(record.get(field), f"{candidate}.{field}") != expected:
            raise CorrectnessBundleError(f"{candidate} {field} drifted")
    if record.get("trace_fingerprint_fnv1a64") != TRACE_FINGERPRINT:
        raise CorrectnessBundleError(f"{candidate} trace fingerprint drifted")
    return commit, {
        "path": f"{candidate}/full.json",
        "sha256": sha256_file(path),
        "evidence_id": expected_id,
        "trace_count": TRACE_COUNT,
        "trace_operations": TRACE_OPERATIONS,
        "synthetic_operations": SYNTHETIC_OPERATIONS,
        "trace_fingerprint_fnv1a64": TRACE_FINGERPRINT,
    }


def build_bundle(
    *, repo_root: Path, input_root: Path, expected_commit: str | None = None
) -> dict[str, object]:
    repo_root = repo_root.resolve()
    input_root = input_root.resolve()
    if not input_root.is_dir():
        raise CorrectnessBundleError(f"correctness input root is not a directory: {input_root}")
    validate_input_inventory(input_root)
    if expected_commit is not None:
        expected_commit = git_sha(expected_commit, "expected commit")
    target = target_contract(repo_root)
    common_commit: str | None = None
    candidates: dict[str, object] = {}
    for candidate in CANDIDATES:
        path = input_root / candidate / "full.json"
        commit, evidence = validate_candidate(
            path, candidate, target=target, expected_commit=expected_commit
        )
        if common_commit is None:
            common_commit = commit
        elif commit != common_commit:
            raise CorrectnessBundleError("full correctness records do not share one source commit")
        candidates[candidate] = evidence
    assert common_commit is not None
    bundle: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "qualification": "section",
        "mode": "full",
        "commit_sha": common_commit,
        "target": target,
        "trace_schema": TRACE_SCHEMA,
        "sem_ids": list(SEM_IDS),
        "candidate_order": list(CANDIDATES),
        "candidates": candidates,
    }
    bundle["bundle_sha256"] = canonical_digest(bundle)
    return bundle


def validate_sealed_bundle(
    *, repo_root: Path, bundle_root: Path, expected_commit: str | None = None
) -> dict[str, object]:
    """Reopen a sealed correctness bundle and independently revalidate all content identities."""
    repo_root = repo_root.resolve()
    bundle_root = bundle_root.resolve()
    if not bundle_root.is_dir():
        raise CorrectnessBundleError(f"sealed correctness bundle is not a directory: {bundle_root}")
    validate_sealed_inventory(bundle_root)
    if expected_commit is not None:
        expected_commit = git_sha(expected_commit, "expected commit")

    manifest = load_json(bundle_root / MANIFEST_NAME)
    expected_keys = {
        "schema",
        "kind",
        "qualification",
        "mode",
        "commit_sha",
        "target",
        "trace_schema",
        "sem_ids",
        "candidate_order",
        "candidates",
        "bundle_sha256",
    }
    if set(manifest) != expected_keys:
        raise CorrectnessBundleError("sealed bundle manifest fields drifted")
    if manifest.get("schema") != SCHEMA or manifest.get("kind") != KIND:
        raise CorrectnessBundleError("sealed bundle manifest schema/kind mismatch")
    if manifest.get("qualification") != "section" or manifest.get("mode") != "full":
        raise CorrectnessBundleError("sealed bundle is not full section correctness evidence")
    digest = sha256(manifest.get("bundle_sha256"), "bundle manifest digest")
    payload = dict(manifest)
    payload.pop("bundle_sha256")
    if canonical_digest(payload) != digest:
        raise CorrectnessBundleError("sealed bundle manifest digest does not recompute")

    target = target_contract(repo_root)
    if manifest.get("target") != target:
        raise CorrectnessBundleError("sealed bundle target identity drifted")
    if manifest.get("trace_schema") != TRACE_SCHEMA or manifest.get("sem_ids") != list(SEM_IDS):
        raise CorrectnessBundleError("sealed bundle trace/SEM surface drifted")
    if manifest.get("candidate_order") != list(CANDIDATES):
        raise CorrectnessBundleError("sealed bundle candidate order drifted")
    manifest_commit = git_sha(manifest.get("commit_sha"), "sealed bundle commit")
    if expected_commit is not None and manifest_commit != expected_commit:
        raise CorrectnessBundleError(
            f"sealed bundle commit mismatch: expected {expected_commit}, got {manifest_commit}"
        )

    raw_candidates = manifest.get("candidates")
    if not isinstance(raw_candidates, dict) or set(raw_candidates) != set(CANDIDATES):
        raise CorrectnessBundleError("sealed bundle candidate registry drifted")
    for candidate in CANDIDATES:
        raw_manifest_evidence = raw_candidates.get(candidate)
        if not isinstance(raw_manifest_evidence, dict):
            raise CorrectnessBundleError(f"sealed bundle candidate entry is malformed: {candidate}")
        commit, recomputed = validate_candidate(
            bundle_root / candidate / "full.json",
            candidate,
            target=target,
            expected_commit=manifest_commit,
        )
        if commit != manifest_commit:
            raise CorrectnessBundleError("sealed bundle contains mixed correctness commits")
        if raw_manifest_evidence != recomputed:
            raise CorrectnessBundleError(
                f"sealed bundle manifest/file identity mismatch for {candidate}"
            )
    return manifest


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--repo-root", type=Path, default=Path("."))
    result.add_argument("--input-root", type=Path, required=True)
    result.add_argument("--expected-commit")
    result.add_argument("--output", type=Path, required=True)
    return result


def main() -> int:
    args = parser().parse_args()
    try:
        bundle = build_bundle(
            repo_root=args.repo_root,
            input_root=args.input_root,
            expected_commit=args.expected_commit,
        )
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(bundle, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (CorrectnessBundleError, OSError, json.JSONDecodeError) as error:
        print(f"section correctness bundle error: {error}")
        return 1
    print(
        "section full correctness bundle: "
        f"commit={bundle['commit_sha']} bundle={bundle['bundle_sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
