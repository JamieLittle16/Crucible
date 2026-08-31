#!/usr/bin/env python3
"""Diagnose the remaining source-review/admission blockers for R2C world state.

The doctor never infers Minecraft semantics and never prints official source excerpts. It reports
which explicit human-review or semantic-authoring obligations remain, then reuses the authoritative
validators once a phase appears complete.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Mapping, Sequence

try:
    from . import r2c_world_state_admission_materialize as materialize
    from . import r2c_world_state_admission_promote as promote
    from . import r2c_world_state_source_review_finalize as finalize
except ImportError:  # Direct ``python3 tools/...`` execution.
    import r2c_world_state_admission_materialize as materialize  # type: ignore[no-redef]
    import r2c_world_state_admission_promote as promote  # type: ignore[no-redef]
    import r2c_world_state_source_review_finalize as finalize  # type: ignore[no-redef]

SCHEMA = 1
KIND = "r2c-world-state-admission-doctor"


class DoctorError(RuntimeError):
    """Invalid doctor invocation or malformed prerequisite artifact."""


def _read_json(path: Path, label: str) -> tuple[dict[str, object], str]:
    if path.is_symlink() or not path.is_file():
        raise DoctorError(f"{label} must be a real non-symlink file: {path}")
    try:
        raw = path.read_bytes()
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise DoctorError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise DoctorError(f"{label} must be a JSON object")
    return value, materialize._sha256_bytes(raw)


def _array(value: object) -> list[object]:
    return value if isinstance(value, list) else []


def _strings(value: object) -> list[str]:
    raw = _array(value)
    return [item for item in raw if isinstance(item, str) and item]


def _block(code: str, *, group: str | None = None, detail: object | None = None) -> dict[str, object]:
    result: dict[str, object] = {"code": code}
    if group is not None:
        result["group"] = group
    if detail is not None:
        result["detail"] = detail
    return result


def _base(phase: str, blockers: list[dict[str, object]], next_step: str) -> dict[str, object]:
    return {
        "schema": SCHEMA,
        "kind": KIND,
        "phase": phase,
        "contains_official_source_text": False,
        "semantic_inference_performed": False,
        "ready_for_next_step": not blockers,
        "blockers": blockers,
        "next_step": next_step,
    }


def diagnose_review(review_pack: Path, worksheet: Path) -> dict[str, object]:
    pack_value, pack_sha = finalize._read_json(review_pack, "R2C world-state review pack")
    worksheet_value, worksheet_sha = finalize._read_json(
        worksheet, "R2C world-state review worksheet"
    )
    pack_records, memberships, discovery_sha = finalize._validate_pack(pack_value, pack_sha)

    expected_identity = {
        "schema": finalize.SCHEMA,
        "kind": finalize.packer.WORKSHEET_KIND,
        "review_id": finalize.packer.DISCOVERY_REVIEW_ID,
        "commit_policy": finalize.packer.WORKSHEET_COMMIT_POLICY,
        "contains_official_source_text": False,
        "source_archive_sha256": finalize.EXPECTED_SOURCE_SHA256,
        "discovery_sha256": discovery_sha,
        "review_pack_sha256": pack_sha,
        "production_admitted": False,
    }
    blockers: list[dict[str, object]] = []
    for field, expected in expected_identity.items():
        if worksheet_value.get(field) != expected:
            blockers.append(
                _block(
                    "review-worksheet-link-drift",
                    detail={"field": field, "expected": expected, "actual": worksheet_value.get(field)},
                )
            )

    raw_groups = _array(worksheet_value.get("groups"))
    if len(raw_groups) != len(finalize.FOCUS_GROUPS):
        blockers.append(
            _block(
                "review-group-count",
                detail={"expected": len(finalize.FOCUS_GROUPS), "actual": len(raw_groups)},
            )
        )
    group_status: list[dict[str, object]] = []
    for index, group_id in enumerate(finalize.FOCUS_GROUPS):
        raw_group = raw_groups[index] if index < len(raw_groups) else None
        group = raw_group if isinstance(raw_group, dict) else {}
        expected_candidates = set(memberships[group_id])
        selected = _strings(group.get("selected_source_identities"))
        rejected = _strings(group.get("rejected_source_identities"))
        selected_set = set(selected)
        rejected_set = set(rejected)
        followups = _strings(group.get("followup_dependencies"))
        observations = _strings(group.get("semantic_observations"))
        hazards = set(_strings(group.get("hazards_reviewed")))

        if group.get("group_id") != group_id:
            blockers.append(_block("review-group-order-or-id", group=group_id))
        if group.get("source_inspected") is not True:
            blockers.append(_block("source-not-inspected", group=group_id))
        if group.get("review_complete") is not True:
            blockers.append(_block("review-not-complete", group=group_id))
        if followups:
            blockers.append(_block("followup-dependencies-open", group=group_id, detail=followups))
        if not observations:
            blockers.append(_block("semantic-observation-required", group=group_id))
        if not selected:
            blockers.append(_block("selected-source-required", group=group_id))
        overlap = sorted(selected_set & rejected_set)
        if overlap:
            blockers.append(_block("selected-rejected-overlap", group=group_id, detail=overlap))
        partition = selected_set | rejected_set
        if partition != expected_candidates:
            blockers.append(
                _block(
                    "candidate-partition-incomplete",
                    group=group_id,
                    detail={
                        "unclassified": sorted(expected_candidates - partition),
                        "unexpected": sorted(partition - expected_candidates),
                    },
                )
            )
        required_hazards = {
            str(hazard)
            for identity in selected_set & set(pack_records)
            for hazard in pack_records[identity]["atlas_observed_hazards"]  # type: ignore[index]
        }
        missing_hazards = sorted(required_hazards - hazards)
        if missing_hazards:
            blockers.append(
                _block("selected-source-hazards-unreviewed", group=group_id, detail=missing_hazards)
            )

        group_status.append(
            {
                "group_id": group_id,
                "candidates": len(expected_candidates),
                "selected": len(selected_set),
                "rejected": len(rejected_set),
                "followup_dependencies": len(followups),
                "semantic_observations": len(observations),
                "source_inspected": group.get("source_inspected") is True,
                "review_complete": group.get("review_complete") is True,
            }
        )

    if not blockers:
        try:
            finalize._validate_worksheet(
                worksheet_value,
                worksheet_sha=worksheet_sha,
                pack_sha=pack_sha,
                pack_records=pack_records,
                pack_memberships=memberships,
                discovery_sha=discovery_sha,
            )
        except finalize.FinalizeError as error:
            blockers.append(_block("authoritative-review-validation", detail=str(error)))

    result = _base(
        "source-review",
        blockers,
        (
            "Run r2c_world_state_source_review_finalize.py when ready; otherwise complete only the "
            "reported review obligations after inspecting the local source excerpts."
        ),
    )
    result["groups"] = group_status
    result["review_pack_sha256"] = pack_sha
    result["worksheet_sha256"] = worksheet_sha
    return result


def diagnose_admission(review_result: Path, worksheet: Path) -> dict[str, object]:
    review_value, review_sha = materialize._read_json(
        review_result, "R2C world-state source-review result"
    )
    worksheet_value, worksheet_sha = materialize._read_json(
        worksheet, "R2C world-state admission worksheet"
    )
    reviewed_sources = materialize._validate_review_result(review_value)
    blockers: list[dict[str, object]] = []

    if worksheet_value.get("review_result_sha256") != review_sha:
        blockers.append(_block("admission-review-result-drift"))
    if worksheet_value.get("all_groups_admission_complete") is not True:
        blockers.append(_block("all-groups-admission-incomplete"))

    raw_groups = _array(worksheet_value.get("groups"))
    seen_rule_ids: set[str] = set()
    source_rule_counts = {identity: 0 for identity in reviewed_sources}
    group_status: list[dict[str, object]] = []
    for index, group_id in enumerate(finalize.FOCUS_GROUPS):
        raw_group = raw_groups[index] if index < len(raw_groups) else None
        group = raw_group if isinstance(raw_group, dict) else {}
        expected_sources = {
            identity for identity, entry in reviewed_sources.items() if group_id in entry["groups"]
        }
        actual_sources = set(_strings(group.get("selected_source_identities")))
        rules = _array(group.get("semantic_rules"))

        if group.get("group_id") != group_id:
            blockers.append(_block("admission-group-order-or-id", group=group_id))
        if group.get("admission_complete") is not True:
            blockers.append(_block("admission-not-complete", group=group_id))
        if actual_sources != expected_sources:
            blockers.append(
                _block(
                    "admission-selected-source-drift",
                    group=group_id,
                    detail={
                        "missing": sorted(expected_sources - actual_sources),
                        "unexpected": sorted(actual_sources - expected_sources),
                    },
                )
            )
        if not rules:
            blockers.append(_block("semantic-rule-required", group=group_id))

        valid_rules = 0
        for rule_index, raw_rule in enumerate(rules):
            if not isinstance(raw_rule, dict):
                blockers.append(
                    _block("semantic-rule-invalid", group=group_id, detail={"index": rule_index})
                )
                continue
            rule_id = raw_rule.get("id")
            statement = raw_rule.get("statement")
            sources = _strings(raw_rule.get("source_identities"))
            if not isinstance(rule_id, str) or not rule_id.startswith(materialize.prepare.SEM_PREFIX):
                blockers.append(
                    _block("semantic-rule-id-invalid", group=group_id, detail={"index": rule_index})
                )
                continue
            if rule_id in seen_rule_ids:
                blockers.append(_block("semantic-rule-id-duplicate", group=group_id, detail=rule_id))
            seen_rule_ids.add(rule_id)
            if not isinstance(statement, str) or not statement or statement != statement.strip() or "\r" in statement:
                blockers.append(
                    _block(
                        "semantic-rule-statement-invalid",
                        group=group_id,
                        detail={"rule_id": rule_id},
                    )
                )
            if not sources:
                blockers.append(_block("semantic-rule-source-required", group=group_id, detail=rule_id))
            unknown = sorted(set(sources) - expected_sources)
            if unknown:
                blockers.append(
                    _block(
                        "semantic-rule-source-not-selected",
                        group=group_id,
                        detail={"rule_id": rule_id, "sources": unknown},
                    )
                )
            for identity in set(sources) & expected_sources:
                source_rule_counts[identity] += 1
            valid_rules += 1

        group_status.append(
            {
                "group_id": group_id,
                "selected_sources": len(expected_sources),
                "semantic_rules": len(rules),
                "admission_complete": group.get("admission_complete") is True,
                "valid_rule_shapes": valid_rules,
            }
        )

    unused = sorted(identity for identity, count in source_rule_counts.items() if count == 0)
    if unused:
        blockers.append(_block("selected-source-unused-by-semantic-rules", detail=unused))

    if not blockers:
        try:
            materialize._validate_worksheet(
                worksheet_value,
                worksheet_sha=worksheet_sha,
                review_sha=review_sha,
                reviewed_sources=reviewed_sources,
            )
        except materialize.MaterializeError as error:
            blockers.append(_block("authoritative-admission-validation", detail=str(error)))

    result = _base(
        "semantic-admission",
        blockers,
        (
            "Run r2c_world_state_admission_materialize.py when ready; otherwise author only the "
            "reported explicit SEM obligations. Automatic semantic inference remains forbidden."
        ),
    )
    result["groups"] = group_status
    result["review_result_sha256"] = review_sha
    result["worksheet_sha256"] = worksheet_sha
    return result


def diagnose_staging(staging_dir: Path, gate_report: Path | None) -> dict[str, object]:
    try:
        staged, manifest_raw, manifest = promote._validate_staging(staging_dir)
    except promote.PromoteError as error:
        return _base(
            "source-gate",
            [_block("staging-invalid", detail=str(error))],
            "Repair or rematerialize the source-free staging bundle before source admission.",
        )

    if gate_report is None:
        result = _base(
            "source-gate",
            [_block("manifest-bound-source-gate-required")],
            (
                "Run r2c_world_state_source_gate.py against this staging directory and the pinned "
                "Vanilla Atlas. Do not use the legacy generic gate command for R2C world-state promotion."
            ),
        )
    else:
        try:
            report, report_raw = promote._read_json(gate_report, "source-gate report")
            promote._validate_gate_report(report, report_raw, staged, manifest_raw)
        except promote.PromoteError as error:
            result = _base(
                "source-gate",
                [_block("source-gate-not-promotion-ready", detail=str(error))],
                "Re-run the manifest-bound R2C source gate after resolving the reported evidence mismatch.",
            )
        else:
            result = _base(
                "promotion-ready",
                [],
                (
                    "Run r2c_world_state_admission_promote.py. This authorizes repository evidence "
                    "promotion only; runtime biome/heightmap/light implementation remains separate."
                ),
            )
    result["materialization_manifest_sha256"] = materialize._sha256_bytes(manifest_raw)
    result["var_records"] = manifest["var_records"]
    result["semantic_rules"] = manifest["semantic_rules"]
    return result


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="phase", required=True)

    review = sub.add_parser("review", help="diagnose the local source-review worksheet")
    review.add_argument("--review-pack", type=Path, required=True)
    review.add_argument("--worksheet", type=Path, required=True)

    admission = sub.add_parser("admission", help="diagnose explicit SEM authoring")
    admission.add_argument("--review-result", type=Path, required=True)
    admission.add_argument("--worksheet", type=Path, required=True)

    staging = sub.add_parser("staging", help="diagnose source-gate/promotion readiness")
    staging.add_argument("--staging-dir", type=Path, required=True)
    staging.add_argument("--gate-report", type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.phase == "review":
            result = diagnose_review(args.review_pack, args.worksheet)
        elif args.phase == "admission":
            result = diagnose_admission(args.review_result, args.worksheet)
        else:
            result = diagnose_staging(args.staging_dir, args.gate_report)
    except (DoctorError, finalize.FinalizeError, materialize.MaterializeError) as error:
        print(f"R2C world-state admission doctor failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["ready_for_next_step"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
