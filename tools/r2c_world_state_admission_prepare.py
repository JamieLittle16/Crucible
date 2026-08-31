#!/usr/bin/env python3
"""Prepare a structured, source-free R2C world-state semantic-admission worksheet.

The input is the completed source-free review result emitted by
`r2c_world_state_source_review_finalize.py`. This tool does not infer semantic rules from source
names, signatures, call graphs, or free-form observations. It merely freezes the exact selected
source set and creates empty structured rule slots for a human reviewer to author.

A later materializer must reject the worksheet until every group is explicitly completed and every
semantic rule names exact selected-source support.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Mapping, Sequence

try:
    from . import r2c_world_state_source_review_finalize as review
except ImportError:  # Direct `python3 tools/...` execution.
    import r2c_world_state_source_review_finalize as review  # type: ignore[no-redef]

SCHEMA = 1
KIND = "r2c-world-state-semantic-admission-worksheet"
ID = "ADMISSION-NET-R2C-WORLD-STATE-26_2-001"
COMMIT_POLICY = "SOURCE_FREE_AUTHORING_NOT_ADMISSION"
SEM_PREFIX = "SEM-NET-R2C-WORLD-"
VAR_PREFIX = "VAR-NET-R2C-WORLD-"


class PrepareError(RuntimeError):
    """Fail-closed R2C semantic-admission authoring error."""


def _pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _read_json(path: Path) -> tuple[dict[str, object], str]:
    if path.is_symlink() or not path.is_file():
        raise PrepareError(f"review result must be a real non-symlink file: {path}")
    try:
        raw = path.read_bytes()
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PrepareError(f"cannot read review result: {error}") from error
    if not isinstance(value, dict):
        raise PrepareError("review result must be a JSON object")
    return value, _sha256_bytes(raw)


def _fresh_output(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise PrepareError(f"output file must not already exist: {path}")
    parent = path.parent
    if parent and parent.exists() and parent.is_symlink():
        raise PrepareError(f"output parent must not be a symlink: {parent}")
    if parent and not parent.exists():
        try:
            parent.mkdir(parents=True)
        except OSError as error:
            raise PrepareError(f"cannot create output parent {parent}: {error}") from error
    return path


def _object(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise PrepareError(f"{label} must be an object")
    return value


def _array(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise PrepareError(f"{label} must be an array")
    return value


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise PrepareError(f"{label} must be a non-empty string")
    return value


def _source(value: object, label: str) -> dict[str, str]:
    source = _object(value, label)
    fields = {
        "type",
        "signature",
        "fingerprint_algorithm",
        "normalized_sha256",
        "body_sha256",
    }
    if set(source) != fields:
        raise PrepareError(f"{label} fields are not canonical")
    result = {field: _string(source[field], f"{label}.{field}") for field in fields}
    if result["fingerprint_algorithm"] != "java-token-v2-literal-sensitive":
        raise PrepareError(f"{label}.fingerprint_algorithm mismatch")
    for field in ("normalized_sha256", "body_sha256"):
        digest = result[field]
        if len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
            raise PrepareError(f"{label}.{field} must be lowercase SHA-256")
    return result


def _candidate(value: object, label: str) -> dict[str, object]:
    candidate = _object(value, label)
    expected = {
        "candidate_id",
        "source_identity",
        "source",
        "source_location",
        "atlas_observed_hazards",
        "atlas_classifications",
        "calls",
    }
    if set(candidate) != expected:
        raise PrepareError(f"{label} fields are not canonical")
    candidate_id = _string(candidate["candidate_id"], f"{label}.candidate_id")
    identity = _string(candidate["source_identity"], f"{label}.source_identity")
    source = _source(candidate["source"], f"{label}.source")
    if identity != f"{source['type']}#{source['signature']}":
        raise PrepareError(f"{label}.source_identity mismatch")
    location = _object(candidate["source_location"], f"{label}.source_location")
    if set(location) != {"path", "start_line", "end_line"}:
        raise PrepareError(f"{label}.source_location fields are not canonical")
    path = _string(location["path"], f"{label}.source_location.path")
    start = location["start_line"]
    end = location["end_line"]
    if type(start) is not int or type(end) is not int or start < 1 or end < start:
        raise PrepareError(f"{label}.source_location line range is invalid")
    hazards = _array(candidate["atlas_observed_hazards"], f"{label}.atlas_observed_hazards")
    if any(not isinstance(item, str) or not item for item in hazards):
        raise PrepareError(f"{label}.atlas_observed_hazards must contain strings")
    classifications = _array(candidate["atlas_classifications"], f"{label}.atlas_classifications")
    if any(not isinstance(item, str) or not item for item in classifications):
        raise PrepareError(f"{label}.atlas_classifications must contain strings")
    calls = _object(candidate["calls"], f"{label}.calls")
    if set(calls) != {
        "call_sites",
        "resolved_targets",
        "unresolved_call_sites",
        "top_unresolved_callees",
    }:
        raise PrepareError(f"{label}.calls fields are not canonical")
    return {
        "candidate_id": candidate_id,
        "source_identity": identity,
        "source": source,
        "source_location": {"path": path, "start_line": start, "end_line": end},
        "atlas_observed_hazards": list(hazards),
        "atlas_classifications": list(classifications),
        "calls": calls,
    }


def _var_id(candidate_id: str) -> str:
    prefix = "DISC-NET-R2C-WORLD-"
    if not candidate_id.startswith(prefix):
        raise PrepareError(f"cannot derive stable VAR id from candidate id: {candidate_id}")
    suffix = candidate_id.removeprefix(prefix)
    if not suffix or not suffix.isdigit():
        raise PrepareError(f"candidate id has non-canonical suffix: {candidate_id}")
    return VAR_PREFIX + suffix


def _validate_review_result(value: Mapping[str, object]) -> list[dict[str, object]]:
    required = {
        "schema": SCHEMA,
        "kind": review.RESULT_KIND,
        "id": review.RESULT_ID,
        "commit_policy": review.RESULT_COMMIT_POLICY,
        "source_archive_sha256": review.EXPECTED_SOURCE_SHA256,
        "contains_official_source_text": False,
        "all_groups_review_complete": True,
        "production_admitted": False,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise PrepareError(f"review result identity mismatch: {json.dumps(mismatches, sort_keys=True)}")
    for digest_field in ("discovery_sha256", "review_pack_sha256", "worksheet_sha256"):
        digest = value.get(digest_field)
        if not isinstance(digest, str) or len(digest) != 64:
            raise PrepareError(f"review result {digest_field} is invalid")

    raw_groups = _array(value.get("groups"), "review result groups")
    if len(raw_groups) != len(review.FOCUS_GROUPS):
        raise PrepareError("review result must contain exactly the focused groups")
    selected_by_identity: dict[str, dict[str, object]] = {}
    groups: list[dict[str, object]] = []
    for index, expected_group_id in enumerate(review.FOCUS_GROUPS):
        group = _object(raw_groups[index], f"review result groups[{index}]")
        if group.get("group_id") != expected_group_id:
            raise PrepareError("review result groups are missing or out of canonical order")
        selected_raw = _array(group.get("selected_sources"), f"{expected_group_id}.selected_sources")
        if not selected_raw:
            raise PrepareError(f"{expected_group_id} has no selected sources")
        identities: list[str] = []
        for candidate_index, raw_candidate in enumerate(selected_raw):
            candidate = _candidate(raw_candidate, f"{expected_group_id}.selected_sources[{candidate_index}]")
            identity = str(candidate["source_identity"])
            existing = selected_by_identity.get(identity)
            if existing is not None and existing != candidate:
                raise PrepareError(f"selected source metadata differs across groups: {identity}")
            selected_by_identity.setdefault(identity, candidate)
            identities.append(identity)
        if len(identities) != len(set(identities)):
            raise PrepareError(f"{expected_group_id} selected sources contain duplicates")
        groups.append({
            "group_id": expected_group_id,
            "selected_source_identities": identities,
            "semantic_rules": [],
            "admission_complete": False,
        })

    selected_sources = []
    seen_var_ids: set[str] = set()
    for identity, candidate in sorted(selected_by_identity.items()):
        var_id = _var_id(str(candidate["candidate_id"]))
        if var_id in seen_var_ids:
            raise PrepareError(f"derived duplicate VAR id: {var_id}")
        seen_var_ids.add(var_id)
        selected_sources.append({
            "var_id": var_id,
            "source_identity": identity,
            "candidate": candidate,
        })
    return [{"groups": groups, "selected_sources": selected_sources}]


def prepare(review_result: Path, output: Path) -> dict[str, object]:
    value, review_sha = _read_json(review_result)
    normalized = _validate_review_result(value)[0]
    worksheet: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "id": ID,
        "commit_policy": COMMIT_POLICY,
        "review_result_sha256": review_sha,
        "source_archive_sha256": review.EXPECTED_SOURCE_SHA256,
        "contains_official_source_text": False,
        "semantic_rule_contract": {
            "id_prefix": SEM_PREFIX,
            "required_fields": ["id", "statement", "source_identities"],
            "source_support_must_be_selected": True,
            "automatic_semantic_inference_forbidden": True,
        },
        "selected_sources": normalized["selected_sources"],
        "groups": normalized["groups"],
        "all_groups_admission_complete": False,
        "production_admitted": False,
        "next_step": (
            "Author explicit semantic rules from the completed pinned-source review. Each rule must "
            "use a stable SEM-NET-R2C-WORLD-* id and cite exact selected source identities."
        ),
    }
    raw = _pretty_bytes(worksheet)
    path = _fresh_output(output)
    try:
        path.write_bytes(raw)
    except OSError as error:
        raise PrepareError(f"cannot write admission worksheet {path}: {error}") from error
    return {
        "output": str(path),
        "sha256": _sha256_bytes(raw),
        "groups": len(normalized["groups"]),  # type: ignore[arg-type]
        "selected_sources": len(normalized["selected_sources"]),  # type: ignore[arg-type]
        "contains_official_source_text": False,
        "production_admitted": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--review-result", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        summary = prepare(args.review_result, args.output)
    except PrepareError as error:
        print(f"R2C world-state admission preparation failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
