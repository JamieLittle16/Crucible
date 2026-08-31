#!/usr/bin/env python3
"""Finalize completed local R2C biome/heightmap/light review into source-free evidence.

This is review closure, not semantic admission. Production code must still wait for canonical VAR/SEM
materialization and the independent vanilla source gate.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Mapping, Sequence

try:
    from . import r2c_world_state_source_review_pack as packer
except ImportError:  # Direct `python3 tools/...` execution.
    import r2c_world_state_source_review_pack as packer  # type: ignore[no-redef]

SCHEMA = 1
RESULT_KIND = "r2c-world-state-source-review-result"
RESULT_ID = "REVIEW-NET-R2C-WORLD-STATE-26_2-001"
RESULT_COMMIT_POLICY = "SOURCE_FREE_REVIEW_RESULT_NOT_ADMISSION"
FOCUS_GROUPS = packer.FOCUS_GROUPS
EXPECTED_SOURCE_SHA256 = packer.EXPECTED_SOURCE_SHA256
SOURCE_FREE_FIELDS = (
    "candidate_id",
    "source_identity",
    "source",
    "source_location",
    "atlas_observed_hazards",
    "atlas_classifications",
    "calls",
)


class FinalizeError(RuntimeError):
    """Fail-closed focused R2C source-review finalization error."""


def _pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _read_json(path: Path, label: str) -> tuple[dict[str, object], str]:
    if path.is_symlink() or not path.is_file():
        raise FinalizeError(f"{label} must be a real non-symlink file: {path}")
    try:
        raw = path.read_bytes()
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise FinalizeError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise FinalizeError(f"{label} must be a JSON object")
    return value, _sha256_bytes(raw)


def _fresh_output_file(path: Path) -> Path:
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


def _required_dict(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise FinalizeError(f"{label} must be an object")
    return value


def _required_list(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise FinalizeError(f"{label} must be an array")
    return value


def _string_list(value: object, label: str, *, nonempty: bool = False) -> list[str]:
    raw = _required_list(value, label)
    if any(not isinstance(item, str) or not item for item in raw):
        raise FinalizeError(f"{label} must contain non-empty strings")
    result = [str(item) for item in raw]
    if nonempty and not result:
        raise FinalizeError(f"{label} must not be empty")
    if len(result) != len(set(result)):
        raise FinalizeError(f"{label} must not contain duplicates")
    return result


def _canonical_source(value: object, label: str) -> dict[str, str]:
    source = _required_dict(value, label)
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
    for field in ("normalized_sha256", "body_sha256"):
        digest = result[field]
        if len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
            raise FinalizeError(f"{label}.{field} must be lowercase SHA-256")
    return result


def _canonical_location(value: object, label: str) -> dict[str, object]:
    location = _required_dict(value, label)
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


def _canonical_calls(value: object, label: str) -> dict[str, object]:
    calls = _required_dict(value, label)
    fields = {"call_sites", "resolved_targets", "unresolved_call_sites", "top_unresolved_callees"}
    if set(calls) != fields:
        raise FinalizeError(f"{label} fields are not canonical")
    call_sites = calls["call_sites"]
    unresolved = calls["unresolved_call_sites"]
    if type(call_sites) is not int or call_sites < 0:
        raise FinalizeError(f"{label}.call_sites must be a non-negative integer")
    if type(unresolved) is not int or unresolved < 0 or unresolved > call_sites:
        raise FinalizeError(f"{label}.unresolved_call_sites is invalid")
    resolved = _string_list(calls["resolved_targets"], f"{label}.resolved_targets")
    top: list[dict[str, object]] = []
    for index, raw_item in enumerate(
        _required_list(calls["top_unresolved_callees"], f"{label}.top_unresolved_callees")
    ):
        item = _required_dict(raw_item, f"{label}.top_unresolved_callees[{index}]")
        if set(item) != {"callee", "sites"}:
            raise FinalizeError(f"{label}.top_unresolved_callees[{index}] fields are invalid")
        callee = item["callee"]
        sites = item["sites"]
        if not isinstance(callee, str) or not callee or type(sites) is not int or sites < 1:
            raise FinalizeError(f"{label}.top_unresolved_callees[{index}] is invalid")
        top.append({"callee": callee, "sites": sites})
    return {
        "call_sites": call_sites,
        "resolved_targets": resolved,
        "unresolved_call_sites": unresolved,
        "top_unresolved_callees": top,
    }


def _source_free_candidate(value: object, label: str) -> dict[str, object]:
    candidate = _required_dict(value, label)
    if set(candidate) != set(SOURCE_FREE_FIELDS):
        raise FinalizeError(f"{label} fields are not canonical")
    candidate_id = candidate["candidate_id"]
    identity = candidate["source_identity"]
    if not isinstance(candidate_id, str) or not candidate_id:
        raise FinalizeError(f"{label}.candidate_id is invalid")
    if not isinstance(identity, str) or not identity:
        raise FinalizeError(f"{label}.source_identity is invalid")
    source = _canonical_source(candidate["source"], f"{label}.source")
    if identity != f"{source['type']}#{source['signature']}":
        raise FinalizeError(f"{label}.source_identity does not match source fingerprint identity")
    return {
        "candidate_id": candidate_id,
        "source_identity": identity,
        "source": source,
        "source_location": _canonical_location(candidate["source_location"], f"{label}.source_location"),
        "atlas_observed_hazards": _string_list(
            candidate["atlas_observed_hazards"], f"{label}.atlas_observed_hazards"
        ),
        "atlas_classifications": _string_list(
            candidate["atlas_classifications"], f"{label}.atlas_classifications"
        ),
        "calls": _canonical_calls(candidate["calls"], f"{label}.calls"),
    }


def _source_free_from_pack_record(record: Mapping[str, object], label: str) -> dict[str, object]:
    missing = [field for field in SOURCE_FREE_FIELDS if field not in record]
    if missing:
        raise FinalizeError(f"{label} is missing source-free candidate fields: {missing}")
    return _source_free_candidate({field: record[field] for field in SOURCE_FREE_FIELDS}, label)


def _validate_pack(
    value: Mapping[str, object], digest: str
) -> tuple[dict[str, dict[str, object]], dict[str, list[str]], str]:
    required = {
        "schema": SCHEMA,
        "kind": packer.PACK_KIND,
        "review_id": packer.DISCOVERY_REVIEW_ID,
        "commit_policy": packer.PACK_COMMIT_POLICY,
        "contains_official_source_text": True,
        "source_archive_sha256": EXPECTED_SOURCE_SHA256,
        "focused_groups": list(FOCUS_GROUPS),
        "production_admitted": False,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise FinalizeError(f"review pack identity mismatch: {json.dumps(mismatches, sort_keys=True)}")
    discovery_sha = value.get("discovery_sha256")
    if not isinstance(discovery_sha, str) or len(discovery_sha) != 64:
        raise FinalizeError("review pack discovery_sha256 is invalid")
    raw_memberships = _required_dict(value.get("group_memberships"), "review pack group_memberships")
    if set(raw_memberships) != set(FOCUS_GROUPS):
        raise FinalizeError("review pack group_memberships must contain exactly the focused groups")
    memberships = {
        group_id: _string_list(
            raw_memberships[group_id], f"review pack memberships {group_id}", nonempty=True
        )
        for group_id in FOCUS_GROUPS
    }

    records: dict[str, dict[str, object]] = {}
    excerpt_bytes = 0
    for index, raw_record in enumerate(
        _required_list(value.get("source_records"), "review pack source_records")
    ):
        label = f"review pack source_records[{index}]"
        record = _required_dict(raw_record, label)
        source_free = _source_free_from_pack_record(record, label)
        identity = str(source_free["source_identity"])
        if identity in records:
            raise FinalizeError(f"duplicate review pack source identity: {identity}")
        group_ids = _string_list(record.get("group_ids"), f"{identity}.group_ids", nonempty=True)
        if any(group_id not in FOCUS_GROUPS for group_id in group_ids):
            raise FinalizeError(f"{identity}.group_ids contains a non-focused group")
        excerpt = record.get("source_excerpt")
        excerpt_sha = record.get("source_excerpt_sha256")
        if not isinstance(excerpt, str) or not isinstance(excerpt_sha, str):
            raise FinalizeError(f"{identity} review pack excerpt metadata is invalid")
        if _sha256_bytes(excerpt.encode("utf-8")) != excerpt_sha:
            raise FinalizeError(f"{identity} source excerpt digest mismatch")
        excerpt_bytes += len(excerpt.encode("utf-8"))
        records[identity] = source_free
        expected_groups = {group_id for group_id in FOCUS_GROUPS if identity in memberships[group_id]}
        if set(group_ids) != expected_groups:
            raise FinalizeError(f"{identity} group_ids disagrees with group_memberships")
    if value.get("unique_source_records") != len(records):
        raise FinalizeError("review pack unique_source_records mismatch")
    if value.get("source_excerpt_bytes") != excerpt_bytes:
        raise FinalizeError("review pack source_excerpt_bytes mismatch")
    membership_union = {identity for identities in memberships.values() for identity in identities}
    if set(records) != membership_union:
        raise FinalizeError("review pack membership/source-record identity sets differ")
    if len(digest) != 64:
        raise FinalizeError("review pack digest is invalid")
    return records, memberships, discovery_sha


def _validate_worksheet(
    value: Mapping[str, object],
    *,
    worksheet_sha: str,
    pack_sha: str,
    pack_records: Mapping[str, dict[str, object]],
    pack_memberships: Mapping[str, list[str]],
    discovery_sha: str,
) -> list[dict[str, object]]:
    required = {
        "schema": SCHEMA,
        "kind": packer.WORKSHEET_KIND,
        "review_id": packer.DISCOVERY_REVIEW_ID,
        "commit_policy": packer.WORKSHEET_COMMIT_POLICY,
        "contains_official_source_text": False,
        "source_archive_sha256": EXPECTED_SOURCE_SHA256,
        "discovery_sha256": discovery_sha,
        "review_pack_sha256": pack_sha,
        "production_admitted": False,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise FinalizeError(f"worksheet identity/link mismatch: {json.dumps(mismatches, sort_keys=True)}")
    if len(worksheet_sha) != 64:
        raise FinalizeError("worksheet digest is invalid")

    raw_groups = _required_list(value.get("groups"), "worksheet groups")
    if len(raw_groups) != len(FOCUS_GROUPS):
        raise FinalizeError("worksheet must contain exactly the focused groups")
    finalized: list[dict[str, object]] = []
    for index, expected_group_id in enumerate(FOCUS_GROUPS):
        group = _required_dict(raw_groups[index], f"worksheet groups[{index}]")
        if group.get("group_id") != expected_group_id:
            raise FinalizeError("worksheet focused groups are missing or out of canonical order")
        if group.get("source_inspected") is not True:
            raise FinalizeError(f"{expected_group_id} source_inspected must be true")
        if group.get("review_complete") is not True:
            raise FinalizeError(f"{expected_group_id} review_complete must be true")
        followups = _string_list(
            group.get("followup_dependencies"), f"{expected_group_id}.followup_dependencies"
        )
        if followups:
            raise FinalizeError(f"{expected_group_id} still has unresolved followup_dependencies")
        observations = _string_list(
            group.get("semantic_observations"),
            f"{expected_group_id}.semantic_observations",
            nonempty=True,
        )
        hazards_reviewed = _string_list(
            group.get("hazards_reviewed"), f"{expected_group_id}.hazards_reviewed"
        )
        selected = _string_list(
            group.get("selected_source_identities"),
            f"{expected_group_id}.selected_source_identities",
            nonempty=True,
        )
        rejected = _string_list(
            group.get("rejected_source_identities"),
            f"{expected_group_id}.rejected_source_identities",
        )
        if set(selected) & set(rejected):
            raise FinalizeError(f"{expected_group_id} selected/rejected identities overlap")
        expected_identities = set(pack_memberships[expected_group_id])
        if set(selected) | set(rejected) != expected_identities:
            raise FinalizeError(
                f"{expected_group_id} selected/rejected identities must exactly partition candidates"
            )

        worksheet_candidates: dict[str, dict[str, object]] = {}
        for candidate_index, raw_candidate in enumerate(
            _required_list(group.get("candidates"), f"{expected_group_id}.candidates")
        ):
            candidate = _source_free_candidate(
                raw_candidate, f"{expected_group_id}.candidates[{candidate_index}]"
            )
            identity = str(candidate["source_identity"])
            if identity in worksheet_candidates:
                raise FinalizeError(
                    f"{expected_group_id} contains duplicate candidate identity {identity}"
                )
            worksheet_candidates[identity] = candidate
            expected_candidate = pack_records.get(identity)
            if expected_candidate is None or candidate != expected_candidate:
                raise FinalizeError(f"{expected_group_id} candidate metadata drift for {identity}")
        if set(worksheet_candidates) != expected_identities:
            raise FinalizeError(f"{expected_group_id} worksheet candidate set differs from review pack")

        selected_records = [pack_records[identity] for identity in selected]
        required_hazards = {
            str(hazard)
            for record in selected_records
            for hazard in record["atlas_observed_hazards"]  # type: ignore[index]
        }
        missing_hazards = sorted(required_hazards - set(hazards_reviewed))
        if missing_hazards:
            raise FinalizeError(
                f"{expected_group_id} selected-source Atlas hazards are not fully reviewed: "
                f"{missing_hazards}"
            )
        finalized.append(
            {
                "group_id": expected_group_id,
                "selected_sources": selected_records,
                "rejected_source_identities": rejected,
                "hazards_reviewed": hazards_reviewed,
                "semantic_observations": observations,
            }
        )
    return finalized


def finalize(review_pack: Path, worksheet: Path, output: Path) -> dict[str, object]:
    pack_value, pack_sha = _read_json(review_pack, "R2C world-state review pack")
    worksheet_value, worksheet_sha = _read_json(worksheet, "R2C world-state review worksheet")
    pack_records, memberships, discovery_sha = _validate_pack(pack_value, pack_sha)
    groups = _validate_worksheet(
        worksheet_value,
        worksheet_sha=worksheet_sha,
        pack_sha=pack_sha,
        pack_records=pack_records,
        pack_memberships=memberships,
        discovery_sha=discovery_sha,
    )
    result: dict[str, object] = {
        "schema": SCHEMA,
        "kind": RESULT_KIND,
        "id": RESULT_ID,
        "commit_policy": RESULT_COMMIT_POLICY,
        "source_archive_sha256": EXPECTED_SOURCE_SHA256,
        "discovery_sha256": discovery_sha,
        "review_pack_sha256": pack_sha,
        "worksheet_sha256": worksheet_sha,
        "contains_official_source_text": False,
        "all_groups_review_complete": True,
        "groups": groups,
        "production_admitted": False,
        "next_step": (
            "Materialize canonical source-free R2C VAR/SEM records from this reviewed evidence, "
            "then run the independent vanilla source gate before production implementation."
        ),
    }
    result_bytes = _pretty_bytes(result)
    output_path = _fresh_output_file(output)
    try:
        output_path.write_bytes(result_bytes)
    except OSError as error:
        raise FinalizeError(f"cannot write review result {output_path}: {error}") from error
    selected_count = sum(len(_required_list(group["selected_sources"], "selected_sources")) for group in groups)
    return {
        "output": str(output_path),
        "sha256": _sha256_bytes(result_bytes),
        "groups": len(groups),
        "selected_sources": selected_count,
        "contains_official_source_text": False,
        "production_admitted": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--review-pack", type=Path, required=True)
    parser.add_argument("--worksheet", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        summary = finalize(args.review_pack, args.worksheet, args.output)
    except FinalizeError as error:
        print(f"R2C world-state source-review finalization failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
