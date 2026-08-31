#!/usr/bin/env python3
"""Materialize reviewed R2C biome/height/light semantics into source-free VAR/SEM/gate staging.

This is deliberately the last step *before* independent source admission. It consumes the exact
source-free review result plus a human-authored semantic-admission worksheet. It never reads official
source text and never sets production admission itself.

The output directory is fresh, external to the repository, and contains:

- ``records/*.json``: canonical VAR_REVIEWED source records;
- ``semantics/R2C_WORLD_STATE_SEMANTICS.md``: rendered human-authored semantic rules;
- ``gate.json``: a candidate input for ``tools/vanilla_source_gate.py``;
- ``manifest.json``: content-addressed staging provenance.

Only a subsequent successful ``vanilla_source_gate.py`` run against the pinned Atlas may admit these
facts for production use.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Mapping, Sequence

try:
    from . import r2c_world_state_admission_prepare as prepare
    from . import r2c_world_state_source_review_finalize as review
except ImportError:  # Direct ``python3 tools/...`` execution.
    import r2c_world_state_admission_prepare as prepare  # type: ignore[no-redef]
    import r2c_world_state_source_review_finalize as review  # type: ignore[no-redef]

SCHEMA = 1
KIND = "r2c-world-state-admission-materialization"
ID = "MATERIALIZE-NET-R2C-WORLD-STATE-26_2-001"
GATE_ID = "GATE-NET-R2C-WORLD-STATE-26_2-001"
FRONTIER = "r2c-world-projection"
SEMANTICS_FILE = "R2C_WORLD_STATE_SEMANTICS.md"
COMMIT_POLICY = "SOURCE_FREE_STAGING_REQUIRES_INDEPENDENT_GATE"
REPO_ROOT = Path(__file__).resolve().parents[1]


class MaterializeError(RuntimeError):
    """Fail-closed R2C world-state admission materialization error."""


def _pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _read_json(path: Path, label: str) -> tuple[dict[str, object], str]:
    if path.is_symlink() or not path.is_file():
        raise MaterializeError(f"{label} must be a real non-symlink file: {path}")
    try:
        raw = path.read_bytes()
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise MaterializeError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise MaterializeError(f"{label} must be a JSON object")
    return value, _sha256_bytes(raw)


def _fresh_external_dir(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise MaterializeError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repository = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repository)
    except ValueError:
        return resolved
    raise MaterializeError("R2C admission staging must live outside the repository until gated")


def _object(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise MaterializeError(f"{label} must be an object")
    return value


def _array(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise MaterializeError(f"{label} must be an array")
    return value


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise MaterializeError(f"{label} must be a non-empty string")
    return value


def _string_array(value: object, label: str, *, nonempty: bool = False) -> list[str]:
    raw = _array(value, label)
    if any(not isinstance(item, str) or not item for item in raw):
        raise MaterializeError(f"{label} must contain non-empty strings")
    result = [str(item) for item in raw]
    if nonempty and not result:
        raise MaterializeError(f"{label} must not be empty")
    if len(result) != len(set(result)):
        raise MaterializeError(f"{label} must not contain duplicates")
    return result


def _canonical_source(value: object, label: str) -> dict[str, str]:
    source = _object(value, label)
    fields = {
        "type",
        "signature",
        "fingerprint_algorithm",
        "normalized_sha256",
        "body_sha256",
    }
    if set(source) != fields:
        raise MaterializeError(f"{label} fields are not canonical")
    result = {field: _string(source[field], f"{label}.{field}") for field in fields}
    if result["fingerprint_algorithm"] != "java-token-v2-literal-sensitive":
        raise MaterializeError(f"{label}.fingerprint_algorithm mismatch")
    for field in ("normalized_sha256", "body_sha256"):
        digest = result[field]
        if len(digest) != 64 or any(character not in "0123456789abcdef" for character in digest):
            raise MaterializeError(f"{label}.{field} must be lowercase SHA-256")
    return result


def _canonical_candidate(value: object, label: str) -> dict[str, object]:
    candidate = _object(value, label)
    required = {
        "candidate_id",
        "source_identity",
        "source",
        "source_location",
        "atlas_observed_hazards",
        "atlas_classifications",
        "calls",
    }
    if set(candidate) != required:
        raise MaterializeError(f"{label} fields are not canonical")
    candidate_id = _string(candidate["candidate_id"], f"{label}.candidate_id")
    identity = _string(candidate["source_identity"], f"{label}.source_identity")
    source = _canonical_source(candidate["source"], f"{label}.source")
    if identity != f"{source['type']}#{source['signature']}":
        raise MaterializeError(f"{label}.source_identity mismatch")
    location = _object(candidate["source_location"], f"{label}.source_location")
    if set(location) != {"path", "start_line", "end_line"}:
        raise MaterializeError(f"{label}.source_location fields are not canonical")
    path = _string(location["path"], f"{label}.source_location.path")
    start = location["start_line"]
    end = location["end_line"]
    if type(start) is not int or type(end) is not int or start < 1 or end < start:
        raise MaterializeError(f"{label}.source_location line range is invalid")
    hazards = _string_array(candidate["atlas_observed_hazards"], f"{label}.atlas_observed_hazards")
    classifications = _string_array(candidate["atlas_classifications"], f"{label}.atlas_classifications")
    calls = _object(candidate["calls"], f"{label}.calls")
    if set(calls) != {
        "call_sites",
        "resolved_targets",
        "unresolved_call_sites",
        "top_unresolved_callees",
    }:
        raise MaterializeError(f"{label}.calls fields are not canonical")
    return {
        "candidate_id": candidate_id,
        "source_identity": identity,
        "source": source,
        "source_location": {"path": path, "start_line": start, "end_line": end},
        "atlas_observed_hazards": hazards,
        "atlas_classifications": classifications,
        "calls": calls,
    }


def _validate_review_result(value: Mapping[str, object]) -> dict[str, dict[str, object]]:
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
        raise MaterializeError(f"review result identity mismatch: {json.dumps(mismatches, sort_keys=True)}")

    groups = _array(value.get("groups"), "review result groups")
    if len(groups) != len(review.FOCUS_GROUPS):
        raise MaterializeError("review result must contain exactly the focused groups")
    selected: dict[str, dict[str, object]] = {}
    for index, group_id in enumerate(review.FOCUS_GROUPS):
        group = _object(groups[index], f"review result groups[{index}]")
        if group.get("group_id") != group_id:
            raise MaterializeError("review result groups are missing or out of canonical order")
        reviewed_hazards = set(
            _string_array(group.get("hazards_reviewed"), f"{group_id}.hazards_reviewed")
        )
        sources = _array(group.get("selected_sources"), f"{group_id}.selected_sources")
        if not sources:
            raise MaterializeError(f"{group_id} has no selected sources")
        for source_index, raw_source in enumerate(sources):
            candidate = _canonical_candidate(raw_source, f"{group_id}.selected_sources[{source_index}]")
            identity = str(candidate["source_identity"])
            missing = set(candidate["atlas_observed_hazards"]) - reviewed_hazards  # type: ignore[arg-type]
            if missing:
                raise MaterializeError(f"{group_id} review result is missing reviewed hazards for {identity}: {sorted(missing)}")
            existing = selected.get(identity)
            if existing is not None and existing["candidate"] != candidate:
                raise MaterializeError(f"selected source metadata differs across groups: {identity}")
            entry = selected.setdefault(
                identity,
                {"candidate": candidate, "groups": [], "hazards_reviewed": set()},
            )
            entry["groups"].append(group_id)  # type: ignore[union-attr]
            entry["hazards_reviewed"].update(reviewed_hazards)  # type: ignore[union-attr]
    return selected


def _semantic_rule(value: object, label: str, allowed_sources: set[str]) -> dict[str, object]:
    rule = _object(value, label)
    if set(rule) != {"id", "statement", "source_identities"}:
        raise MaterializeError(f"{label} must contain exactly id, statement and source_identities")
    rule_id = _string(rule["id"], f"{label}.id")
    if not rule_id.startswith(prepare.SEM_PREFIX):
        raise MaterializeError(f"{label}.id must use {prepare.SEM_PREFIX}")
    statement = _string(rule["statement"], f"{label}.statement")
    if statement != statement.strip() or "\r" in statement:
        raise MaterializeError(f"{label}.statement must be trimmed canonical text")
    sources = _string_array(rule["source_identities"], f"{label}.source_identities", nonempty=True)
    unknown = sorted(set(sources) - allowed_sources)
    if unknown:
        raise MaterializeError(f"{label} cites source identities not selected for this group: {unknown}")
    return {"id": rule_id, "statement": statement, "source_identities": sources}


def _validate_worksheet(
    value: Mapping[str, object],
    *,
    worksheet_sha: str,
    review_sha: str,
    reviewed_sources: Mapping[str, dict[str, object]],
) -> tuple[list[dict[str, object]], dict[str, set[str]]]:
    required = {
        "schema": SCHEMA,
        "kind": prepare.KIND,
        "id": prepare.ID,
        "commit_policy": prepare.COMMIT_POLICY,
        "review_result_sha256": review_sha,
        "source_archive_sha256": review.EXPECTED_SOURCE_SHA256,
        "contains_official_source_text": False,
        "all_groups_admission_complete": True,
        "production_admitted": False,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise MaterializeError(f"admission worksheet identity/completion mismatch: {json.dumps(mismatches, sort_keys=True)}")
    if len(worksheet_sha) != 64:
        raise MaterializeError("admission worksheet digest is invalid")
    contract = _object(value.get("semantic_rule_contract"), "semantic_rule_contract")
    if contract.get("automatic_semantic_inference_forbidden") is not True:
        raise MaterializeError("automatic semantic inference must remain forbidden")
    if contract.get("source_support_must_be_selected") is not True:
        raise MaterializeError("semantic rule source support must remain selected-source-only")

    selected_entries = _array(value.get("selected_sources"), "selected_sources")
    worksheet_sources: dict[str, dict[str, object]] = {}
    var_ids: set[str] = set()
    for index, raw_entry in enumerate(selected_entries):
        entry = _object(raw_entry, f"selected_sources[{index}]")
        if set(entry) != {"var_id", "source_identity", "candidate"}:
            raise MaterializeError(f"selected_sources[{index}] fields are not canonical")
        var_id = _string(entry["var_id"], f"selected_sources[{index}].var_id")
        identity = _string(entry["source_identity"], f"selected_sources[{index}].source_identity")
        candidate = _canonical_candidate(entry["candidate"], f"selected_sources[{index}].candidate")
        if identity != candidate["source_identity"]:
            raise MaterializeError(f"selected_sources[{index}] identity/candidate mismatch")
        expected = reviewed_sources.get(identity)
        if expected is None or expected["candidate"] != candidate:
            raise MaterializeError(f"worksheet selected source is not the exact reviewed source: {identity}")
        if var_id != prepare._var_id(str(candidate["candidate_id"])):
            raise MaterializeError(f"worksheet VAR id drift for {identity}")
        if var_id in var_ids or identity in worksheet_sources:
            raise MaterializeError("worksheet selected source/VAR identities must be unique")
        var_ids.add(var_id)
        worksheet_sources[identity] = {"var_id": var_id, "candidate": candidate}
    if set(worksheet_sources) != set(reviewed_sources):
        raise MaterializeError("worksheet selected-source set differs from completed source review")

    raw_groups = _array(value.get("groups"), "admission worksheet groups")
    if len(raw_groups) != len(review.FOCUS_GROUPS):
        raise MaterializeError("admission worksheet must contain exactly the focused groups")
    rules: list[dict[str, object]] = []
    seen_rule_ids: set[str] = set()
    rules_by_source: dict[str, set[str]] = {identity: set() for identity in worksheet_sources}
    for index, group_id in enumerate(review.FOCUS_GROUPS):
        group = _object(raw_groups[index], f"groups[{index}]")
        if group.get("group_id") != group_id:
            raise MaterializeError("admission worksheet groups are missing or out of canonical order")
        if group.get("admission_complete") is not True:
            raise MaterializeError(f"{group_id}.admission_complete must be true")
        group_sources = set(
            _string_array(
                group.get("selected_source_identities"),
                f"{group_id}.selected_source_identities",
                nonempty=True,
            )
        )
        expected_group_sources = {
            identity for identity, source in reviewed_sources.items() if group_id in source["groups"]
        }
        if group_sources != expected_group_sources:
            raise MaterializeError(f"{group_id} selected source set differs from completed source review")
        raw_rules = _array(group.get("semantic_rules"), f"{group_id}.semantic_rules")
        if not raw_rules:
            raise MaterializeError(f"{group_id} must contain at least one semantic rule")
        for rule_index, raw_rule in enumerate(raw_rules):
            rule = _semantic_rule(raw_rule, f"{group_id}.semantic_rules[{rule_index}]", group_sources)
            rule_id = str(rule["id"])
            if rule_id in seen_rule_ids:
                raise MaterializeError(f"duplicate semantic rule id: {rule_id}")
            seen_rule_ids.add(rule_id)
            rendered = dict(rule)
            rendered["group_id"] = group_id
            rules.append(rendered)
            for identity in rule["source_identities"]:  # type: ignore[union-attr]
                rules_by_source[str(identity)].add(rule_id)

    unused = sorted(identity for identity, linked in rules_by_source.items() if not linked)
    if unused:
        raise MaterializeError(f"every selected source must support at least one semantic rule: {unused}")
    return rules, rules_by_source


def _render_semantics(rules: Sequence[Mapping[str, object]], worksheet_sha: str) -> bytes:
    lines = [
        "# R2C World-State Semantics — Minecraft Java 26.2",
        "",
        "> Generated from a human-authored, source-free admission worksheet. This file contains no",
        "> official source text. Independent Vanilla Atlas admission is still required.",
        "",
        f"- Admission worksheet SHA-256: `{worksheet_sha}`",
        f"- Source archive SHA-256: `{review.EXPECTED_SOURCE_SHA256}`",
        "- Production admitted by this materialization: **no**",
        "",
    ]
    current_group = None
    for rule in rules:
        group_id = str(rule["group_id"])
        if group_id != current_group:
            lines.extend((f"## {group_id}", ""))
            current_group = group_id
        lines.extend((
            f"### {rule['id']}",
            "",
            str(rule["statement"]),
            "",
            "Source support:",
        ))
        lines.extend(f"- `{identity}`" for identity in rule["source_identities"])  # type: ignore[union-attr]
        lines.append("")
    return ("\n".join(lines).rstrip() + "\n").encode("utf-8")


def materialize(review_result: Path, worksheet: Path, output_dir: Path) -> dict[str, object]:
    review_value, review_sha = _read_json(review_result, "R2C world-state source-review result")
    worksheet_value, worksheet_sha = _read_json(worksheet, "R2C world-state admission worksheet")
    reviewed_sources = _validate_review_result(review_value)
    rules, rules_by_source = _validate_worksheet(
        worksheet_value,
        worksheet_sha=worksheet_sha,
        review_sha=review_sha,
        reviewed_sources=reviewed_sources,
    )

    records: list[tuple[str, bytes]] = []
    gate_methods: list[dict[str, str]] = []
    worksheet_selected = {
        str(entry["source_identity"]): _object(entry, "selected source")
        for entry in _array(worksheet_value["selected_sources"], "selected_sources")
    }
    for identity in sorted(reviewed_sources):
        reviewed = reviewed_sources[identity]
        candidate = reviewed["candidate"]
        assert isinstance(candidate, dict)
        entry = worksheet_selected[identity]
        var_id = str(entry["var_id"])
        observed_hazards = [str(item) for item in candidate["atlas_observed_hazards"]]
        record = {
            "schema": 1,
            "id": var_id,
            "status": "VAR_REVIEWED",
            "source": candidate["source"],
            "classifications": sorted(set(str(item) for item in candidate["atlas_classifications"])),
            "hazards_reviewed": sorted(set(observed_hazards)),
            "semantic_rules": sorted(rules_by_source[identity]),
            "evidence": [review.RESULT_ID, prepare.ID],
            "notes": [
                "Canonicalized from completed R2C world-state source review plus explicit human-authored semantic admission rules."
            ],
        }
        records.append((f"records/{var_id}.json", _pretty_bytes(record)))
        gate_methods.append({"query": identity, "var_id": var_id})

    gate = {
        "schema": 1,
        "id": GATE_ID,
        "frontier": FRONTIER,
        "minimum_status": "VAR_REVIEWED",
        "require_semantic_rules": True,
        "require_hazards_reviewed": True,
        "methods": gate_methods,
    }
    semantics = _render_semantics(rules, worksheet_sha)
    staged: list[tuple[str, bytes]] = records + [
        (f"semantics/{SEMANTICS_FILE}", semantics),
        ("gate.json", _pretty_bytes(gate)),
    ]

    output = _fresh_external_dir(output_dir)
    manifest_files = [
        {"path": path, "size": len(raw), "sha256": _sha256_bytes(raw)} for path, raw in staged
    ]
    manifest = {
        "schema": SCHEMA,
        "kind": KIND,
        "id": ID,
        "commit_policy": COMMIT_POLICY,
        "review_result_sha256": review_sha,
        "admission_worksheet_sha256": worksheet_sha,
        "source_archive_sha256": review.EXPECTED_SOURCE_SHA256,
        "contains_official_source_text": False,
        "var_records": len(records),
        "semantic_rules": len(rules),
        "gate_id": GATE_ID,
        "independent_gate_required": True,
        "production_admitted": False,
        "files": manifest_files,
        "next_step": (
            "Run tools/vanilla_source_gate.py against gate.json and this staged records directory "
            "using the pinned Vanilla Atlas. Only an admitted=true report may authorize repository "
            "promotion and production reliance."
        ),
    }
    manifest_raw = _pretty_bytes(manifest)

    try:
        output.mkdir(parents=True)
        for relative, raw in staged:
            path = output / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)
        (output / "manifest.json").write_bytes(manifest_raw)
    except OSError as error:
        raise MaterializeError(f"cannot write staged admission bundle: {error}") from error

    return {
        "output_dir": str(output),
        "manifest_sha256": _sha256_bytes(manifest_raw),
        "var_records": len(records),
        "semantic_rules": len(rules),
        "gate_id": GATE_ID,
        "contains_official_source_text": False,
        "production_admitted": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--review-result", type=Path, required=True)
    parser.add_argument("--worksheet", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        summary = materialize(args.review_result, args.worksheet, args.output_dir)
    except MaterializeError as error:
        print(f"R2C world-state admission materialization failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
