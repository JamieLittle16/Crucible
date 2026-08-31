#!/usr/bin/env python3
"""Build a focused local source-review pack for R2C biome, heightmap and light semantics.

The committed R2C discovery output is intentionally source-free. This tool is the next local-only
step: it binds that discovery to the exact source archive pinned by `vanilla.lock.toml`, extracts
only the candidate declaration line spans for the three R2C.4 world-state groups, and writes two
strictly separated artifacts outside the repository:

* `review-pack.json` is source-rich and marked `EPHEMERAL_DO_NOT_COMMIT`;
* `worksheet.json` is source-free and fingerprint-bound for later manual review/admission.

Neither artifact admits production semantics. Exact source review, delegate closure, VAR/SEM
materialization and the independent source gate remain mandatory before target-specific behavior is
implemented.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
import zipfile
from pathlib import Path, PurePosixPath
from typing import Mapping, Sequence

SCHEMA = 1
DISCOVERY_KIND = "r2c-world-projection-source-discovery"
DISCOVERY_REVIEW_ID = "REVIEW-NET-R2C-WORLD-PROJECTION-DISCOVERY-26_2-001"
PACK_KIND = "r2c-world-state-source-review-pack"
WORKSHEET_KIND = "r2c-world-state-source-review-worksheet"
MANIFEST_KIND = "r2c-world-state-source-review-manifest"
PACK_COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
WORKSHEET_COMMIT_POLICY = "SOURCE_FREE_REVIEW_ONLY_NOT_ADMISSION"
FOCUS_GROUPS = ("R2C-BIOMES", "R2C-HEIGHTMAPS", "R2C-LIGHT")
EXPECTED_TARGET = {
    "minecraft_version": "26.2",
    "protocol_version": 776,
    "world_version": 4903,
}
EXPECTED_SOURCE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
MAX_CANDIDATE_LINES = 2048
MAX_TOTAL_SOURCE_BYTES = 16 * 1024 * 1024

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SOURCE = Path.home() / "Documents/mc-source/mc-src.zip"
DEFAULT_LOCK = REPO_ROOT / "vanilla/vanilla.lock.toml"


class ReviewPackError(RuntimeError):
    """Fail-closed focused source-review-pack error."""


def _pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _fresh_external_dir(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise ReviewPackError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repository = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repository)
    except ValueError:
        return resolved
    raise ReviewPackError("R2C source-review output must live outside the repository")


def _read_json_object(path: Path, label: str) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ReviewPackError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise ReviewPackError(f"{label} must be a JSON object")
    return value


def _load_lock_source_sha(lock_path: Path) -> str:
    try:
        value = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ReviewPackError(f"cannot read vanilla lock: {error}") from error
    source = value.get("source")
    if not isinstance(source, dict):
        raise ReviewPackError("vanilla lock source table is missing")
    source_sha = source.get("archive_sha256")
    if not isinstance(source_sha, str) or len(source_sha) != 64:
        raise ReviewPackError("vanilla lock source.archive_sha256 is invalid")
    if (
        value.get("minecraft") != EXPECTED_TARGET["minecraft_version"]
        or value.get("protocol") != EXPECTED_TARGET["protocol_version"]
        or value.get("data_version") != EXPECTED_TARGET["world_version"]
    ):
        raise ReviewPackError("vanilla lock target identity does not match R2C.4 target")
    if source_sha != EXPECTED_SOURCE_SHA256:
        raise ReviewPackError("vanilla lock source identity does not match the frozen R2C source")
    return source_sha


def _required_dict(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise ReviewPackError(f"{label} must be an object")
    return value


def _required_list(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise ReviewPackError(f"{label} must be an array")
    return value


def _validate_discovery(value: Mapping[str, object]) -> tuple[list[dict[str, object]], str]:
    if value.get("schema") != SCHEMA or value.get("kind") != DISCOVERY_KIND:
        raise ReviewPackError("R2C discovery identity mismatch")
    if value.get("review_id") != DISCOVERY_REVIEW_ID:
        raise ReviewPackError("R2C discovery review id mismatch")
    if value.get("source_text_included") is not False or value.get("production_admitted") is not False:
        raise ReviewPackError("R2C discovery must be source-free and non-admitted")

    source = _required_dict(value.get("source"), "discovery source")
    target = {
        "minecraft_version": source.get("minecraft_version"),
        "protocol_version": source.get("protocol_version"),
        "world_version": source.get("world_version"),
    }
    if target != EXPECTED_TARGET:
        raise ReviewPackError("R2C discovery target identity mismatch")
    source_sha = source.get("archive_sha256")
    if source_sha != EXPECTED_SOURCE_SHA256:
        raise ReviewPackError("R2C discovery source identity mismatch")

    raw_groups = _required_list(value.get("groups"), "discovery groups")
    groups_by_id: dict[str, dict[str, object]] = {}
    for raw_group in raw_groups:
        group = _required_dict(raw_group, "discovery group")
        group_id = group.get("group_id")
        if not isinstance(group_id, str) or not group_id or group_id in groups_by_id:
            raise ReviewPackError("R2C discovery contains invalid/duplicate group id")
        if group.get("production_admitted") is not False:
            raise ReviewPackError(f"discovery group is unexpectedly admitted: {group_id}")
        groups_by_id[group_id] = group

    missing = [group_id for group_id in FOCUS_GROUPS if group_id not in groups_by_id]
    if missing:
        raise ReviewPackError(f"R2C discovery is missing focused groups: {missing}")
    return [groups_by_id[group_id] for group_id in FOCUS_GROUPS], str(source_sha)


def _safe_archive_member(raw_path: object) -> str:
    if not isinstance(raw_path, str) or not raw_path or "\\" in raw_path:
        raise ReviewPackError("candidate source path is invalid")
    path = PurePosixPath(raw_path)
    if path.is_absolute() or ".." in path.parts or "." in path.parts:
        raise ReviewPackError(f"unsafe candidate source path: {raw_path}")
    return path.as_posix()


def _source_free_candidate(candidate: Mapping[str, object]) -> dict[str, object]:
    required = (
        "candidate_id",
        "source_identity",
        "source",
        "source_location",
        "atlas_observed_hazards",
        "atlas_classifications",
        "calls",
    )
    if any(field not in candidate for field in required):
        raise ReviewPackError("candidate is missing required discovery metadata")
    source = _required_dict(candidate["source"], "candidate source fingerprint")
    location = _required_dict(candidate["source_location"], "candidate source location")
    source_identity = candidate["source_identity"]
    candidate_id = candidate["candidate_id"]
    if not isinstance(source_identity, str) or not source_identity:
        raise ReviewPackError("candidate source_identity is invalid")
    if not isinstance(candidate_id, str) or not candidate_id:
        raise ReviewPackError("candidate_id is invalid")
    if source.get("type") is None or source.get("signature") is None:
        raise ReviewPackError(f"candidate source fingerprint incomplete: {source_identity}")
    _safe_archive_member(location.get("path"))
    start = location.get("start_line")
    end = location.get("end_line")
    if type(start) is not int or type(end) is not int or start < 1 or end < start:
        raise ReviewPackError(f"candidate line range is invalid: {source_identity}")
    if end - start + 1 > MAX_CANDIDATE_LINES:
        raise ReviewPackError(f"candidate line range exceeds review bound: {source_identity}")
    return {
        "candidate_id": candidate_id,
        "source_identity": source_identity,
        "source": source,
        "source_location": location,
        "atlas_observed_hazards": candidate["atlas_observed_hazards"],
        "atlas_classifications": candidate["atlas_classifications"],
        "calls": candidate["calls"],
    }


def _focused_candidates(groups: Sequence[Mapping[str, object]]) -> tuple[list[dict[str, object]], dict[str, list[str]]]:
    by_identity: dict[str, dict[str, object]] = {}
    memberships: dict[str, list[str]] = {}
    for group in groups:
        group_id = group.get("group_id")
        if not isinstance(group_id, str):
            raise ReviewPackError("focused discovery group id is invalid")
        candidates = _required_list(group.get("candidate_methods"), f"{group_id} candidates")
        memberships[group_id] = []
        for raw_candidate in candidates:
            candidate = _source_free_candidate(_required_dict(raw_candidate, "candidate"))
            identity = str(candidate["source_identity"])
            previous = by_identity.get(identity)
            if previous is not None and previous != candidate:
                raise ReviewPackError(f"shared source identity has inconsistent discovery metadata: {identity}")
            by_identity[identity] = candidate
            memberships[group_id].append(identity)
        memberships[group_id] = sorted(set(memberships[group_id]))
    return [by_identity[key] for key in sorted(by_identity)], memberships


def _read_source_excerpt(
    archive: zipfile.ZipFile,
    candidate: Mapping[str, object],
    source_cache: dict[str, list[str]],
) -> tuple[str, int]:
    identity = str(candidate["source_identity"])
    location = _required_dict(candidate["source_location"], "candidate source location")
    member = _safe_archive_member(location.get("path"))
    start = int(location["start_line"])
    end = int(location["end_line"])
    if member not in source_cache:
        try:
            raw = archive.read(member)
        except KeyError as error:
            raise ReviewPackError(f"source archive member is missing: {member}") from error
        try:
            text = raw.decode("utf-8", errors="strict")
        except UnicodeDecodeError as error:
            raise ReviewPackError(f"source archive member is not UTF-8: {member}") from error
        source_cache[member] = text.splitlines(keepends=True)
    lines = source_cache[member]
    if end > len(lines):
        raise ReviewPackError(
            f"candidate line range exceeds source member: {identity}: end={end} lines={len(lines)}"
        )
    excerpt = "".join(lines[start - 1 : end])
    return excerpt, len(excerpt.encode("utf-8"))


def build(
    discovery_path: Path,
    source_archive: Path,
    lock_path: Path,
    output_dir: Path,
) -> dict[str, object]:
    output = _fresh_external_dir(output_dir)
    discovery = _read_json_object(discovery_path, "R2C discovery")
    groups, discovery_source_sha = _validate_discovery(discovery)
    lock_source_sha = _load_lock_source_sha(lock_path)
    if discovery_source_sha != lock_source_sha:
        raise ReviewPackError("discovery and vanilla lock source identities differ")
    actual_source_sha = _sha256_file(source_archive)
    if actual_source_sha != lock_source_sha:
        raise ReviewPackError(
            f"source archive SHA-256 mismatch: expected {lock_source_sha}, got {actual_source_sha}"
        )

    unique_candidates, memberships = _focused_candidates(groups)
    if not unique_candidates:
        raise ReviewPackError("focused R2C.4 discovery contains no candidate methods")

    source_records: list[dict[str, object]] = []
    total_source_bytes = 0
    try:
        with zipfile.ZipFile(source_archive) as archive:
            source_cache: dict[str, list[str]] = {}
            for candidate in unique_candidates:
                excerpt, excerpt_bytes = _read_source_excerpt(archive, candidate, source_cache)
                total_source_bytes += excerpt_bytes
                if total_source_bytes > MAX_TOTAL_SOURCE_BYTES:
                    raise ReviewPackError(
                        f"focused source pack exceeds {MAX_TOTAL_SOURCE_BYTES} byte review bound"
                    )
                groups_for_candidate = [
                    group_id
                    for group_id in FOCUS_GROUPS
                    if str(candidate["source_identity"]) in memberships[group_id]
                ]
                source_records.append({
                    **candidate,
                    "group_ids": groups_for_candidate,
                    "source_excerpt": excerpt,
                    "source_excerpt_sha256": _sha256_bytes(excerpt.encode("utf-8")),
                })
    except zipfile.BadZipFile as error:
        raise ReviewPackError(f"invalid source archive: {error}") from error

    discovery_sha = _sha256_file(discovery_path)
    pack: dict[str, object] = {
        "schema": SCHEMA,
        "kind": PACK_KIND,
        "review_id": DISCOVERY_REVIEW_ID,
        "commit_policy": PACK_COMMIT_POLICY,
        "contains_official_source_text": True,
        "source_archive_sha256": actual_source_sha,
        "discovery_sha256": discovery_sha,
        "focused_groups": list(FOCUS_GROUPS),
        "group_memberships": memberships,
        "source_records": source_records,
        "unique_source_records": len(source_records),
        "source_excerpt_bytes": total_source_bytes,
        "production_admitted": False,
    }
    pack_bytes = _pretty_bytes(pack)
    pack_sha = _sha256_bytes(pack_bytes)

    candidate_by_identity = {
        str(candidate["source_identity"]): candidate for candidate in unique_candidates
    }
    worksheet_groups: list[dict[str, object]] = []
    for group in groups:
        group_id = str(group["group_id"])
        worksheet_groups.append({
            "group_id": group_id,
            "review_focus": group.get("review_focus"),
            "candidates": [candidate_by_identity[identity] for identity in memberships[group_id]],
            "source_inspected": False,
            "selected_source_identities": [],
            "rejected_source_identities": [],
            "hazards_reviewed": [],
            "followup_dependencies": [],
            "semantic_observations": [],
            "review_complete": False,
        })
    worksheet: dict[str, object] = {
        "schema": SCHEMA,
        "kind": WORKSHEET_KIND,
        "review_id": DISCOVERY_REVIEW_ID,
        "commit_policy": WORKSHEET_COMMIT_POLICY,
        "contains_official_source_text": False,
        "source_archive_sha256": actual_source_sha,
        "discovery_sha256": discovery_sha,
        "review_pack_sha256": pack_sha,
        "groups": worksheet_groups,
        "production_admitted": False,
    }
    worksheet_bytes = _pretty_bytes(worksheet)
    worksheet_sha = _sha256_bytes(worksheet_bytes)

    manifest: dict[str, object] = {
        "schema": SCHEMA,
        "kind": MANIFEST_KIND,
        "review_id": DISCOVERY_REVIEW_ID,
        "files": [
            {"path": "review-pack.json", "sha256": pack_sha, "size": len(pack_bytes), "source_rich": True},
            {"path": "worksheet.json", "sha256": worksheet_sha, "size": len(worksheet_bytes), "source_rich": False},
        ],
        "source_archive_sha256": actual_source_sha,
        "production_admitted": False,
    }
    manifest_bytes = _pretty_bytes(manifest)

    output.mkdir(parents=True)
    (output / "review-pack.json").write_bytes(pack_bytes)
    (output / "worksheet.json").write_bytes(worksheet_bytes)
    (output / "manifest.json").write_bytes(manifest_bytes)
    return {
        "output_dir": str(output),
        "review_pack_sha256": pack_sha,
        "worksheet_sha256": worksheet_sha,
        "unique_source_records": len(source_records),
        "source_excerpt_bytes": total_source_bytes,
        "focused_groups": len(FOCUS_GROUPS),
        "production_admitted": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--discovery", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = build(args.discovery, args.source, args.lock, args.output_dir)
    except (OSError, ReviewPackError) as error:
        print(f"R2C world-state source review pack failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
