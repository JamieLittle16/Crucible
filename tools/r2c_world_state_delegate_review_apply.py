#!/usr/bin/env python3
"""Apply committed source-free human decisions to an R2C delegate review worksheet.

The input worksheet must be the exact untouched source-free worksheet named by the decision record.
This tool performs no source selection and no semantic inference: the committed decision file names
all selected candidate ids, explicitly declares that every omitted candidate is rejected, and
supplies the reviewed hazards and semantic observations. The output remains source-free and is not
production admission.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Mapping, Sequence

try:
    from . import r2c_world_state_delegate_closure_source_review as closure
except ImportError:  # Direct `python3 tools/...` execution.
    import r2c_world_state_delegate_closure_source_review as closure  # type: ignore[no-redef]

SCHEMA = 1
DECISION_KIND = "r2c-world-state-delegate-human-review-decisions"
DECISION_COMMIT_POLICY = "SOURCE_FREE_HUMAN_REVIEW_DECISIONS_NOT_ADMISSION"
UNSELECTED_POLICY = "REJECT"
DEFAULT_DECISIONS = (
    Path(__file__).resolve().parents[1]
    / "vanilla/reviews/network/r2c-world-state-delegate-review-decisions.json"
)


class ApplyError(RuntimeError):
    """Fail-closed delegate human-review application error."""


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
        raise ApplyError(f"{label} must be a real non-symlink file: {path}")
    try:
        raw = path.read_bytes()
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ApplyError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise ApplyError(f"{label} must be a JSON object")
    return value, raw, _sha256_bytes(raw)


def _fresh_output(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise ApplyError(f"output file must not already exist: {path}")
    parent = path.parent
    if parent and parent.exists() and parent.is_symlink():
        raise ApplyError(f"output parent must not be a symlink: {parent}")
    if parent and not parent.exists():
        try:
            parent.mkdir(parents=True)
        except OSError as error:
            raise ApplyError(f"cannot create output parent {parent}: {error}") from error
    return path


def _object(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise ApplyError(f"{label} must be an object")
    return value


def _array(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise ApplyError(f"{label} must be an array")
    return value


def _strings(value: object, label: str, *, nonempty: bool = False) -> list[str]:
    raw = _array(value, label)
    if any(not isinstance(item, str) or not item for item in raw):
        raise ApplyError(f"{label} must contain non-empty strings")
    result = [str(item) for item in raw]
    if nonempty and not result:
        raise ApplyError(f"{label} must not be empty")
    if len(result) != len(set(result)):
        raise ApplyError(f"{label} must not contain duplicates")
    return result


def _current_plan_sha() -> str:
    closure._load_plan(closure.DEFAULT_PLAN)
    return _sha256_file(closure.DEFAULT_PLAN)


def _validate_decisions(
    value: Mapping[str, object], *, worksheet_sha: str, worksheet_value: Mapping[str, object]
) -> list[dict[str, object]]:
    expected_top = {
        "schema": SCHEMA,
        "kind": DECISION_KIND,
        "review_id": closure.PLAN_ID,
        "commit_policy": DECISION_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": closure.EXPECTED_SOURCE_SHA256,
        "plan_sha256": _current_plan_sha(),
        "generated_worksheet_sha256": worksheet_sha,
        "review_pack_sha256": worksheet_value.get("review_pack_sha256"),
        "automatic_source_selection_forbidden": True,
        "unselected_candidate_policy": UNSELECTED_POLICY,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in expected_top.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise ApplyError(f"delegate review decision provenance mismatch: {json.dumps(mismatches, sort_keys=True)}")
    expected_fields = set(expected_top) | {"groups"}
    if set(value) != expected_fields:
        raise ApplyError("delegate review decision top-level fields are not canonical")

    raw_groups = _array(value.get("groups"), "delegate review decision groups")
    if len(raw_groups) != len(closure.EXPECTED_GROUPS):
        raise ApplyError("delegate review decisions must contain exactly the planned groups")
    decisions: list[dict[str, object]] = []
    for index, (expected_group, expected_parent) in enumerate(closure.EXPECTED_GROUPS):
        group = _object(raw_groups[index], f"delegate review decision groups[{index}]")
        expected_group_fields = {
            "group_id",
            "parent_group_id",
            "source_inspected",
            "selected_candidate_ids",
            "hazards_reviewed",
            "followup_dependencies",
            "semantic_observations",
        }
        if set(group) != expected_group_fields:
            raise ApplyError(f"delegate review decision groups[{index}] fields are not canonical")
        if group.get("group_id") != expected_group or group.get("parent_group_id") != expected_parent:
            raise ApplyError("delegate review decision groups are missing or out of canonical order")
        if group.get("source_inspected") is not True:
            raise ApplyError(f"{expected_group}.source_inspected must be true")
        selected = _strings(
            group.get("selected_candidate_ids"),
            f"{expected_group}.selected_candidate_ids",
            nonempty=True,
        )
        if selected != sorted(selected):
            raise ApplyError(f"{expected_group}.selected_candidate_ids must be in canonical order")
        hazards = _strings(group.get("hazards_reviewed"), f"{expected_group}.hazards_reviewed")
        followups = _strings(
            group.get("followup_dependencies"), f"{expected_group}.followup_dependencies"
        )
        if followups:
            raise ApplyError(f"{expected_group} still has unresolved followup_dependencies")
        observations = _strings(
            group.get("semantic_observations"),
            f"{expected_group}.semantic_observations",
            nonempty=True,
        )
        decisions.append(
            {
                "group_id": expected_group,
                "parent_group_id": expected_parent,
                "selected_candidate_ids": selected,
                "hazards_reviewed": hazards,
                "semantic_observations": observations,
            }
        )
    return decisions


def _validate_blank_worksheet(value: Mapping[str, object]) -> list[dict[str, object]]:
    required = {
        "schema": SCHEMA,
        "kind": closure.WORKSHEET_KIND,
        "review_id": closure.PLAN_ID,
        "parent_review_id": closure.PARENT_REVIEW_ID,
        "commit_policy": closure.WORKSHEET_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": closure.EXPECTED_SOURCE_SHA256,
        "plan_sha256": _current_plan_sha(),
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise ApplyError(f"delegate worksheet identity/provenance mismatch: {json.dumps(mismatches, sort_keys=True)}")
    review_pack_sha = value.get("review_pack_sha256")
    if not isinstance(review_pack_sha, str) or len(review_pack_sha) != 64:
        raise ApplyError("delegate worksheet review_pack_sha256 is invalid")

    raw_groups = _array(value.get("groups"), "delegate worksheet groups")
    if len(raw_groups) != len(closure.EXPECTED_GROUPS):
        raise ApplyError("delegate worksheet must contain exactly the planned groups")
    groups: list[dict[str, object]] = []
    seen_candidate_ids: set[str] = set()
    for index, (expected_group, expected_parent) in enumerate(closure.EXPECTED_GROUPS):
        group = _object(raw_groups[index], f"delegate worksheet groups[{index}]")
        if group.get("group_id") != expected_group or group.get("parent_group_id") != expected_parent:
            raise ApplyError("delegate worksheet groups are missing or out of canonical order")
        if group.get("source_inspected") is not False or group.get("review_complete") is not False:
            raise ApplyError(f"{expected_group} worksheet has already been reviewed")
        for field in (
            "selected_source_identities",
            "rejected_source_identities",
            "hazards_reviewed",
            "followup_dependencies",
            "semantic_observations",
        ):
            if _array(group.get(field), f"{expected_group}.{field}"):
                raise ApplyError(f"{expected_group}.{field} must be blank before decision application")

        candidates = _array(group.get("candidates"), f"{expected_group}.candidates")
        if not candidates:
            raise ApplyError(f"{expected_group} contains no candidates")
        candidate_by_id: dict[str, dict[str, object]] = {}
        for candidate_index, raw_candidate in enumerate(candidates):
            candidate = _object(raw_candidate, f"{expected_group}.candidates[{candidate_index}]")
            candidate_id = candidate.get("candidate_id")
            identity = candidate.get("source_identity")
            if not isinstance(candidate_id, str) or not candidate_id:
                raise ApplyError(f"{expected_group}.candidates[{candidate_index}].candidate_id is invalid")
            if not isinstance(identity, str) or not identity:
                raise ApplyError(f"{expected_group}.candidates[{candidate_index}].source_identity is invalid")
            if candidate_id in candidate_by_id or candidate_id in seen_candidate_ids:
                raise ApplyError(f"duplicate delegate candidate id: {candidate_id}")
            group_ids = _strings(
                candidate.get("group_ids"),
                f"{expected_group}.candidates[{candidate_index}].group_ids",
                nonempty=True,
            )
            if expected_group not in group_ids:
                raise ApplyError(f"{candidate_id} does not belong to {expected_group}")
            _strings(
                candidate.get("atlas_observed_hazards"),
                f"{expected_group}.candidates[{candidate_index}].atlas_observed_hazards",
            )
            candidate_by_id[candidate_id] = candidate
            seen_candidate_ids.add(candidate_id)
        groups.append(
            {
                "group": group,
                "candidate_by_id": candidate_by_id,
                "candidate_order": [str(_object(item, "candidate")["candidate_id"]) for item in candidates],
            }
        )
    return groups


def apply(worksheet: Path, decisions: Path, output: Path) -> dict[str, object]:
    worksheet_value, _worksheet_bytes, worksheet_sha = _read_json(
        worksheet, "delegate source-free worksheet"
    )
    decisions_value, _decisions_bytes, decisions_sha = _read_json(
        decisions, "delegate human-review decisions"
    )
    worksheet_groups = _validate_blank_worksheet(worksheet_value)
    decision_groups = _validate_decisions(
        decisions_value, worksheet_sha=worksheet_sha, worksheet_value=worksheet_value
    )

    selected_total = 0
    rejected_total = 0
    for worksheet_group, decision in zip(worksheet_groups, decision_groups, strict=True):
        group = worksheet_group["group"]
        assert isinstance(group, dict)
        candidate_by_id = worksheet_group["candidate_by_id"]
        assert isinstance(candidate_by_id, dict)
        candidate_order = worksheet_group["candidate_order"]
        assert isinstance(candidate_order, list)
        selected_ids = decision["selected_candidate_ids"]
        assert isinstance(selected_ids, list)
        unknown = sorted(set(selected_ids) - set(candidate_by_id))
        if unknown:
            raise ApplyError(f"{decision['group_id']} selected unknown candidate ids: {unknown}")

        selected_id_set = set(selected_ids)
        selected_identities = [str(candidate_by_id[candidate_id]["source_identity"]) for candidate_id in selected_ids]
        rejected_identities = [
            str(candidate_by_id[candidate_id]["source_identity"])
            for candidate_id in candidate_order
            if candidate_id not in selected_id_set
        ]
        required_hazards = sorted(
            {
                str(hazard)
                for candidate_id in selected_ids
                for hazard in candidate_by_id[candidate_id]["atlas_observed_hazards"]
            }
        )
        if decision["hazards_reviewed"] != required_hazards:
            raise ApplyError(
                f"{decision['group_id']} hazards_reviewed must exactly match selected-source hazards"
            )

        group["source_inspected"] = True
        group["selected_source_identities"] = selected_identities
        group["rejected_source_identities"] = rejected_identities
        group["hazards_reviewed"] = decision["hazards_reviewed"]
        group["followup_dependencies"] = []
        group["semantic_observations"] = decision["semantic_observations"]
        group["review_complete"] = True
        selected_total += len(selected_identities)
        rejected_total += len(rejected_identities)

    raw = _pretty_bytes(worksheet_value)
    output = _fresh_output(output)
    try:
        output.write_bytes(raw)
    except OSError as error:
        raise ApplyError(f"cannot write completed delegate worksheet {output}: {error}") from error
    return {
        "output": str(output),
        "sha256": _sha256_bytes(raw),
        "decision_sha256": decisions_sha,
        "groups": len(decision_groups),
        "selected_sources": selected_total,
        "rejected_sources": rejected_total,
        "contains_official_source_text": False,
        "production_admitted": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worksheet", type=Path, required=True)
    parser.add_argument("--decisions", type=Path, default=DEFAULT_DECISIONS)
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        summary = apply(args.worksheet, args.decisions, args.output)
    except (ApplyError, OSError) as error:
        print(f"R2C delegate human-review application failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
