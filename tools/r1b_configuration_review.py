#!/usr/bin/env python3
"""Prepare and finalize the bounded manual source review for R1B Configuration.

This tool never reads Mojang source and never makes a semantic judgment. `prepare` converts the
source-free review pack into a deterministic worksheet whose decisions are deliberately blank.
A reviewer must inspect the pinned source locally, then explicitly mark every candidate as inspected
and accepted, dispose every Atlas-observed hazard, and select existing R1B SEM rules. `finalize`
validates those human dispositions and promotes only those bound INDEXED drafts to VAR_REVIEWED.

The existing `vanilla_source_gate.py` remains the final independent admission check against the
pinned Atlas database after finalization.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any, Mapping, Sequence

try:
    from . import r1b_configuration_bundle_review as bundle_review
    from . import r1b_configuration_source_probe as source_probe
except ImportError:  # Direct `python3 tools/...` execution.
    import r1b_configuration_bundle_review as bundle_review  # type: ignore[no-redef]
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]

SCHEMA = 1
PLAN_ID = "REVIEW-NET-R1B-CONFIG-26_2-001"
WORKSHEET_KIND = "r1b-configuration-manual-review-worksheet"
FINAL_KIND = "r1b-configuration-reviewed-record-set"
DEFAULT_PLAN = Path("vanilla/reviews/network/r1b-configuration-review-plan.json")
DEFAULT_SEMANTICS = Path("vanilla/semantics/network/R1_CONFIGURATION_SEMANTICS.md")
SEM_PATTERN = re.compile(r"\bSEM-NET-R1B-\d{3}\b")


class ReviewError(RuntimeError):
    """Fail-closed R1B manual-review workflow error."""


def pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode(
        "utf-8"
    )


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ReviewError(f"{label} must be a JSON object")
    return value


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ReviewError(f"{label} must be a non-empty string")
    return value


def _string_list(value: object, label: str, *, allow_empty: bool = True) -> list[str]:
    if not isinstance(value, list) or any(not isinstance(item, str) or not item for item in value):
        raise ReviewError(f"{label} must be an array of non-empty strings")
    result = [str(item) for item in value]
    if not allow_empty and not result:
        raise ReviewError(f"{label} must not be empty")
    if len(result) != len(set(result)):
        raise ReviewError(f"{label} must not contain duplicates")
    return result


def _load_json(path: Path, label: str) -> tuple[dict[str, Any], bytes]:
    if path.is_symlink() or not path.is_file():
        raise ReviewError(f"{label} must be a real non-symlink file: {path}")
    raw = path.read_bytes()
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        raise ReviewError(f"invalid {label} JSON: {error}") from error
    return _object(value, label), raw


def load_semantic_rules(path: Path) -> tuple[set[str], str]:
    if path.is_symlink() or not path.is_file():
        raise ReviewError(f"semantic contract must be a real non-symlink file: {path}")
    raw = path.read_bytes()
    rules = set(SEM_PATTERN.findall(raw.decode("utf-8")))
    expected = {f"SEM-NET-R1B-{index:03d}" for index in range(1, 13)}
    if rules != expected:
        raise ReviewError(
            f"R1B semantic contract rule set drifted: expected {sorted(expected)}, got {sorted(rules)}"
        )
    return rules, sha256_bytes(raw)


def load_review_plan(path: Path, semantics_path: Path) -> tuple[dict[str, Any], bytes]:
    plan, raw = _load_json(path, "R1B review plan")
    required = {
        "schema",
        "id",
        "minecraft",
        "protocol",
        "semantic_contract",
        "capture_semantic_rules",
        "manual_review_policy",
        "candidates",
    }
    if set(plan) != required:
        raise ReviewError("R1B review plan has unexpected or missing top-level fields")
    if plan.get("schema") != SCHEMA or plan.get("id") != PLAN_ID:
        raise ReviewError("unsupported R1B review plan identity")
    if plan.get("minecraft") != "26.2" or plan.get("protocol") != 776:
        raise ReviewError("R1B review plan target does not match Minecraft 26.2 / protocol 776")
    if Path(_string(plan.get("semantic_contract"), "plan.semantic_contract")).name != semantics_path.name:
        raise ReviewError("R1B review plan points at a different semantic contract")

    all_rules, _semantic_sha = load_semantic_rules(semantics_path)
    capture_rules = set(
        _string_list(plan.get("capture_semantic_rules"), "plan.capture_semantic_rules", allow_empty=False)
    )
    if not capture_rules <= all_rules:
        raise ReviewError("review plan capture semantic rules are not present in the contract")

    policy = _object(plan.get("manual_review_policy"), "plan.manual_review_policy")
    expected_policy = {
        "source_must_be_inspected": True,
        "atlas_hazards_are_prompts_not_dispositions": True,
        "all_observed_hazards_must_be_explicitly_disposed": True,
        "semantic_rule_candidates_are_prompts_not_claims": True,
        "record_status_before_review": "INDEXED",
        "record_status_after_acceptance": "VAR_REVIEWED",
    }
    if policy != expected_policy:
        raise ReviewError("R1B review plan manual-review policy drifted")

    candidates = plan.get("candidates")
    if not isinstance(candidates, list) or len(candidates) != len(source_probe.CANDIDATES):
        raise ReviewError("R1B review plan candidate cardinality does not match source probe")
    linked_rules: set[str] = set()
    for index, ((expected_id, expected_query), raw_candidate) in enumerate(
        zip(source_probe.CANDIDATES, candidates)
    ):
        candidate = _object(raw_candidate, f"plan.candidates[{index}]")
        if set(candidate) != {"var_id", "query", "semantic_rule_candidates", "review_focus"}:
            raise ReviewError(f"plan candidate {index} has unexpected fields")
        if candidate.get("var_id") != expected_id or candidate.get("query") != expected_query:
            raise ReviewError(f"plan candidate {index} does not match the source-probe candidate set")
        rule_candidates = set(
            _string_list(
                candidate.get("semantic_rule_candidates"),
                f"plan.candidates[{index}].semantic_rule_candidates",
                allow_empty=False,
            )
        )
        if not rule_candidates <= all_rules:
            raise ReviewError(f"plan candidate {expected_id} references an unknown SEM rule")
        if rule_candidates & capture_rules:
            raise ReviewError(f"plan candidate {expected_id} links capture-only semantic evidence")
        _string_list(candidate.get("review_focus"), f"plan.candidates[{index}].review_focus", allow_empty=False)
        linked_rules.update(rule_candidates)

    source_rules = all_rules - capture_rules
    if linked_rules != source_rules:
        missing = sorted(source_rules - linked_rules)
        extra = sorted(linked_rules - source_rules)
        raise ReviewError(f"review plan source-SEM coverage is incomplete: missing={missing}, extra={extra}")
    return plan, raw


def _review_pack_manifest(review_pack: Path) -> tuple[dict[str, Any], bytes]:
    manifest, raw = _load_json(review_pack / "manifest.json", "R1B review-pack manifest")
    if manifest.get("schema") != SCHEMA or manifest.get("kind") != bundle_review.REVIEW_PACK_KIND:
        raise ReviewError("unsupported R1B review-pack manifest identity")
    if manifest.get("commit_policy") != bundle_review.COMMIT_POLICY:
        raise ReviewError("review-pack commit policy drifted")
    if manifest.get("contains_official_source_text") is not False:
        raise ReviewError("manual review workflow accepts only source-text-free review packs")
    return manifest, raw


def _pack_candidates(
    review_pack: Path,
    manifest: Mapping[str, object],
) -> list[dict[str, object]]:
    raw_candidates = manifest.get("review_candidates")
    generated = _object(manifest.get("generated"), "review-pack generated")
    record_files = generated.get("record_files")
    if not isinstance(raw_candidates, list) or not isinstance(record_files, list):
        raise ReviewError("review-pack candidates/generated records must be arrays")
    if len(raw_candidates) != len(source_probe.CANDIDATES) or len(record_files) != len(
        source_probe.CANDIDATES
    ):
        raise ReviewError("review-pack candidate/record cardinality drifted")

    record_by_id: dict[str, dict[str, object]] = {}
    for index, value in enumerate(record_files):
        item = _object(value, f"review-pack generated.record_files[{index}]")
        if set(item) != {"var_id", "path", "sha256"}:
            raise ReviewError("review-pack generated record descriptor has unexpected fields")
        var_id = _string(item.get("var_id"), "generated record var_id")
        path = review_pack / _string(item.get("path"), "generated record path")
        record, raw = _load_json(path, f"generated record {var_id}")
        if sha256_bytes(raw) != item.get("sha256"):
            raise ReviewError(f"generated record hash drifted: {var_id}")
        if record.get("id") != var_id or record.get("status") != "INDEXED":
            raise ReviewError(f"generated record {var_id} must remain the bound INDEXED draft")
        if record.get("hazards_reviewed") != [] or record.get("semantic_rules") != []:
            raise ReviewError(f"generated record {var_id} contains premature review dispositions")
        record_by_id[var_id] = record

    result: list[dict[str, object]] = []
    for index, ((expected_id, expected_query), value) in enumerate(
        zip(source_probe.CANDIDATES, raw_candidates)
    ):
        candidate = _object(value, f"review-pack review_candidates[{index}]")
        if candidate.get("var_id") != expected_id or candidate.get("query") != expected_query:
            raise ReviewError("review-pack candidate order/identity drifted")
        source = _object(candidate.get("source"), f"review-pack source {expected_id}")
        observed_hazards = _string_list(
            candidate.get("atlas_observed_hazards"), f"review-pack hazards {expected_id}"
        )
        record = record_by_id.get(expected_id)
        if record is None:
            raise ReviewError(f"review-pack generated record missing: {expected_id}")
        if record.get("source") != source or record.get("classifications") != candidate.get(
            "classifications"
        ):
            raise ReviewError(f"review-pack record/candidate source drifted: {expected_id}")
        result.append(
            {
                "var_id": expected_id,
                "query": expected_query,
                "source": source,
                "classifications": candidate.get("classifications"),
                "atlas_observed_hazards": observed_hazards,
                "record": record,
            }
        )
    return result


def prepare_worksheet(
    *,
    review_pack: Path,
    output: Path,
    plan_path: Path = DEFAULT_PLAN,
    semantics_path: Path = DEFAULT_SEMANTICS,
) -> dict[str, object]:
    """Create one deterministic blank worksheet bound to an exact source-free review pack."""
    if output.exists():
        raise ReviewError(f"worksheet output must not already exist: {output}")
    plan, plan_raw = load_review_plan(plan_path, semantics_path)
    _rules, semantics_sha = load_semantic_rules(semantics_path)
    manifest, manifest_raw = _review_pack_manifest(review_pack)
    pack_candidates = _pack_candidates(review_pack, manifest)
    plan_candidates = plan["candidates"]
    assert isinstance(plan_candidates, list)

    worksheet_candidates: list[dict[str, object]] = []
    for pack_candidate, raw_plan in zip(pack_candidates, plan_candidates):
        plan_candidate = _object(raw_plan, "plan candidate")
        worksheet_candidates.append(
            {
                "var_id": pack_candidate["var_id"],
                "query": pack_candidate["query"],
                "source": pack_candidate["source"],
                "classifications": pack_candidate["classifications"],
                "atlas_observed_hazards": pack_candidate["atlas_observed_hazards"],
                "semantic_rule_candidates": plan_candidate["semantic_rule_candidates"],
                "review_focus": plan_candidate["review_focus"],
                "decision": {
                    "source_inspected": None,
                    "accepted": None,
                    "hazards_reviewed": [],
                    "semantic_rules": [],
                    "notes": [],
                },
            }
        )

    worksheet = {
        "schema": SCHEMA,
        "kind": WORKSHEET_KIND,
        "contains_official_source_text": False,
        "review_pack_manifest_sha256": sha256_bytes(manifest_raw),
        "review_plan_sha256": sha256_bytes(plan_raw),
        "semantic_contract_sha256": semantics_sha,
        "capture_semantic_rules": plan["capture_semantic_rules"],
        "candidates": worksheet_candidates,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(pretty_bytes(worksheet))
    return worksheet


def _load_bound_worksheet(
    *,
    review_pack: Path,
    worksheet_path: Path,
    plan_path: Path,
    semantics_path: Path,
) -> tuple[dict[str, Any], list[dict[str, object]], set[str], set[str], bytes]:
    plan, plan_raw = load_review_plan(plan_path, semantics_path)
    semantic_rules, semantics_sha = load_semantic_rules(semantics_path)
    capture_rules = set(str(item) for item in plan["capture_semantic_rules"])
    manifest, manifest_raw = _review_pack_manifest(review_pack)
    pack_candidates = _pack_candidates(review_pack, manifest)
    worksheet, worksheet_raw = _load_json(worksheet_path, "R1B review worksheet")
    required = {
        "schema",
        "kind",
        "contains_official_source_text",
        "review_pack_manifest_sha256",
        "review_plan_sha256",
        "semantic_contract_sha256",
        "capture_semantic_rules",
        "candidates",
    }
    if set(worksheet) != required:
        raise ReviewError("R1B worksheet has unexpected or missing fields")
    if worksheet.get("schema") != SCHEMA or worksheet.get("kind") != WORKSHEET_KIND:
        raise ReviewError("unsupported R1B worksheet identity")
    if worksheet.get("contains_official_source_text") is not False:
        raise ReviewError("R1B worksheet must remain source-text-free")
    bindings = {
        "review_pack_manifest_sha256": sha256_bytes(manifest_raw),
        "review_plan_sha256": sha256_bytes(plan_raw),
        "semantic_contract_sha256": semantics_sha,
    }
    for key, expected in bindings.items():
        if worksheet.get(key) != expected:
            raise ReviewError(f"R1B worksheet binding drifted: {key}")
    if set(_string_list(worksheet.get("capture_semantic_rules"), "worksheet capture rules")) != capture_rules:
        raise ReviewError("R1B worksheet capture-rule boundary drifted")

    raw_candidates = worksheet.get("candidates")
    if not isinstance(raw_candidates, list) or len(raw_candidates) != len(pack_candidates):
        raise ReviewError("R1B worksheet candidate cardinality drifted")
    plan_candidates = plan["candidates"]
    assert isinstance(plan_candidates, list)
    validated: list[dict[str, object]] = []
    for index, (raw_ws, pack_candidate, raw_plan) in enumerate(
        zip(raw_candidates, pack_candidates, plan_candidates)
    ):
        ws = _object(raw_ws, f"worksheet.candidates[{index}]")
        plan_candidate = _object(raw_plan, f"plan.candidates[{index}]")
        expected_static = {
            "var_id": pack_candidate["var_id"],
            "query": pack_candidate["query"],
            "source": pack_candidate["source"],
            "classifications": pack_candidate["classifications"],
            "atlas_observed_hazards": pack_candidate["atlas_observed_hazards"],
            "semantic_rule_candidates": plan_candidate["semantic_rule_candidates"],
            "review_focus": plan_candidate["review_focus"],
        }
        if set(ws) != set(expected_static) | {"decision"}:
            raise ReviewError(f"worksheet candidate {index} has unexpected fields")
        for key, expected in expected_static.items():
            if ws.get(key) != expected:
                raise ReviewError(f"worksheet candidate {pack_candidate['var_id']} drifted: {key}")
        decision = _object(ws.get("decision"), f"worksheet decision {pack_candidate['var_id']}")
        if set(decision) != {
            "source_inspected",
            "accepted",
            "hazards_reviewed",
            "semantic_rules",
            "notes",
        }:
            raise ReviewError(f"worksheet decision {pack_candidate['var_id']} has unexpected fields")
        validated.append({**pack_candidate, "decision": decision})
    return worksheet, validated, semantic_rules, capture_rules, worksheet_raw


def finalize_review(
    *,
    review_pack: Path,
    worksheet_path: Path,
    output_dir: Path,
    plan_path: Path = DEFAULT_PLAN,
    semantics_path: Path = DEFAULT_SEMANTICS,
) -> dict[str, object]:
    """Promote only explicitly inspected/accepted worksheet entries to VAR_REVIEWED records."""
    if output_dir.exists():
        raise ReviewError(f"final output directory must not already exist: {output_dir}")
    _worksheet, candidates, semantic_rules, capture_rules, worksheet_raw = _load_bound_worksheet(
        review_pack=review_pack,
        worksheet_path=worksheet_path,
        plan_path=plan_path,
        semantics_path=semantics_path,
    )

    finalized: list[dict[str, object]] = []
    for candidate in candidates:
        var_id = str(candidate["var_id"])
        decision = _object(candidate["decision"], f"worksheet decision {var_id}")
        if decision.get("source_inspected") is not True:
            raise ReviewError(f"{var_id}: source_inspected must be explicitly true")
        if decision.get("accepted") is not True:
            raise ReviewError(f"{var_id}: accepted must be explicitly true")
        reviewed_hazards = set(
            _string_list(decision.get("hazards_reviewed"), f"{var_id}.hazards_reviewed")
        )
        observed_hazards = set(str(item) for item in candidate["atlas_observed_hazards"])
        missing_hazards = sorted(observed_hazards - reviewed_hazards)
        if missing_hazards:
            raise ReviewError(
                f"{var_id}: Atlas-observed hazards lack explicit disposition: {', '.join(missing_hazards)}"
            )
        selected_rules = set(
            _string_list(
                decision.get("semantic_rules"), f"{var_id}.semantic_rules", allow_empty=False
            )
        )
        unknown_rules = selected_rules - semantic_rules
        if unknown_rules:
            raise ReviewError(f"{var_id}: worksheet selected unknown SEM rules: {sorted(unknown_rules)}")
        capture_links = selected_rules & capture_rules
        if capture_links:
            raise ReviewError(
                f"{var_id}: source VAR cannot claim capture-only semantic rules: {sorted(capture_links)}"
            )
        notes = _string_list(decision.get("notes"), f"{var_id}.notes")
        record = dict(_object(candidate["record"], f"bound record {var_id}"))
        record["status"] = "VAR_REVIEWED"
        record["hazards_reviewed"] = sorted(reviewed_hazards)
        record["semantic_rules"] = sorted(selected_rules)
        record["notes"] = notes
        finalized.append(record)

    output_dir.mkdir(parents=True)
    records_dir = output_dir / "records"
    records_dir.mkdir()
    record_descriptors: list[dict[str, str]] = []
    for record in finalized:
        var_id = str(record["id"])
        relative = Path("records") / f"{var_id}.json"
        raw = pretty_bytes(record)
        (output_dir / relative).write_bytes(raw)
        record_descriptors.append(
            {"var_id": var_id, "path": str(relative), "sha256": sha256_bytes(raw)}
        )

    review_manifest, review_manifest_raw = _review_pack_manifest(review_pack)
    generated = _object(review_manifest.get("generated"), "review-pack generated")
    gate_descriptor = _object(generated.get("gate_file"), "review-pack gate descriptor")
    gate_source = review_pack / _string(gate_descriptor.get("path"), "review-pack gate path")
    gate, gate_raw = _load_json(gate_source, "review-pack Configuration gate")
    if sha256_bytes(gate_raw) != gate_descriptor.get("sha256"):
        raise ReviewError("review-pack Configuration gate hash drifted")
    if gate.get("minimum_status") != "VAR_REVIEWED" or gate.get("require_semantic_rules") is not True or gate.get("require_hazards_reviewed") is not True:
        raise ReviewError("review-pack Configuration gate no longer enforces the manual-review boundary")
    gate_dir = output_dir / "gate"
    gate_dir.mkdir()
    gate_relative = Path("gate") / f"{bundle_review.GATE_ID}.json"
    (output_dir / gate_relative).write_bytes(gate_raw)

    plan_raw = plan_path.read_bytes()
    semantics_raw = semantics_path.read_bytes()
    manifest = {
        "schema": SCHEMA,
        "kind": FINAL_KIND,
        "contains_official_source_text": False,
        "review_pack_manifest_sha256": sha256_bytes(review_manifest_raw),
        "worksheet_sha256": sha256_bytes(worksheet_raw),
        "review_plan_sha256": sha256_bytes(plan_raw),
        "semantic_contract_sha256": sha256_bytes(semantics_raw),
        "records": record_descriptors,
        "gate": {
            "path": str(gate_relative),
            "sha256": sha256_bytes(gate_raw),
            "suggested_repository_path": str(bundle_review.GATE_PATH),
        },
        "next_required_step": "Run tools/vanilla_source_gate.py against the pinned Atlas database and these reviewed records; do not admit Configuration unless admitted=true with no failures.",
    }
    (output_dir / "manifest.json").write_bytes(pretty_bytes(manifest))
    return manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-configuration-review")
    subparsers = parser.add_subparsers(dest="command", required=True)

    prepare = subparsers.add_parser("prepare")
    prepare.add_argument("--review-pack", required=True, type=Path)
    prepare.add_argument("--output", required=True, type=Path)
    prepare.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    prepare.add_argument("--semantics", type=Path, default=DEFAULT_SEMANTICS)

    finalize = subparsers.add_parser("finalize")
    finalize.add_argument("--review-pack", required=True, type=Path)
    finalize.add_argument("--worksheet", required=True, type=Path)
    finalize.add_argument("--output-dir", required=True, type=Path)
    finalize.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    finalize.add_argument("--semantics", type=Path, default=DEFAULT_SEMANTICS)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "prepare":
            worksheet = prepare_worksheet(
                review_pack=args.review_pack,
                output=args.output,
                plan_path=args.plan,
                semantics_path=args.semantics,
            )
            print(f"worksheet={args.output}")
            print(f"candidates={len(worksheet['candidates'])}")
            print("manual_source_review_required=true")
            return 0
        manifest = finalize_review(
            review_pack=args.review_pack,
            worksheet_path=args.worksheet,
            output_dir=args.output_dir,
            plan_path=args.plan,
            semantics_path=args.semantics,
        )
        print(f"reviewed_record_set={args.output_dir}")
        print(f"records={len(manifest['records'])}")
        print("source_gate_still_required=true")
        return 0
    except (OSError, ReviewError) as error:
        print(f"R1B Configuration manual review error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
