#!/usr/bin/env python3
"""Bind completed R2C delegate review evidence into a completed parent world-state review.

The parent BIOMES/HEIGHTMAPS/LIGHT review and the second-order biome-palette/light-data-layer
review are deliberately completed independently. This source-free step joins those two reviewed
source sets without inferring semantics or reading official source text.

The result keeps the canonical parent review-result schema consumed by the existing admission
preparer/materializer. Reviewed delegate candidates are stripped to that canonical candidate shape
and appended only to their declared parent group. The full bound-result digest therefore becomes the
single review identity carried by later semantic authoring, while an explicit binding record keeps
the parent upload manifest and delegate-review result content-addressed.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import sys
from pathlib import Path
from typing import Mapping, Sequence

try:
    from . import r2c_world_state_delegate_closure_source_review as closure
    from . import r2c_world_state_source_review_bundle as parent_bundle
    from . import r2c_world_state_source_review_delegate_finalize as delegate_review
    from . import r2c_world_state_source_review_finalize as parent_review
    from . import r2c_world_state_source_review_pack as parent_packer
except ImportError:  # Direct `python3 tools/...` execution.
    import r2c_world_state_delegate_closure_source_review as closure  # type: ignore[no-redef]
    import r2c_world_state_source_review_bundle as parent_bundle  # type: ignore[no-redef]
    import r2c_world_state_source_review_delegate_finalize as delegate_review  # type: ignore[no-redef]
    import r2c_world_state_source_review_finalize as parent_review  # type: ignore[no-redef]
    import r2c_world_state_source_review_pack as parent_packer  # type: ignore[no-redef]

SCHEMA = 1
BINDING_KIND = "r2c-world-state-delegate-binding"
SOURCE_FREE_DELEGATE_FIELDS = parent_review.SOURCE_FREE_FIELDS


class BindError(RuntimeError):
    """Fail-closed parent/delegate review binding error."""


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
        raise BindError(f"{label} must be a real non-symlink file: {path}")
    try:
        raw = path.read_bytes()
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise BindError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise BindError(f"{label} must be a JSON object")
    return value, _sha256_bytes(raw)


def _fresh_output(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise BindError(f"output file must not already exist: {path}")
    if path.parent and path.parent.exists() and path.parent.is_symlink():
        raise BindError(f"output parent must not be a symlink: {path.parent}")
    if path.parent and not path.parent.exists():
        path.parent.mkdir(parents=True)
    return path


def _object(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise BindError(f"{label} must be an object")
    return value


def _array(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise BindError(f"{label} must be an array")
    return value


def _strings(value: object, label: str, *, nonempty: bool = False) -> list[str]:
    raw = _array(value, label)
    if any(not isinstance(item, str) or not item for item in raw):
        raise BindError(f"{label} must contain non-empty strings")
    result = [str(item) for item in raw]
    if nonempty and not result:
        raise BindError(f"{label} must not be empty")
    if len(result) != len(set(result)):
        raise BindError(f"{label} must not contain duplicates")
    return result


def _digest(value: object, label: str) -> str:
    if not isinstance(value, str) or len(value) != 64 or any(ch not in "0123456789abcdef" for ch in value):
        raise BindError(f"{label} must be lowercase SHA-256")
    return value


def _current_parent_provenance() -> dict[str, str]:
    plan_path = parent_bundle.discovery.DEFAULT_PLAN
    plan = parent_bundle.discovery._load_plan(plan_path)
    return {
        "plan_sha256": _sha256_file(plan_path),
        "frontier_sha256": _sha256_file(plan.frontier),
    }


def _validate_parent_manifest(value: Mapping[str, object]) -> dict[str, str]:
    provenance = _current_parent_provenance()
    required = {
        "schema": SCHEMA,
        "kind": parent_bundle.BUNDLE_MANIFEST_KIND,
        "commit_policy": parent_bundle.BUNDLE_MANIFEST_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": parent_packer.EXPECTED_SOURCE_SHA256,
        **provenance,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise BindError(f"parent bundle manifest provenance mismatch: {json.dumps(mismatches, sort_keys=True)}")
    result = dict(provenance)
    for field in ("discovery_sha256", "review_pack_sha256", "worksheet_sha256"):
        result[field] = _digest(value.get(field), f"parent bundle manifest {field}")
    return result


def _validate_parent_review(
    value: Mapping[str, object], manifest: Mapping[str, str]
) -> tuple[list[dict[str, object]], set[str], set[str]]:
    required = {
        "schema": SCHEMA,
        "kind": parent_review.RESULT_KIND,
        "id": parent_review.RESULT_ID,
        "commit_policy": parent_review.RESULT_COMMIT_POLICY,
        "source_archive_sha256": parent_review.EXPECTED_SOURCE_SHA256,
        "contains_official_source_text": False,
        "all_groups_review_complete": True,
        "production_admitted": False,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    for field in ("discovery_sha256", "review_pack_sha256", "worksheet_sha256"):
        actual = _digest(value.get(field), f"parent review {field}")
        expected = manifest[field]
        if actual != expected:
            mismatches[field] = {"expected": expected, "actual": actual}
    if mismatches:
        raise BindError(f"parent review/bundle mismatch: {json.dumps(mismatches, sort_keys=True)}")

    raw_groups = _array(value.get("groups"), "parent review groups")
    if len(raw_groups) != len(parent_review.FOCUS_GROUPS):
        raise BindError("parent review must contain exactly the focused groups")

    groups: list[dict[str, object]] = []
    identities: set[str] = set()
    candidate_ids: set[str] = set()
    for index, expected_group in enumerate(parent_review.FOCUS_GROUPS):
        group = _object(raw_groups[index], f"parent review groups[{index}]")
        if group.get("group_id") != expected_group:
            raise BindError("parent review groups are missing or out of canonical order")
        observations = _strings(
            group.get("semantic_observations"), f"{expected_group}.semantic_observations", nonempty=True
        )
        hazards = set(_strings(group.get("hazards_reviewed"), f"{expected_group}.hazards_reviewed"))
        sources = _array(group.get("selected_sources"), f"{expected_group}.selected_sources")
        if not sources:
            raise BindError(f"{expected_group} has no selected parent sources")
        for source_index, raw_source in enumerate(sources):
            try:
                candidate = parent_review._source_free_candidate(
                    raw_source, f"{expected_group}.selected_sources[{source_index}]"
                )
            except parent_review.FinalizeError as error:
                raise BindError(str(error)) from error
            identity = str(candidate["source_identity"])
            candidate_id = str(candidate["candidate_id"])
            if identity in identities or candidate_id in candidate_ids:
                raise BindError("parent selected source identities and candidate ids must be globally unique")
            missing_hazards = set(candidate["atlas_observed_hazards"]) - hazards  # type: ignore[arg-type]
            if missing_hazards:
                raise BindError(
                    f"{expected_group} parent review is missing selected-source hazards for {identity}: "
                    f"{sorted(missing_hazards)}"
                )
            identities.add(identity)
            candidate_ids.add(candidate_id)
        groups.append({
            "group": group,
            "observations": observations,
            "hazards": hazards,
        })
    return groups, identities, candidate_ids


def _validate_delegate_review(
    value: Mapping[str, object], parent_provenance: Mapping[str, str]
) -> list[dict[str, object]]:
    current_delegate_plan_sha = _sha256_file(closure.DEFAULT_PLAN)
    required = {
        "schema": SCHEMA,
        "kind": delegate_review.RESULT_KIND,
        "id": delegate_review.RESULT_ID,
        "commit_policy": delegate_review.RESULT_COMMIT_POLICY,
        "parent_review_id": parent_packer.DISCOVERY_REVIEW_ID,
        "source_archive_sha256": parent_packer.EXPECTED_SOURCE_SHA256,
        "contains_official_source_text": False,
        "all_groups_review_complete": True,
        "production_admitted": False,
        "plan_sha256": current_delegate_plan_sha,
        "parent_discovery_plan_sha256": parent_provenance["plan_sha256"],
        "frontier_sha256": parent_provenance["frontier_sha256"],
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise BindError(f"delegate review provenance mismatch: {json.dumps(mismatches, sort_keys=True)}")
    for field in (
        "review_pack_sha256",
        "worksheet_sha256",
        "generated_worksheet_sha256",
        "manifest_sha256",
    ):
        _digest(value.get(field), f"delegate review {field}")

    raw_groups = _array(value.get("groups"), "delegate review groups")
    if len(raw_groups) != len(closure.EXPECTED_GROUPS):
        raise BindError("delegate review must contain exactly the planned groups")

    group_headers: list[tuple[str, str, str]] = []
    for index, (expected_group, expected_parent) in enumerate(closure.EXPECTED_GROUPS):
        group = _object(raw_groups[index], f"delegate review groups[{index}]")
        if group.get("group_id") != expected_group or group.get("parent_group_id") != expected_parent:
            raise BindError("delegate review groups are missing, mis-parented or out of canonical order")
        focus = group.get("review_focus")
        if not isinstance(focus, str) or not focus:
            raise BindError(f"{expected_group}.review_focus must be non-empty")
        group_headers.append((expected_group, expected_parent, focus))

    allowed_groups = {group for group, _parent, _focus in group_headers}
    focus_by_group = {group: focus for group, _parent, focus in group_headers}
    normalized: list[dict[str, object]] = []
    seen_identities: dict[str, dict[str, object]] = {}
    seen_candidate_ids: dict[str, dict[str, object]] = {}

    for index, (group_id, parent_group, _focus) in enumerate(group_headers):
        group = _object(raw_groups[index], f"delegate review groups[{index}]")
        hazards = set(_strings(group.get("hazards_reviewed"), f"{group_id}.hazards_reviewed"))
        observations = _strings(
            group.get("semantic_observations"), f"{group_id}.semantic_observations", nonempty=True
        )
        selected = _array(group.get("selected_sources"), f"{group_id}.selected_sources")
        if not selected:
            raise BindError(f"{group_id} has no selected delegate sources")
        canonical: list[dict[str, object]] = []
        for source_index, raw_source in enumerate(selected):
            try:
                candidate = delegate_review._candidate(
                    raw_source,
                    f"{group_id}.selected_sources[{source_index}]",
                    allowed_groups=allowed_groups,
                    focus_by_group=focus_by_group,
                )
            except delegate_review.FinalizeError as error:
                raise BindError(str(error)) from error
            if group_id not in candidate["group_ids"]:  # type: ignore[operator]
                raise BindError(f"delegate source {candidate['source_identity']} is not a member of {group_id}")
            identity = str(candidate["source_identity"])
            candidate_id = str(candidate["candidate_id"])
            missing_hazards = set(candidate["atlas_observed_hazards"]) - hazards  # type: ignore[arg-type]
            if missing_hazards:
                raise BindError(
                    f"{group_id} delegate review is missing selected-source hazards for {identity}: "
                    f"{sorted(missing_hazards)}"
                )
            previous = seen_identities.get(identity)
            if previous is not None and previous != candidate:
                raise BindError(f"delegate source metadata differs across groups: {identity}")
            previous_id = seen_candidate_ids.get(candidate_id)
            if previous_id is not None and previous_id != candidate:
                raise BindError(f"delegate candidate id metadata differs across groups: {candidate_id}")
            seen_identities.setdefault(identity, candidate)
            seen_candidate_ids.setdefault(candidate_id, candidate)
            canonical.append(candidate)
        normalized.append({
            "group_id": group_id,
            "parent_group_id": parent_group,
            "hazards": hazards,
            "observations": observations,
            "selected_sources": canonical,
        })
    return normalized


def _parent_candidate(candidate: Mapping[str, object]) -> dict[str, object]:
    return {field: copy.deepcopy(candidate[field]) for field in SOURCE_FREE_DELEGATE_FIELDS}


def bind(
    *,
    parent_review_result: Path,
    parent_bundle_manifest: Path,
    delegate_review_result: Path,
    output: Path,
) -> dict[str, object]:
    parent_value, parent_sha = _read_json(parent_review_result, "parent R2C world-state review result")
    manifest_value, manifest_sha = _read_json(parent_bundle_manifest, "parent R2C bundle manifest")
    delegate_value, delegate_sha = _read_json(delegate_review_result, "R2C delegate review result")

    provenance = _validate_parent_manifest(manifest_value)
    _parent_groups, parent_identities, parent_candidate_ids = _validate_parent_review(
        parent_value, provenance
    )
    delegate_groups = _validate_delegate_review(delegate_value, provenance)

    bound = copy.deepcopy(parent_value)
    bound_groups = _array(bound.get("groups"), "bound parent groups")
    by_parent = {
        str(_object(raw, "bound parent group")["group_id"]): _object(raw, "bound parent group")
        for raw in bound_groups
    }

    delegate_source_count = 0
    for delegate_group in delegate_groups:
        parent_group_id = str(delegate_group["parent_group_id"])
        if parent_group_id not in by_parent:
            raise BindError(f"delegate group targets unknown parent group: {parent_group_id}")
        parent_group = by_parent[parent_group_id]
        parent_sources = _array(parent_group.get("selected_sources"), f"{parent_group_id}.selected_sources")
        hazards = set(_strings(parent_group.get("hazards_reviewed"), f"{parent_group_id}.hazards_reviewed"))
        observations = _strings(
            parent_group.get("semantic_observations"), f"{parent_group_id}.semantic_observations", nonempty=True
        )

        for raw_candidate in delegate_group["selected_sources"]:  # type: ignore[union-attr]
            candidate = _object(raw_candidate, "delegate selected source")
            identity = str(candidate["source_identity"])
            candidate_id = str(candidate["candidate_id"])
            if identity in parent_identities:
                raise BindError(f"delegate source identity collides with parent-selected source: {identity}")
            if candidate_id in parent_candidate_ids:
                raise BindError(f"delegate candidate id collides with parent-selected candidate: {candidate_id}")
            parent_sources.append(_parent_candidate(candidate))
            parent_identities.add(identity)
            parent_candidate_ids.add(candidate_id)
            delegate_source_count += 1

        hazards.update(delegate_group["hazards"])  # type: ignore[arg-type]
        parent_group["hazards_reviewed"] = sorted(hazards)
        for observation in delegate_group["observations"]:  # type: ignore[union-attr]
            if observation not in observations:
                observations.append(str(observation))
        parent_group["semantic_observations"] = observations

    bound["delegate_binding"] = {
        "kind": BINDING_KIND,
        "parent_review_result_sha256": parent_sha,
        "parent_bundle_manifest_sha256": manifest_sha,
        "delegate_review_result_sha256": delegate_sha,
        "delegate_review_id": delegate_review.RESULT_ID,
        "plan_sha256": provenance["plan_sha256"],
        "frontier_sha256": provenance["frontier_sha256"],
        "selected_delegate_sources": delegate_source_count,
    }
    bound["next_step"] = (
        "Prepare one source-free semantic-admission worksheet from this delegate-bound parent review, "
        "author explicit SEM rules with exact source support, then run materialization and the independent Vanilla Atlas gate."
    )

    raw = _pretty_bytes(bound)
    output = _fresh_output(output)
    output.write_bytes(raw)
    return {
        "output": str(output),
        "sha256": _sha256_bytes(raw),
        "parent_review_result_sha256": parent_sha,
        "parent_bundle_manifest_sha256": manifest_sha,
        "delegate_review_result_sha256": delegate_sha,
        "selected_delegate_sources": delegate_source_count,
        "contains_official_source_text": False,
        "production_admitted": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--parent-review-result", type=Path, required=True)
    parser.add_argument("--parent-bundle-manifest", type=Path, required=True)
    parser.add_argument("--delegate-review-result", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        summary = bind(
            parent_review_result=args.parent_review_result,
            parent_bundle_manifest=args.parent_bundle_manifest,
            delegate_review_result=args.delegate_review_result,
            output=args.output,
        )
    except (BindError, OSError) as error:
        print(f"R2C parent/delegate review binding failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
