#!/usr/bin/env python3
"""Independently harden a representative section set before benchmark handoff."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from collections import Counter
from pathlib import Path
from typing import Any

import official_representative_section_world
import section_corpus_set
import section_representative_plan

SCHEMA = 1
KIND = "section-representative-set-admission"
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
CELL_FACT_KEYS = frozenset(
    {"non_air", "counted_fluid", "random_block", "random_fluid"}
)
SECTION_CLASS_KEYS = frozenset(
    {"all_air", "contains_fluid", "random_block_present", "random_fluid_present"}
)


class AdmissionError(ValueError):
    """Raised when a structurally valid set is not safe for benchmark handoff."""


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise AdmissionError(f"{label} must be an object")
    return value


def _integer(mapping: dict[str, Any], key: str, label: str) -> int:
    value = mapping.get(key)
    if isinstance(value, bool) or not isinstance(value, int):
        raise AdmissionError(f"{label}.{key} must be an integer")
    return value


def _string(mapping: dict[str, Any], key: str, label: str) -> str:
    value = mapping.get(key)
    if not isinstance(value, str) or not value:
        raise AdmissionError(f"{label}.{key} must be a non-empty string")
    return value


def _sha256_value(value: object, label: str) -> str:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise AdmissionError(f"{label} must be canonical lowercase SHA-256")
    return value


def _boolean(mapping: dict[str, Any], key: str, label: str) -> bool:
    value = mapping.get(key)
    if not isinstance(value, bool):
        raise AdmissionError(f"{label}.{key} must be a boolean")
    return value


def _equal(actual: object, expected: object, label: str) -> None:
    if actual != expected:
        raise AdmissionError(f"{label} mismatch: expected {expected!r}, got {actual!r}")


def _canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    return _object(value, str(path))


def _exact_counter(
    value: object,
    expected_keys: frozenset[str],
    upper_bound: int,
    label: str,
) -> dict[str, int]:
    raw = _object(value, label)
    _equal(set(raw), set(expected_keys), f"{label} key set")
    result: dict[str, int] = {}
    for key in sorted(expected_keys):
        count = _integer(raw, key, label)
        if count < 0 or count > upper_bound:
            raise AdmissionError(
                f"{label}.{key} must be in 0..{upper_bound}; got {count}"
            )
        result[key] = count
    return result


def _semantic_consistency(
    cell_facts: dict[str, int],
    section_classes: dict[str, int],
    label: str,
) -> None:
    if cell_facts["counted_fluid"] > cell_facts["non_air"]:
        raise AdmissionError(f"{label}: counted-fluid cells exceed non-air cells")
    if cell_facts["random_block"] > cell_facts["non_air"]:
        raise AdmissionError(f"{label}: random-block cells exceed non-air cells")
    if cell_facts["random_fluid"] > cell_facts["counted_fluid"]:
        raise AdmissionError(f"{label}: random-fluid cells exceed counted-fluid cells")
    if section_classes["random_fluid_present"] > section_classes["contains_fluid"]:
        raise AdmissionError(
            f"{label}: random-fluid sections exceed fluid-containing sections"
        )


def _validate_server_properties(world: dict[str, Any], seed: int) -> str:
    expected_text = official_representative_section_world.server_properties(seed)
    expected_lines = expected_text.splitlines()
    actual = world.get("server_properties")
    if not isinstance(actual, list) or not all(isinstance(line, str) for line in actual):
        raise AdmissionError("world evidence server_properties must be a string list")
    _equal(actual, expected_lines, "world server property contract")
    return hashlib.sha256(expected_text.encode("utf-8")).hexdigest()


def _validate_member_semantics(
    manifest: dict[str, Any],
    expected_dimensions: set[str],
    label: str,
) -> tuple[dict[str, int], dict[str, int], dict[str, dict[str, object]]]:
    section_count = _integer(manifest, "section_count", label)
    total_cells = _integer(manifest, "total_cells", label)
    if section_count <= 0 or total_cells != section_count * 4096:
        raise AdmissionError(f"{label}: invalid section/cell totals")

    global_cell_facts = _exact_counter(
        manifest.get("cell_facts"), CELL_FACT_KEYS, total_cells, f"{label} cell_facts"
    )
    global_classes = _exact_counter(
        manifest.get("section_classes"),
        SECTION_CLASS_KEYS,
        section_count,
        f"{label} section_classes",
    )
    _semantic_consistency(global_cell_facts, global_classes, label)

    raw_dimensions = _object(manifest.get("per_dimension"), f"{label} per_dimension")
    _equal(set(raw_dimensions), expected_dimensions, f"{label} dimension key set")
    merged_cells: Counter[str] = Counter()
    merged_classes: Counter[str] = Counter()
    validated: dict[str, dict[str, object]] = {}

    for dimension in sorted(expected_dimensions):
        summary = _object(raw_dimensions.get(dimension), f"{label} {dimension}")
        dimension_sections = _integer(summary, "section_count", f"{label} {dimension}")
        dimension_cells = _integer(summary, "total_cells", f"{label} {dimension}")
        if dimension_sections <= 0 or dimension_cells != dimension_sections * 4096:
            raise AdmissionError(f"{label} {dimension}: invalid section/cell totals")
        cell_facts = _exact_counter(
            summary.get("cell_facts"),
            CELL_FACT_KEYS,
            dimension_cells,
            f"{label} {dimension} cell_facts",
        )
        classes = _exact_counter(
            summary.get("section_classes"),
            SECTION_CLASS_KEYS,
            dimension_sections,
            f"{label} {dimension} section_classes",
        )
        _semantic_consistency(cell_facts, classes, f"{label} {dimension}")
        merged_cells.update(cell_facts)
        merged_classes.update(classes)
        validated[dimension] = {
            "section_count": dimension_sections,
            "total_cells": dimension_cells,
            "cell_facts": cell_facts,
            "section_classes": classes,
        }

    _equal(
        sum(int(item["section_count"]) for item in validated.values()),
        section_count,
        f"{label} dimension section total",
    )
    _equal(
        dict(merged_cells),
        global_cell_facts,
        f"{label} per-dimension/global cell facts",
    )
    _equal(
        dict(merged_classes),
        global_classes,
        f"{label} per-dimension/global section classes",
    )
    return global_cell_facts, global_classes, validated


def _validate_set_record(
    record: dict[str, Any],
    plan: dict[str, object],
) -> tuple[list[dict[str, Any]], set[str]]:
    _equal(_integer(record, "schema", "corpus set"), 1, "corpus set schema")
    _equal(
        _string(record, "kind", "corpus set"), section_corpus_set.KIND, "corpus set kind"
    )
    _equal(record.get("policy"), plan["policy"], "corpus set policy")
    _equal(record.get("plan_sha256"), plan["plan_sha256"], "corpus set plan digest")
    _equal(
        _boolean(record, "decision_eligible", "corpus set"),
        True,
        "corpus set structural decision eligibility",
    )
    _equal(
        record.get("decision_scope"),
        "dimension-separated-only",
        "corpus set decision scope",
    )
    _equal(
        _boolean(record, "cross_dimension_score_allowed", "corpus set"),
        False,
        "cross-dimension score guard",
    )
    _equal(
        _integer(record, "member_count", "corpus set"),
        section_representative_plan.SEED_COUNT,
        "corpus set member count",
    )

    population_identity = _object(record.get("population_identity"), "population identity")
    _equal(
        _canonical_digest(population_identity),
        _sha256_value(record.get("population_sha256"), "corpus set population_sha256"),
        "population identity digest",
    )
    evidence_without_digest = dict(record)
    expected_evidence = _sha256_value(
        evidence_without_digest.pop("evidence_sha256", None),
        "corpus set evidence_sha256",
    )
    _equal(
        _canonical_digest(evidence_without_digest),
        expected_evidence,
        "corpus set evidence digest",
    )

    members = record.get("members")
    if not isinstance(members, list) or len(members) != section_representative_plan.SEED_COUNT:
        raise AdmissionError("corpus set must contain exactly four member records")
    dimensions = _object(record.get("per_dimension"), "corpus set per_dimension")
    expected_dimensions = {
        descriptor.key
        for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS
    }
    _equal(set(dimensions), expected_dimensions, "corpus set dimension keys")
    return [
        _object(member, f"corpus set member[{index}]")
        for index, member in enumerate(members)
    ], expected_dimensions


def build_admission(
    *,
    plan: dict[str, object],
    set_record: dict[str, Any],
    members_root: Path,
    set_file_sha256: str,
) -> dict[str, object]:
    section_representative_plan.validate_plan(plan)
    set_file_sha256 = _sha256_value(set_file_sha256, "corpus-set file SHA-256")
    set_members, expected_dimensions = _validate_set_record(set_record, plan)
    seeds = plan["seeds"]
    assert isinstance(seeds, list)

    per_dimension_cells = {dimension: Counter() for dimension in expected_dimensions}
    per_dimension_classes = {dimension: Counter() for dimension in expected_dimensions}
    admitted_members: list[dict[str, object]] = []

    for seed_index, expected_seed_raw in enumerate(seeds):
        expected_seed = int(expected_seed_raw)
        set_member = set_members[seed_index]
        _equal(
            _integer(set_member, "seed_index", f"set member {seed_index}"),
            seed_index,
            "set member seed index",
        )
        _equal(
            _integer(set_member, "seed", f"set member {seed_index}"),
            expected_seed,
            "set member seed",
        )

        directory = members_root / f"seed-{seed_index}"
        world = _load_json(directory / "world-evidence.json")
        manifest = _load_json(directory / "corpus-manifest.json")
        _equal(
            _integer(world, "seed_index", "world evidence"),
            seed_index,
            "world seed index",
        )
        _equal(
            _integer(world, "seed", "world evidence"), expected_seed, "world seed"
        )
        property_sha = _validate_server_properties(world, expected_seed)

        corpus_sha = _sha256_value(
            manifest.get("corpus_sha256"), "member manifest corpus_sha256"
        )
        _equal(corpus_sha, set_member.get("corpus_sha256"), "set/member corpus identity")
        global_cells, global_classes, dimensions = _validate_member_semantics(
            manifest, expected_dimensions, f"seed-{seed_index} manifest"
        )
        for dimension, summary in dimensions.items():
            per_dimension_cells[dimension].update(summary["cell_facts"])
            per_dimension_classes[dimension].update(summary["section_classes"])

        admitted_members.append(
            {
                "seed_index": seed_index,
                "seed": expected_seed,
                "corpus_sha256": corpus_sha,
                "server_properties_sha256": property_sha,
                "cell_facts": global_cells,
                "section_classes": global_classes,
            }
        )

    per_dimension: dict[str, object] = {}
    raw_set_dimensions = _object(set_record.get("per_dimension"), "corpus set per_dimension")
    for dimension in sorted(expected_dimensions):
        raw_summary = _object(raw_set_dimensions.get(dimension), f"corpus set {dimension}")
        section_count = _integer(raw_summary, "section_count", f"corpus set {dimension}")
        total_cells = _integer(raw_summary, "total_cells", f"corpus set {dimension}")
        if total_cells != section_count * 4096:
            raise AdmissionError(f"corpus set {dimension}: invalid section/cell totals")
        per_dimension[dimension] = {
            "section_count": section_count,
            "total_cells": total_cells,
            "cell_facts": dict(sorted(per_dimension_cells[dimension].items())),
            "section_classes": dict(sorted(per_dimension_classes[dimension].items())),
        }

    result: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "policy": plan["policy"],
        "plan_sha256": plan["plan_sha256"],
        "population_sha256": set_record["population_sha256"],
        "set_evidence_sha256": set_record["evidence_sha256"],
        "set_file_sha256": set_file_sha256,
        "member_count": len(admitted_members),
        "decision_eligible": True,
        "benchmark_handoff_eligible": True,
        "decision_scope": "dimension-separated-only",
        "cross_dimension_score_allowed": False,
        "members": admitted_members,
        "per_dimension": per_dimension,
    }
    result["admission_sha256"] = _canonical_digest(result)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--members-root", type=Path, required=True)
    parser.add_argument("--corpus-set", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        plan = section_representative_plan.load_plan(args.plan)
        set_bytes = args.corpus_set.read_bytes()
        set_record = _object(json.loads(set_bytes), "corpus set")
        result = build_admission(
            plan=plan,
            set_record=set_record,
            members_root=args.members_root,
            set_file_sha256=hashlib.sha256(set_bytes).hexdigest(),
        )
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (
        OSError,
        json.JSONDecodeError,
        AdmissionError,
        section_representative_plan.PlanError,
    ) as error:
        print(f"representative population admission error: {error}")
        return 1

    print(
        "representative population admission: "
        f"members={result['member_count']} population={result['population_sha256']} "
        f"admission={result['admission_sha256']} PASS"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
