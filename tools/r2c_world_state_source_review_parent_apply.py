#!/usr/bin/env python3
"""Apply source-free human decisions to the parent R2C world-state review worksheet.

The source-rich parent dossier remains ephemeral. A reviewer records only explicit source-free
candidate selections, hazard closure and semantic observations in a small decision file. This tool
binds those decisions to the exact current provenance-sealed parent bundle manifest and untouched
worksheet, rejects every omitted candidate by policy, and emits the completed source-free worksheet
expected by ``r2c_world_state_source_review_finalize.py``.

No source selection or semantic inference occurs here, and the output is not production admission.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Mapping, Sequence

try:
    from . import r2c_world_state_source_review_bundle as bundle
    from . import r2c_world_state_source_review_finalize as finalize
    from . import r2c_world_state_source_review_pack as packer
except ImportError:  # Direct `python3 tools/...` execution.
    import r2c_world_state_source_review_bundle as bundle  # type: ignore[no-redef]
    import r2c_world_state_source_review_finalize as finalize  # type: ignore[no-redef]
    import r2c_world_state_source_review_pack as packer  # type: ignore[no-redef]

SCHEMA = 1
DECISION_KIND = "r2c-world-state-parent-human-review-decisions"
DECISION_COMMIT_POLICY = "SOURCE_FREE_HUMAN_REVIEW_DECISIONS_NOT_ADMISSION"
UNSELECTED_POLICY = "REJECT"
DEFAULT_DECISIONS = (
    Path(__file__).resolve().parents[1]
    / "vanilla/reviews/network/r2c-world-state-parent-review-decisions.json"
)


class ApplyError(RuntimeError):
    """Fail-closed parent world-state review decision error."""


def _pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _read_json(path: Path, label: str) -> tuple[dict[str, object], str]:
    if path.is_symlink() or not path.is_file():
        raise ApplyError(f"{label} must be a real non-symlink file: {path}")
    try:
        raw = path.read_bytes()
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ApplyError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise ApplyError(f"{label} must be a JSON object")
    return value, _sha256_bytes(raw)


def _fresh_output(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise ApplyError(f"output file must not already exist: {path}")
    if path.parent and path.parent.exists() and path.parent.is_symlink():
        raise ApplyError(f"output parent must not be a symlink: {path.parent}")
    if path.parent and not path.parent.exists():
        path.parent.mkdir(parents=True)
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


def _digest(value: object, label: str) -> str:
    if not isinstance(value, str) or len(value) != 64 or any(ch not in "0123456789abcdef" for ch in value):
        raise ApplyError(f"{label} must be lowercase SHA-256")
    return value


def _current_provenance() -> dict[str, str]:
    plan_path = bundle.discovery.DEFAULT_PLAN
    plan = bundle.discovery._load_plan(plan_path)
    return {
        "plan_sha256": _sha256_file(plan_path),
        "frontier_sha256": _sha256_file(plan.frontier),
    }


def _validate_manifest(value: Mapping[str, object]) -> dict[str, str]:
    provenance = _current_provenance()
    required = {
        "schema": SCHEMA,
        "kind": bundle.BUNDLE_MANIFEST_KIND,
        "commit_policy": bundle.BUNDLE_MANIFEST_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": packer.EXPECTED_SOURCE_SHA256,
        **provenance,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise ApplyError(f"parent bundle manifest provenance mismatch: {json.dumps(mismatches, sort_keys=True)}")
    result = dict(provenance)
    for field in ("discovery_sha256", "review_pack_sha256", "worksheet_sha256"):
        result[field] = _digest(value.get(field), f"parent bundle manifest {field}")
    return result


def _validate_blank_worksheet(
    value: Mapping[str, object], *, worksheet_sha: str, manifest: Mapping[str, str]
) -> list[dict[str, object]]:
    required = {
        "schema": SCHEMA,
        "kind": packer.WORKSHEET_KIND,
        "review_id": packer.DISCOVERY_REVIEW_ID,
        "commit_policy": packer.WORKSHEET_COMMIT_POLICY,
        "contains_official_source_text": False,
        "source_archive_sha256": packer.EXPECTED_SOURCE_SHA256,
        "production_admitted": False,
        "discovery_sha256": manifest["discovery_sha256"],
        "review_pack_sha256": manifest["review_pack_sha256"],
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if worksheet_sha != manifest["worksheet_sha256"]:
        mismatches["worksheet_sha256"] = {
            "expected": manifest["worksheet_sha256"],
            "actual": worksheet_sha,
        }
    if mismatches:
        raise ApplyError(f"parent worksheet/bundle mismatch: {json.dumps(mismatches, sort_keys=True)}")

    raw_groups = _array(value.get("groups"), "parent worksheet groups")
    if len(raw_groups) != len(finalize.FOCUS_GROUPS):
        raise ApplyError("parent worksheet must contain exactly the focused groups")

    groups: list[dict[str, object]] = []
    global_candidates: dict[str, dict[str, object]] = {}
    for index, expected_group in enumerate(finalize.FOCUS_GROUPS):
        group = _object(raw_groups[index], f"parent worksheet groups[{index}]")
        if group.get("group_id") != expected_group:
            raise ApplyError("parent worksheet groups are missing or out of canonical order")
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
        candidate_order: list[str] = []
        for candidate_index, raw_candidate in enumerate(candidates):
            try:
                candidate = finalize._source_free_candidate(
                    raw_candidate, f"{expected_group}.candidates[{candidate_index}]"
                )
            except finalize.FinalizeError as error:
                raise ApplyError(str(error)) from error
            candidate_id = str(candidate["candidate_id"])
            if candidate_id in candidate_by_id:
                raise ApplyError(f"duplicate candidate id inside {expected_group}: {candidate_id}")
            previous = global_candidates.get(candidate_id)
            if previous is not None and previous != candidate:
                raise ApplyError(f"shared parent candidate metadata differs across groups: {candidate_id}")
            global_candidates.setdefault(candidate_id, candidate)
            candidate_by_id[candidate_id] = candidate
            candidate_order.append(candidate_id)
        groups.append({
            "group": group,
            "candidate_by_id": candidate_by_id,
            "candidate_order": candidate_order,
        })
    return groups


def _validate_decisions(
    value: Mapping[str, object],
    *,
    worksheet_sha: str,
    manifest_sha: str,
    manifest: Mapping[str, str],
) -> list[dict[str, object]]:
    expected_top = {
        "schema": SCHEMA,
        "kind": DECISION_KIND,
        "review_id": packer.DISCOVERY_REVIEW_ID,
        "commit_policy": DECISION_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": packer.EXPECTED_SOURCE_SHA256,
        "plan_sha256": manifest["plan_sha256"],
        "frontier_sha256": manifest["frontier_sha256"],
        "bundle_manifest_sha256": manifest_sha,
        "generated_worksheet_sha256": worksheet_sha,
        "discovery_sha256": manifest["discovery_sha256"],
        "review_pack_sha256": manifest["review_pack_sha256"],
        "automatic_source_selection_forbidden": True,
        "unselected_candidate_policy": UNSELECTED_POLICY,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in expected_top.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise ApplyError(f"parent review decision provenance mismatch: {json.dumps(mismatches, sort_keys=True)}")
    if set(value) != set(expected_top) | {"groups"}:
        raise ApplyError("parent review decision top-level fields are not canonical")

    raw_groups = _array(value.get("groups"), "parent review decision groups")
    if len(raw_groups) != len(finalize.FOCUS_GROUPS):
        raise ApplyError("parent review decisions must contain exactly the focused groups")
    decisions: list[dict[str, object]] = []
    for index, expected_group in enumerate(finalize.FOCUS_GROUPS):
        group = _object(raw_groups[index], f"parent review decision groups[{index}]")
        fields = {
            "group_id",
            "source_inspected",
            "selected_candidate_ids",
            "hazards_reviewed",
            "followup_dependencies",
            "semantic_observations",
        }
        if set(group) != fields:
            raise ApplyError(f"parent review decision groups[{index}] fields are not canonical")
        if group.get("group_id") != expected_group:
            raise ApplyError("parent review decision groups are missing or out of canonical order")
        if group.get("source_inspected") is not True:
            raise ApplyError(f"{expected_group}.source_inspected must be true")
        selected = _strings(
            group.get("selected_candidate_ids"), f"{expected_group}.selected_candidate_ids", nonempty=True
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
            group.get("semantic_observations"), f"{expected_group}.semantic_observations", nonempty=True
        )
        decisions.append({
            "group_id": expected_group,
            "selected_candidate_ids": selected,
            "hazards_reviewed": hazards,
            "semantic_observations": observations,
        })
    return decisions


def apply(
    *,
    worksheet: Path,
    bundle_manifest: Path,
    decisions: Path,
    output: Path,
) -> dict[str, object]:
    worksheet_value, worksheet_sha = _read_json(worksheet, "parent source-free worksheet")
    manifest_value, manifest_sha = _read_json(bundle_manifest, "parent bundle manifest")
    decisions_value, decisions_sha = _read_json(decisions, "parent human-review decisions")

    manifest = _validate_manifest(manifest_value)
    worksheet_groups = _validate_blank_worksheet(
        worksheet_value, worksheet_sha=worksheet_sha, manifest=manifest
    )
    decision_groups = _validate_decisions(
        decisions_value,
        worksheet_sha=worksheet_sha,
        manifest_sha=manifest_sha,
        manifest=manifest,
    )

    selected_total = 0
    rejected_total = 0
    for worksheet_group, decision in zip(worksheet_groups, decision_groups, strict=True):
        group = _object(worksheet_group["group"], "parent worksheet group")
        candidate_by_id = _object(worksheet_group["candidate_by_id"], "candidate map")
        candidate_order = _array(worksheet_group["candidate_order"], "candidate order")
        selected_ids = _strings(
            decision["selected_candidate_ids"], f"{decision['group_id']}.selected candidate ids"
        )
        unknown = sorted(set(selected_ids) - set(candidate_by_id))
        if unknown:
            raise ApplyError(f"{decision['group_id']} selected unknown candidate ids: {unknown}")

        selected_set = set(selected_ids)
        selected_identities = [str(_object(candidate_by_id[candidate_id], "candidate")["source_identity"]) for candidate_id in selected_ids]
        rejected_identities = [
            str(_object(candidate_by_id[str(candidate_id)], "candidate")["source_identity"])
            for candidate_id in candidate_order
            if str(candidate_id) not in selected_set
        ]
        required_hazards = sorted({
            str(hazard)
            for candidate_id in selected_ids
            for hazard in _array(
                _object(candidate_by_id[candidate_id], "candidate").get("atlas_observed_hazards"),
                "candidate hazards",
            )
        })
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
    output.write_bytes(raw)
    return {
        "output": str(output),
        "sha256": _sha256_bytes(raw),
        "decision_sha256": decisions_sha,
        "bundle_manifest_sha256": manifest_sha,
        "groups": len(decision_groups),
        "selected_sources": selected_total,
        "rejected_sources": rejected_total,
        "contains_official_source_text": False,
        "production_admitted": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worksheet", type=Path, required=True)
    parser.add_argument("--bundle-manifest", type=Path, required=True)
    parser.add_argument("--decisions", type=Path, default=DEFAULT_DECISIONS)
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = apply(
            worksheet=args.worksheet,
            bundle_manifest=args.bundle_manifest,
            decisions=args.decisions,
            output=args.output,
        )
    except (ApplyError, OSError) as error:
        print(f"R2C parent human-review application failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
