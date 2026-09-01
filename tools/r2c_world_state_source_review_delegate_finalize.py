#!/usr/bin/env python3
"""Finalize the bounded R2C world-state delegate review into source-free evidence.

This closes only the human review of the second-order biome-palette and light-data-layer source
bundle. It does not admit production semantics and it does not silently discharge the parent
R2C-BIOMES/R2C-LIGHT review. The emitted result is the source-free provenance object that a later
parent-review binding step can consume explicitly.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Mapping, Sequence

try:
    from . import r2c_world_state_delegate_closure_source_review as closure
except ImportError:  # Direct `python3 tools/...` execution.
    import r2c_world_state_delegate_closure_source_review as closure  # type: ignore[no-redef]

SCHEMA = 1
RESULT_KIND = "r2c-world-state-delegate-closure-review-result"
RESULT_ID = "REVIEW-NET-R2C-WORLD-STATE-DELEGATE-CLOSURE-26_2-001"
RESULT_COMMIT_POLICY = "SOURCE_FREE_DELEGATE_CLOSURE_REVIEW_RESULT_NOT_ADMISSION"
CANDIDATE_ID = re.compile(r"DISC-NET-R2C-WORLD-DELEGATE-[0-9]{4}\Z")
SOURCE_FREE_FIELDS = (
    "candidate_id",
    "source_identity",
    "source",
    "source_location",
    "atlas_observed_hazards",
    "atlas_classifications",
    "calls",
    "group_ids",
    "review_focus",
)


class FinalizeError(RuntimeError):
    """Fail-closed R2C delegate-closure finalization error."""


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


def _read_json(path: Path, label: str) -> tuple[dict[str, object], bytes, str]:
    if path.is_symlink() or not path.is_file():
        raise FinalizeError(f"{label} must be a real non-symlink file: {path}")
    try:
        raw = path.read_bytes()
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise FinalizeError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise FinalizeError(f"{label} must be a JSON object")
    return value, raw, _sha256_bytes(raw)


def _fresh_output(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise FinalizeError(f"output file must not already exist: {path}")
    parent = path.parent
    if parent and parent.exists() and parent.is_symlink():
        raise FinalizeError(f"output parent must not be a symlink: {parent}")
    if parent and not parent.exists():
        try:
            parent.mkdir(parents=True)
        except OSError as error:
            raise FinalizeError(f"cannot create output parent {parent}: {error}") from error
    return path


def _object(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise FinalizeError(f"{label} must be an object")
    return value


def _array(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise FinalizeError(f"{label} must be an array")
    return value


def _strings(value: object, label: str, *, nonempty: bool = False) -> list[str]:
    raw = _array(value, label)
    if any(not isinstance(item, str) or not item for item in raw):
        raise FinalizeError(f"{label} must contain non-empty strings")
    result = [str(item) for item in raw]
    if nonempty and not result:
        raise FinalizeError(f"{label} must not be empty")
    if len(result) != len(set(result)):
        raise FinalizeError(f"{label} must not contain duplicates")
    return result


def _digest(value: object, label: str) -> str:
    if not isinstance(value, str) or len(value) != 64 or any(ch not in "0123456789abcdef" for ch in value):
        raise FinalizeError(f"{label} must be lowercase SHA-256")
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
        raise FinalizeError(f"{label} fields are not canonical")
    result: dict[str, str] = {}
    for field in fields:
        item = source[field]
        if not isinstance(item, str) or not item:
            raise FinalizeError(f"{label}.{field} must be a non-empty string")
        result[field] = item
    if result["fingerprint_algorithm"] != "java-token-v2-literal-sensitive":
        raise FinalizeError(f"{label}.fingerprint_algorithm mismatch")
    _digest(result["normalized_sha256"], f"{label}.normalized_sha256")
    _digest(result["body_sha256"], f"{label}.body_sha256")
    return result


def _location(value: object, label: str) -> dict[str, object]:
    location = _object(value, label)
    if set(location) != {"path", "start_line", "end_line"}:
        raise FinalizeError(f"{label} fields are not canonical")
    path = location["path"]
    start = location["start_line"]
    end = location["end_line"]
    if not isinstance(path, str) or not path:
        raise FinalizeError(f"{label}.path must be non-empty")
    if type(start) is not int or type(end) is not int or start < 1 or end < start:
        raise FinalizeError(f"{label} line range is invalid")
    return {"path": path, "start_line": start, "end_line": end}


def _calls(value: object, label: str) -> dict[str, object]:
    calls = _object(value, label)
    fields = {"call_sites", "resolved_targets", "unresolved_call_sites", "top_unresolved_callees"}
    if set(calls) != fields:
        raise FinalizeError(f"{label} fields are not canonical")
    sites = calls["call_sites"]
    unresolved = calls["unresolved_call_sites"]
    if type(sites) is not int or sites < 0:
        raise FinalizeError(f"{label}.call_sites must be a non-negative integer")
    if type(unresolved) is not int or unresolved < 0 or unresolved > sites:
        raise FinalizeError(f"{label}.unresolved_call_sites is invalid")
    resolved = _strings(calls["resolved_targets"], f"{label}.resolved_targets")
    top: list[dict[str, object]] = []
    for index, raw in enumerate(_array(calls["top_unresolved_callees"], f"{label}.top_unresolved_callees")):
        item = _object(raw, f"{label}.top_unresolved_callees[{index}]")
        if set(item) != {"callee", "sites"}:
            raise FinalizeError(f"{label}.top_unresolved_callees[{index}] fields are invalid")
        callee = item["callee"]
        count = item["sites"]
        if not isinstance(callee, str) or not callee or type(count) is not int or count < 1:
            raise FinalizeError(f"{label}.top_unresolved_callees[{index}] is invalid")
        top.append({"callee": callee, "sites": count})
    return {
        "call_sites": sites,
        "resolved_targets": resolved,
        "unresolved_call_sites": unresolved,
        "top_unresolved_callees": top,
    }


def _candidate(value: object, label: str, *, allowed_groups: set[str], focus_by_group: Mapping[str, str]) -> dict[str, object]:
    candidate = _object(value, label)
    if set(candidate) != set(SOURCE_FREE_FIELDS):
        raise FinalizeError(f"{label} fields are not canonical")
    candidate_id = candidate["candidate_id"]
    identity = candidate["source_identity"]
    if not isinstance(candidate_id, str) or CANDIDATE_ID.fullmatch(candidate_id) is None:
        raise FinalizeError(f"{label}.candidate_id is invalid")
    if not isinstance(identity, str) or not identity:
        raise FinalizeError(f"{label}.source_identity is invalid")
    source = _source(candidate["source"], f"{label}.source")
    if identity != f"{source['type']}#{source['signature']}":
        raise FinalizeError(f"{label}.source_identity does not match source fingerprint identity")
    groups = _strings(candidate["group_ids"], f"{label}.group_ids", nonempty=True)
    if any(group not in allowed_groups for group in groups):
        raise FinalizeError(f"{label}.group_ids contains an unknown delegate group")
    focus = _strings(candidate["review_focus"], f"{label}.review_focus", nonempty=True)
    expected_focus = sorted({focus_by_group[group] for group in groups})
    if focus != expected_focus:
        raise FinalizeError(f"{label}.review_focus disagrees with group membership")
    return {
        "candidate_id": candidate_id,
        "source_identity": identity,
        "source": source,
        "source_location": _location(candidate["source_location"], f"{label}.source_location"),
        "atlas_observed_hazards": _strings(
            candidate["atlas_observed_hazards"], f"{label}.atlas_observed_hazards"
        ),
        "atlas_classifications": _strings(
            candidate["atlas_classifications"], f"{label}.atlas_classifications"
        ),
        "calls": _calls(candidate["calls"], f"{label}.calls"),
        "group_ids": groups,
        "review_focus": focus,
    }


def _current_provenance() -> dict[str, str]:
    closure._load_plan(closure.DEFAULT_PLAN)
    return {
        "plan_sha256": _sha256_file(closure.DEFAULT_PLAN),
        "parent_discovery_plan_sha256": _sha256_file(closure.DEFAULT_PARENT_PLAN),
        "frontier_sha256": _sha256_file(closure.DEFAULT_FRONTIER),
    }


def _validate_pack(value: Mapping[str, object], pack_sha: str) -> tuple[dict[str, dict[str, object]], list[dict[str, object]], dict[str, str]]:
    provenance = _current_provenance()
    required = {
        "schema": SCHEMA,
        "kind": closure.PACK_KIND,
        "review_id": closure.PLAN_ID,
        "parent_review_id": closure.PARENT_REVIEW_ID,
        "commit_policy": closure.PACK_COMMIT_POLICY,
        "contains_official_source_text": True,
        "production_admitted": False,
        "source_archive_sha256": closure.EXPECTED_SOURCE_SHA256,
        **provenance,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise FinalizeError(f"delegate review pack identity/provenance mismatch: {json.dumps(mismatches, sort_keys=True)}")
    _digest(pack_sha, "delegate review pack digest")

    raw_groups = _array(value.get("groups"), "delegate review pack groups")
    if len(raw_groups) != len(closure.EXPECTED_GROUPS):
        raise FinalizeError("delegate review pack must contain exactly the planned groups")
    groups: list[dict[str, object]] = []
    focus_by_group: dict[str, str] = {}
    parent_by_group: dict[str, str] = {}
    for index, (expected_group, expected_parent) in enumerate(closure.EXPECTED_GROUPS):
        group = _object(raw_groups[index], f"delegate review pack groups[{index}]")
        if set(group) != {"group_id", "parent_group_id", "review_focus"}:
            raise FinalizeError(f"delegate review pack groups[{index}] fields are not canonical")
        if group.get("group_id") != expected_group or group.get("parent_group_id") != expected_parent:
            raise FinalizeError("delegate review pack groups are missing or out of canonical order")
        focus = group.get("review_focus")
        if not isinstance(focus, str) or not focus:
            raise FinalizeError(f"{expected_group}.review_focus must be non-empty")
        groups.append({"group_id": expected_group, "parent_group_id": expected_parent, "review_focus": focus})
        focus_by_group[expected_group] = focus
        parent_by_group[expected_group] = expected_parent

    allowed_groups = set(focus_by_group)
    records: dict[str, dict[str, object]] = {}
    excerpt_bytes = 0
    candidate_ids: set[str] = set()
    for index, raw_record in enumerate(_array(value.get("source_records"), "delegate review pack source_records")):
        record = _object(raw_record, f"delegate review pack source_records[{index}]")
        expected_fields = set(SOURCE_FREE_FIELDS) | {"source_excerpt", "source_excerpt_sha256"}
        if set(record) != expected_fields:
            raise FinalizeError(f"delegate review pack source_records[{index}] fields are not canonical")
        source_free = _candidate(
            {field: record[field] for field in SOURCE_FREE_FIELDS},
            f"delegate review pack source_records[{index}]",
            allowed_groups=allowed_groups,
            focus_by_group=focus_by_group,
        )
        identity = str(source_free["source_identity"])
        candidate_id = str(source_free["candidate_id"])
        if identity in records or candidate_id in candidate_ids:
            raise FinalizeError("delegate review pack source identities and candidate ids must be unique")
        excerpt = record["source_excerpt"]
        excerpt_sha = record["source_excerpt_sha256"]
        if not isinstance(excerpt, str):
            raise FinalizeError(f"{identity}.source_excerpt must be text")
        if _sha256_bytes(excerpt.encode("utf-8")) != _digest(excerpt_sha, f"{identity}.source_excerpt_sha256"):
            raise FinalizeError(f"{identity} source excerpt digest mismatch")
        excerpt_bytes += len(excerpt.encode("utf-8"))
        records[identity] = source_free
        candidate_ids.add(candidate_id)

    if value.get("unique_source_records") != len(records):
        raise FinalizeError("delegate review pack unique_source_records mismatch")
    if value.get("source_excerpt_bytes") != excerpt_bytes:
        raise FinalizeError("delegate review pack source_excerpt_bytes mismatch")
    if not records:
        raise FinalizeError("delegate review pack contains no source records")
    for group_id in allowed_groups:
        if not any(group_id in record["group_ids"] for record in records.values()):
            raise FinalizeError(f"delegate review pack group has no candidates: {group_id}")
    return records, groups, provenance


def _review_precursor_bytes(value: Mapping[str, object]) -> bytes:
    """Reconstruct the exact source-free worksheet emitted before human review."""
    precursor = json.loads(json.dumps(value, ensure_ascii=False))
    groups = _array(precursor.get("groups"), "delegate worksheet groups")
    for index, raw_group in enumerate(groups):
        group = _object(raw_group, f"delegate worksheet groups[{index}]")
        group["source_inspected"] = False
        group["selected_source_identities"] = []
        group["rejected_source_identities"] = []
        group["hazards_reviewed"] = []
        group["followup_dependencies"] = []
        group["semantic_observations"] = []
        group["review_complete"] = False
    return _pretty_bytes(precursor)


def _validate_manifest(
    value: Mapping[str, object],
    *,
    manifest_sha: str,
    pack_sha: str,
    pack_bytes: bytes,
    worksheet_value: Mapping[str, object],
    provenance: Mapping[str, str],
) -> tuple[str, int]:
    required = {
        "schema": SCHEMA,
        "kind": closure.MANIFEST_KIND,
        "review_id": closure.PLAN_ID,
        "commit_policy": closure.MANIFEST_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": closure.EXPECTED_SOURCE_SHA256,
        **provenance,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise FinalizeError(f"delegate manifest identity/provenance mismatch: {json.dumps(mismatches, sort_keys=True)}")
    _digest(manifest_sha, "delegate manifest digest")

    precursor_bytes = _review_precursor_bytes(worksheet_value)
    precursor_sha = _sha256_bytes(precursor_bytes)
    files = _array(value.get("files"), "delegate manifest files")
    if len(files) != 2:
        raise FinalizeError("delegate manifest must bind exactly review-pack.json and worksheet.json")
    expected = {
        "review-pack.json": (pack_sha, len(pack_bytes), True),
        "worksheet.json": (precursor_sha, len(precursor_bytes), False),
    }
    seen: set[str] = set()
    for index, raw_file in enumerate(files):
        item = _object(raw_file, f"delegate manifest files[{index}]")
        if set(item) != {"path", "sha256", "size", "source_rich"}:
            raise FinalizeError(f"delegate manifest files[{index}] fields are not canonical")
        path = item["path"]
        if not isinstance(path, str) or path not in expected or path in seen:
            raise FinalizeError("delegate manifest file surface is invalid or duplicated")
        digest, size, source_rich = expected[path]
        if item.get("sha256") != digest or item.get("size") != size or item.get("source_rich") is not source_rich:
            raise FinalizeError(f"delegate manifest metadata mismatch for {path}")
        seen.add(path)
    if seen != set(expected):
        raise FinalizeError("delegate manifest file surface is incomplete")
    return precursor_sha, len(precursor_bytes)


def _validate_worksheet(
    value: Mapping[str, object],
    *,
    pack_sha: str,
    records: Mapping[str, dict[str, object]],
    groups: Sequence[Mapping[str, object]],
    provenance: Mapping[str, str],
) -> list[dict[str, object]]:
    required = {
        "schema": SCHEMA,
        "kind": closure.WORKSHEET_KIND,
        "review_id": closure.PLAN_ID,
        "parent_review_id": closure.PARENT_REVIEW_ID,
        "commit_policy": closure.WORKSHEET_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": closure.EXPECTED_SOURCE_SHA256,
        "plan_sha256": provenance["plan_sha256"],
        "review_pack_sha256": pack_sha,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise FinalizeError(f"delegate worksheet identity/provenance mismatch: {json.dumps(mismatches, sort_keys=True)}")

    raw_groups = _array(value.get("groups"), "delegate worksheet groups")
    if len(raw_groups) != len(groups):
        raise FinalizeError("delegate worksheet must contain exactly the planned groups")
    finalized: list[dict[str, object]] = []
    all_identities = set(records)
    consumed: set[str] = set()
    for index, expected in enumerate(groups):
        group_id = str(expected["group_id"])
        parent_group_id = str(expected["parent_group_id"])
        focus = str(expected["review_focus"])
        group = _object(raw_groups[index], f"delegate worksheet groups[{index}]")
        expected_fields = {
            "group_id",
            "parent_group_id",
            "review_focus",
            "candidates",
            "source_inspected",
            "selected_source_identities",
            "rejected_source_identities",
            "hazards_reviewed",
            "followup_dependencies",
            "semantic_observations",
            "review_complete",
        }
        if set(group) != expected_fields:
            raise FinalizeError(f"delegate worksheet groups[{index}] fields are not canonical")
        if group.get("group_id") != group_id or group.get("parent_group_id") != parent_group_id or group.get("review_focus") != focus:
            raise FinalizeError("delegate worksheet groups are missing, reordered, or drifted")
        if group.get("source_inspected") is not True:
            raise FinalizeError(f"{group_id}.source_inspected must be true")
        if group.get("review_complete") is not True:
            raise FinalizeError(f"{group_id}.review_complete must be true")
        followups = _strings(group.get("followup_dependencies"), f"{group_id}.followup_dependencies")
        if followups:
            raise FinalizeError(f"{group_id} still has unresolved followup_dependencies")
        observations = _strings(group.get("semantic_observations"), f"{group_id}.semantic_observations", nonempty=True)
        selected = _strings(group.get("selected_source_identities"), f"{group_id}.selected_source_identities", nonempty=True)
        rejected = _strings(group.get("rejected_source_identities"), f"{group_id}.rejected_source_identities")
        hazards = _strings(group.get("hazards_reviewed"), f"{group_id}.hazards_reviewed")
        if set(selected) & set(rejected):
            raise FinalizeError(f"{group_id} selected/rejected identities overlap")
        expected_identities = {
            identity for identity, record in records.items() if group_id in record["group_ids"]
        }
        if set(selected) | set(rejected) != expected_identities:
            raise FinalizeError(f"{group_id} selected/rejected identities must exactly partition candidates")

        worksheet_candidates: dict[str, dict[str, object]] = {}
        for candidate_index, raw_candidate in enumerate(_array(group.get("candidates"), f"{group_id}.candidates")):
            candidate = _candidate(
                raw_candidate,
                f"{group_id}.candidates[{candidate_index}]",
                allowed_groups={str(item["group_id"]) for item in groups},
                focus_by_group={str(item["group_id"]): str(item["review_focus"]) for item in groups},
            )
            identity = str(candidate["source_identity"])
            if identity in worksheet_candidates:
                raise FinalizeError(f"{group_id} contains duplicate candidate identity {identity}")
            if records.get(identity) != candidate:
                raise FinalizeError(f"{group_id} candidate metadata drift for {identity}")
            worksheet_candidates[identity] = candidate
        if set(worksheet_candidates) != expected_identities:
            raise FinalizeError(f"{group_id} worksheet candidate set differs from review pack")

        required_hazards = {
            str(hazard)
            for identity in selected
            for hazard in records[identity]["atlas_observed_hazards"]
        }
        missing_hazards = sorted(required_hazards - set(hazards))
        if missing_hazards:
            raise FinalizeError(f"{group_id} selected-source Atlas hazards are not fully reviewed: {missing_hazards}")
        consumed.update(expected_identities)
        finalized.append(
            {
                "group_id": group_id,
                "parent_group_id": parent_group_id,
                "review_focus": focus,
                "selected_sources": [records[identity] for identity in selected],
                "rejected_source_identities": rejected,
                "hazards_reviewed": hazards,
                "semantic_observations": observations,
            }
        )
    if consumed != all_identities:
        raise FinalizeError("delegate worksheet did not account for every source record")
    return finalized


def finalize(review_pack: Path, worksheet: Path, manifest: Path, output: Path) -> dict[str, object]:
    pack_value, pack_bytes, pack_sha = _read_json(review_pack, "delegate review pack")
    worksheet_value, _worksheet_bytes, worksheet_sha = _read_json(worksheet, "delegate review worksheet")
    manifest_value, _manifest_bytes, manifest_sha = _read_json(manifest, "delegate review manifest")
    records, groups, provenance = _validate_pack(pack_value, pack_sha)
    finalized_groups = _validate_worksheet(
        worksheet_value,
        pack_sha=pack_sha,
        records=records,
        groups=groups,
        provenance=provenance,
    )
    generated_worksheet_sha, generated_worksheet_size = _validate_manifest(
        manifest_value,
        manifest_sha=manifest_sha,
        pack_sha=pack_sha,
        pack_bytes=pack_bytes,
        worksheet_value=worksheet_value,
        provenance=provenance,
    )
    result: dict[str, object] = {
        "schema": SCHEMA,
        "kind": RESULT_KIND,
        "id": RESULT_ID,
        "parent_review_id": closure.PARENT_REVIEW_ID,
        "commit_policy": RESULT_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "all_groups_review_complete": True,
        "source_archive_sha256": closure.EXPECTED_SOURCE_SHA256,
        **provenance,
        "review_pack_sha256": pack_sha,
        "generated_worksheet_sha256": generated_worksheet_sha,
        "generated_worksheet_size": generated_worksheet_size,
        "worksheet_sha256": worksheet_sha,
        "manifest_sha256": manifest_sha,
        "groups": finalized_groups,
        "next_step": (
            "Bind these reviewed delegate sources explicitly into the parent R2C world-state review, "
            "then author source-free semantic rules and run the independent Vanilla Atlas gate."
        ),
    }
    raw = _pretty_bytes(result)
    output = _fresh_output(output)
    try:
        output.write_bytes(raw)
    except OSError as error:
        raise FinalizeError(f"cannot write delegate closure result {output}: {error}") from error
    return {
        "output": str(output),
        "sha256": _sha256_bytes(raw),
        "groups": len(finalized_groups),
        "selected_sources": sum(len(group["selected_sources"]) for group in finalized_groups),
        "contains_official_source_text": False,
        "production_admitted": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--review-pack", type=Path, required=True)
    parser.add_argument("--worksheet", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        summary = finalize(args.review_pack, args.worksheet, args.manifest, args.output)
    except (FinalizeError, OSError) as error:
        print(f"R2C world-state delegate-review finalization failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
