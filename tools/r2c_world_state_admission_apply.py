#!/usr/bin/env python3
"""Apply explicit source-free human semantic decisions to an R2C admission worksheet.

The source-review/binding pipeline prepares an empty semantic-admission worksheet whose selected
sources are already fixed by completed human review. This tool removes the last fragile hand-editing
step: a reviewer authors rule statements plus exact selected candidate IDs in a separate source-free
decision record, and this applier resolves those IDs to the already-frozen source identities.

It performs no semantic inference. Unknown, cross-group, stale or uncovered source support fails
closed. The emitted worksheet keeps the canonical schema consumed unchanged by
``r2c_world_state_admission_materialize.py`` and the independent Vanilla Atlas source gate.
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
    from . import r2c_world_state_admission_prepare as prepare
except ImportError:  # Direct ``python3 tools/...`` execution.
    import r2c_world_state_admission_prepare as prepare  # type: ignore[no-redef]

SCHEMA = 1
DECISION_KIND = "r2c-world-state-semantic-admission-decisions"
DECISION_ID = "ADMISSION-DECISIONS-NET-R2C-WORLD-STATE-26_2-001"
DECISION_COMMIT_POLICY = "SOURCE_FREE_HUMAN_SEMANTIC_DECISIONS_NOT_ADMISSION"


class ApplyError(RuntimeError):
    """Fail-closed semantic-decision application error."""


def _pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


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
    parent = path.parent
    if parent and parent.exists() and parent.is_symlink():
        raise ApplyError(f"output parent must not be a symlink: {parent}")
    if parent and not parent.exists():
        parent.mkdir(parents=True)
    return path


def _object(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise ApplyError(f"{label} must be an object")
    return value


def _array(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise ApplyError(f"{label} must be an array")
    return value


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ApplyError(f"{label} must be a non-empty string")
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
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise ApplyError(f"{label} must be lowercase SHA-256")
    return value


def _validate_prepared_worksheet(
    value: Mapping[str, object],
) -> tuple[dict[str, dict[str, object]], list[dict[str, object]], str]:
    required = {
        "schema": SCHEMA,
        "kind": prepare.KIND,
        "id": prepare.ID,
        "commit_policy": prepare.COMMIT_POLICY,
        "source_archive_sha256": prepare.review.EXPECTED_SOURCE_SHA256,
        "contains_official_source_text": False,
        "all_groups_admission_complete": False,
        "production_admitted": False,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise ApplyError(
            f"prepared semantic worksheet identity/completion mismatch: {json.dumps(mismatches, sort_keys=True)}"
        )

    review_sha = _digest(value.get("review_result_sha256"), "prepared worksheet review_result_sha256")
    contract = _object(value.get("semantic_rule_contract"), "semantic_rule_contract")
    if contract.get("automatic_semantic_inference_forbidden") is not True:
        raise ApplyError("prepared worksheet must forbid automatic semantic inference")
    if contract.get("source_support_must_be_selected") is not True:
        raise ApplyError("prepared worksheet must require selected-source-only support")
    if contract.get("id_prefix") != prepare.SEM_PREFIX:
        raise ApplyError("prepared worksheet semantic rule id prefix drifted")

    selected_entries = _array(value.get("selected_sources"), "selected_sources")
    by_identity: dict[str, dict[str, object]] = {}
    by_candidate_id: dict[str, dict[str, object]] = {}
    for index, raw_entry in enumerate(selected_entries):
        entry = _object(raw_entry, f"selected_sources[{index}]")
        if set(entry) != {"var_id", "source_identity", "candidate"}:
            raise ApplyError(f"selected_sources[{index}] fields are not canonical")
        try:
            candidate = prepare._candidate(entry["candidate"], f"selected_sources[{index}].candidate")
            expected_var = prepare._var_id(str(candidate["candidate_id"]))
        except prepare.PrepareError as error:
            raise ApplyError(str(error)) from error
        identity = _string(entry["source_identity"], f"selected_sources[{index}].source_identity")
        var_id = _string(entry["var_id"], f"selected_sources[{index}].var_id")
        if identity != candidate["source_identity"]:
            raise ApplyError(f"selected_sources[{index}] identity/candidate mismatch")
        if var_id != expected_var:
            raise ApplyError(f"selected_sources[{index}] VAR id drift")
        candidate_id = str(candidate["candidate_id"])
        if identity in by_identity:
            raise ApplyError(f"duplicate selected source identity: {identity}")
        if candidate_id in by_candidate_id:
            raise ApplyError(f"duplicate selected candidate id: {candidate_id}")
        normalized = {"var_id": var_id, "source_identity": identity, "candidate": candidate}
        by_identity[identity] = normalized
        by_candidate_id[candidate_id] = normalized

    raw_groups = _array(value.get("groups"), "prepared worksheet groups")
    if len(raw_groups) != len(prepare.review.FOCUS_GROUPS):
        raise ApplyError("prepared worksheet must contain exactly the focused groups")
    groups: list[dict[str, object]] = []
    seen_group_sources: set[str] = set()
    for index, expected_group in enumerate(prepare.review.FOCUS_GROUPS):
        group = _object(raw_groups[index], f"prepared worksheet groups[{index}]")
        if group.get("group_id") != expected_group:
            raise ApplyError("prepared worksheet groups are missing or out of canonical order")
        if group.get("admission_complete") is not False:
            raise ApplyError(f"{expected_group} prepared worksheet has already been completed")
        if _array(group.get("semantic_rules"), f"{expected_group}.semantic_rules"):
            raise ApplyError(f"{expected_group} prepared worksheet already contains semantic rules")
        identities = _strings(
            group.get("selected_source_identities"),
            f"{expected_group}.selected_source_identities",
            nonempty=True,
        )
        unknown = sorted(set(identities) - set(by_identity))
        if unknown:
            raise ApplyError(f"{expected_group} references unknown selected sources: {unknown}")
        # The completed parent/delegate binder deliberately makes source ownership unambiguous.
        # Reject drift here rather than allowing one candidate to silently support multiple groups.
        overlap = sorted(set(identities) & seen_group_sources)
        if overlap:
            raise ApplyError(f"selected source identities are shared across semantic groups: {overlap}")
        seen_group_sources.update(identities)
        groups.append({"group_id": expected_group, "source_identities": identities})

    if seen_group_sources != set(by_identity):
        unused = sorted(set(by_identity) - seen_group_sources)
        raise ApplyError(f"selected sources are not assigned to a semantic group: {unused}")
    return by_candidate_id, groups, review_sha


def _validate_decisions(
    value: Mapping[str, object],
    *,
    worksheet_sha: str,
    review_sha: str,
    by_candidate_id: Mapping[str, dict[str, object]],
    worksheet_groups: Sequence[Mapping[str, object]],
) -> list[list[dict[str, object]]]:
    required = {
        "schema": SCHEMA,
        "kind": DECISION_KIND,
        "id": DECISION_ID,
        "commit_policy": DECISION_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": prepare.review.EXPECTED_SOURCE_SHA256,
        "prepared_worksheet_sha256": worksheet_sha,
        "review_result_sha256": review_sha,
        "automatic_semantic_inference_forbidden": True,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise ApplyError(f"semantic decision provenance mismatch: {json.dumps(mismatches, sort_keys=True)}")

    raw_groups = _array(value.get("groups"), "semantic decision groups")
    if len(raw_groups) != len(worksheet_groups):
        raise ApplyError("semantic decisions must contain exactly the focused groups")

    seen_rule_ids: set[str] = set()
    result: list[list[dict[str, object]]] = []
    for index, worksheet_group in enumerate(worksheet_groups):
        group_id = str(worksheet_group["group_id"])
        decision_group = _object(raw_groups[index], f"semantic decision groups[{index}]")
        if decision_group.get("group_id") != group_id:
            raise ApplyError("semantic decision groups are missing or out of canonical order")
        if decision_group.get("admission_complete") is not True:
            raise ApplyError(f"{group_id}.admission_complete must be true")

        group_identities = set(_strings(worksheet_group["source_identities"], f"{group_id} sources"))
        raw_rules = _array(decision_group.get("semantic_rules"), f"{group_id}.semantic_rules")
        if not raw_rules:
            raise ApplyError(f"{group_id} must contain at least one semantic decision rule")

        covered: set[str] = set()
        rendered_rules: list[dict[str, object]] = []
        for rule_index, raw_rule in enumerate(raw_rules):
            rule = _object(raw_rule, f"{group_id}.semantic_rules[{rule_index}]")
            if set(rule) != {"id", "statement", "source_candidate_ids"}:
                raise ApplyError(
                    f"{group_id}.semantic_rules[{rule_index}] must contain exactly id, statement and source_candidate_ids"
                )
            rule_id = _string(rule["id"], f"{group_id}.semantic_rules[{rule_index}].id")
            if not rule_id.startswith(prepare.SEM_PREFIX):
                raise ApplyError(f"semantic rule id must use {prepare.SEM_PREFIX}: {rule_id}")
            if rule_id in seen_rule_ids:
                raise ApplyError(f"duplicate semantic rule id: {rule_id}")
            seen_rule_ids.add(rule_id)

            statement = _string(
                rule["statement"], f"{group_id}.semantic_rules[{rule_index}].statement"
            )
            if statement != statement.strip() or "\r" in statement:
                raise ApplyError(f"semantic rule statement must be trimmed canonical text: {rule_id}")

            candidate_ids = _strings(
                rule["source_candidate_ids"],
                f"{group_id}.semantic_rules[{rule_index}].source_candidate_ids",
                nonempty=True,
            )
            identities: list[str] = []
            for candidate_id in candidate_ids:
                entry = by_candidate_id.get(candidate_id)
                if entry is None:
                    raise ApplyError(f"{rule_id} cites unknown selected candidate id: {candidate_id}")
                identity = str(entry["source_identity"])
                if identity not in group_identities:
                    raise ApplyError(
                        f"{rule_id} cites candidate outside {group_id}: {candidate_id} ({identity})"
                    )
                identities.append(identity)
                covered.add(identity)
            rendered_rules.append(
                {"id": rule_id, "statement": statement, "source_identities": identities}
            )

        missing = sorted(group_identities - covered)
        if missing:
            raise ApplyError(
                f"every selected {group_id} source must support at least one semantic rule; uncovered: {missing}"
            )
        result.append(rendered_rules)
    return result


def apply(*, worksheet: Path, decisions: Path, output: Path) -> dict[str, object]:
    """Bind explicit human semantic decisions to one pristine prepared worksheet."""
    worksheet_value, worksheet_sha = _read_json(worksheet, "R2C semantic-admission worksheet")
    decisions_value, decisions_sha = _read_json(decisions, "R2C semantic-admission decisions")
    by_candidate_id, groups, review_sha = _validate_prepared_worksheet(worksheet_value)
    rendered = _validate_decisions(
        decisions_value,
        worksheet_sha=worksheet_sha,
        review_sha=review_sha,
        by_candidate_id=by_candidate_id,
        worksheet_groups=groups,
    )

    completed = copy.deepcopy(worksheet_value)
    completed_groups = _array(completed.get("groups"), "completed worksheet groups")
    for group, rules in zip(completed_groups, rendered, strict=True):
        target = _object(group, "completed worksheet group")
        target["semantic_rules"] = rules
        target["admission_complete"] = True
    completed["all_groups_admission_complete"] = True

    raw = _pretty_bytes(completed)
    output = _fresh_output(output)
    output.write_bytes(raw)
    return {
        "output": str(output),
        "sha256": _sha256_bytes(raw),
        "prepared_worksheet_sha256": worksheet_sha,
        "review_result_sha256": review_sha,
        "decisions_sha256": decisions_sha,
        "groups": len(rendered),
        "semantic_rules": sum(len(rules) for rules in rendered),
        "selected_sources": len(by_candidate_id),
        "contains_official_source_text": False,
        "production_admitted": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worksheet", type=Path, required=True)
    parser.add_argument("--decisions", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        summary = apply(worksheet=args.worksheet, decisions=args.decisions, output=args.output)
    except (ApplyError, OSError) as error:
        print(f"R2C semantic-admission decision application failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
