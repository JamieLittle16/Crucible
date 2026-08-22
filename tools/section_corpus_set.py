#!/usr/bin/env python3
"""Validate the complete representative corpus population before decision use."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import tomllib
from collections import Counter
from pathlib import Path
from typing import Any

import section_representative_plan

SCHEMA = 1
KIND = "section-corpus-set"
POPULATION_KIND = "section-corpus-population-identity"
MEMBER_EXTRACTOR = "vanilla-save-region-v2-representative-member"
MEMBER_PURPOSE = "representative-member"
FULL_CHUNK_STATUS = "minecraft:full"
WORLD_GENERATOR = "official-server-representative-section-world-v2-batched"
WORLD_SCHEMA = 2
SELECTION_COMMAND_SHA256 = "cb97b7490c28e38293251561749a87dbda2d0f78d78c7cf98471e5eff825a354"
EXPECTED_CANDIDATES = {
    "direct-reference": False,
    "direct": True,
    "adaptive": True,
    "fast-local": True,
    "packed-local": True,
}
DEFAULT_LOCK = Path("vanilla/vanilla.lock.toml")
DEFAULT_STATE_MANIFEST = Path("vanilla/state-data/26.2-state-data-manifest.json")
SHA256 = re.compile(r"[0-9a-f]{64}\Z")


class CorpusSetError(ValueError):
    """Raised when representative corpus members do not form the frozen population."""


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise CorpusSetError(f"{label} must be an object")
    return value


def _integer(mapping: dict[str, Any], key: str, label: str) -> int:
    value = mapping.get(key)
    if isinstance(value, bool) or not isinstance(value, int):
        raise CorpusSetError(f"{label}.{key} must be an integer")
    return value


def _string(mapping: dict[str, Any], key: str, label: str) -> str:
    value = mapping.get(key)
    if not isinstance(value, str) or not value:
        raise CorpusSetError(f"{label}.{key} must be a non-empty string")
    return value


def _sha256(mapping: dict[str, Any], key: str, label: str) -> str:
    value = _string(mapping, key, label)
    if SHA256.fullmatch(value) is None:
        raise CorpusSetError(f"{label}.{key} must be canonical lowercase SHA-256")
    return value


def _require_sha256(value: object, label: str) -> str:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise CorpusSetError(f"{label} must be canonical lowercase SHA-256")
    return value


def _boolean(mapping: dict[str, Any], key: str, label: str) -> bool:
    value = mapping.get(key)
    if not isinstance(value, bool):
        raise CorpusSetError(f"{label}.{key} must be a boolean")
    return value


def _equal(actual: object, expected: object, label: str) -> None:
    if actual != expected:
        raise CorpusSetError(f"{label} mismatch: expected {expected!r}, got {actual!r}")


def _counter(mapping: object, label: str) -> Counter[str]:
    raw = _object(mapping, label)
    result: Counter[str] = Counter()
    for key, value in raw.items():
        if not isinstance(key, str) or not key:
            raise CorpusSetError(f"{label} contains an invalid key")
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise CorpusSetError(f"{label}.{key} must be a non-negative integer")
        result[key] = value
    return result


def _nonnegative_mapping(value: object, label: str) -> dict[str, int]:
    return dict(sorted(_counter(value, label).items()))


def _target_from_state_manifest(state_manifest: dict[str, Any]) -> dict[str, object]:
    target = _object(state_manifest.get("target"), "state manifest target")
    return {
        "minecraft_version": _string(target, "minecraft_version", "state manifest target"),
        "protocol_version": _integer(target, "protocol_version", "state manifest target"),
        "data_version": _integer(target, "data_version", "state manifest target"),
        "state_count": _integer(state_manifest, "state_count", "state manifest"),
        "state_data_generation_sha256": _sha256(
            state_manifest, "generation_digest", "state manifest"
        ),
        "state_data_input_sha256": _sha256(
            state_manifest, "input_digest", "state manifest"
        ),
    }


def _canonical_digest(payload: object) -> str:
    encoded = json.dumps(
        payload, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    return _object(value, str(path))


def _expected_selection(plan: dict[str, object]) -> dict[str, list[list[int]]]:
    dimensions = _object(plan["dimensions"], "plan dimensions")
    return {
        descriptor.key: sorted(
            [
                [int(chunk[0]), int(chunk[1])]
                for chunk in dimensions[descriptor.key]["chunks"]
            ]
        )
        for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS
    }


def _validate_candidate_rows(
    rows_value: object,
    section_count: int,
    label: str,
) -> dict[str, dict[str, object]]:
    if not isinstance(rows_value, list) or len(rows_value) != len(EXPECTED_CANDIDATES):
        raise CorpusSetError(f"{label} has the wrong candidate row count")
    observed: dict[str, dict[str, object]] = {}
    for index, raw_row in enumerate(rows_value):
        row_label = f"{label} candidate[{index}]"
        row = _object(raw_row, row_label)
        name = _string(row, "candidate", row_label)
        if name in observed or name not in EXPECTED_CANDIDATES:
            raise CorpusSetError(f"unexpected/duplicate candidate {name!r} in {label}")
        _equal(
            _boolean(row, "production_candidate", row_label),
            EXPECTED_CANDIDATES[name],
            f"{label} {name} production flag",
        )
        _equal(
            _integer(row, "sections", row_label),
            section_count,
            f"{label} {name} section count",
        )
        total_owned = _integer(row, "total_owned_bytes", row_label)
        max_owned = _integer(row, "max_owned_bytes", row_label)
        transitions = _integer(row, "construction_transitions", row_label)
        allocations = _integer(row, "logical_backing_allocations", row_label)
        if min(total_owned, max_owned, transitions, allocations) < 0:
            raise CorpusSetError(f"{label} {name} metrics must be non-negative")
        representations = _counter(row.get("representations"), f"{label} {name} representations")
        if sum(representations.values()) != section_count:
            raise CorpusSetError(f"{label} {name} representation counts do not sum to sections")
        observed[name] = {
            "production_candidate": EXPECTED_CANDIDATES[name],
            "sections": section_count,
            "total_owned_bytes": total_owned,
            "max_owned_bytes": max_owned,
            "construction_transitions": transitions,
            "logical_backing_allocations": allocations,
            "representations": dict(sorted(representations.items())),
        }
    _equal(set(observed), set(EXPECTED_CANDIDATES), f"{label} candidate set")
    return observed


def _empty_candidate_totals() -> dict[str, dict[str, object]]:
    return {
        name: {
            "production_candidate": production,
            "sections": 0,
            "total_owned_bytes": 0,
            "max_owned_bytes": 0,
            "construction_transitions": 0,
            "logical_backing_allocations": 0,
            "representations": Counter(),
        }
        for name, production in EXPECTED_CANDIDATES.items()
    }


def _merge_candidate_metrics(
    totals: dict[str, dict[str, object]],
    metrics: dict[str, dict[str, object]],
) -> None:
    _equal(set(metrics), set(EXPECTED_CANDIDATES), "candidate merge set")
    for name, row in metrics.items():
        target = totals[name]
        target["sections"] = int(target["sections"]) + int(row["sections"])
        target["total_owned_bytes"] = int(target["total_owned_bytes"]) + int(
            row["total_owned_bytes"]
        )
        target["max_owned_bytes"] = max(
            int(target["max_owned_bytes"]), int(row["max_owned_bytes"])
        )
        target["construction_transitions"] = int(
            target["construction_transitions"]
        ) + int(row["construction_transitions"])
        target["logical_backing_allocations"] = int(
            target["logical_backing_allocations"]
        ) + int(row["logical_backing_allocations"])
        representations = target["representations"]
        assert isinstance(representations, Counter)
        representations.update(row["representations"])


def _normalize_candidate_totals(
    totals: dict[str, dict[str, object]],
) -> dict[str, dict[str, object]]:
    normalized: dict[str, dict[str, object]] = {}
    for name, raw in totals.items():
        row = dict(raw)
        representations = row["representations"]
        assert isinstance(representations, Counter)
        row["representations"] = dict(sorted(representations.items()))
        normalized[name] = row
    return normalized


def _validate_world_generation(
    world: dict[str, Any],
    *,
    plan: dict[str, object],
) -> dict[str, object]:
    _equal(_integer(world, "schema", "world evidence"), WORLD_SCHEMA, "world schema")
    _equal(_string(world, "generator", "world evidence"), WORLD_GENERATOR, "world generator")
    expected_tickets = (
        len(section_representative_plan.REPRESENTATIVE_DIMENSIONS)
        * section_representative_plan.CHUNKS_PER_DIMENSION
    )
    _equal(
        _integer(world, "selection_command_count", "world evidence"),
        expected_tickets,
        "world selection command count",
    )
    _equal(
        _sha256(world, "selection_command_sha256", "world evidence"),
        SELECTION_COMMAND_SHA256,
        "world selection command digest",
    )
    batch_size = _integer(world, "batch_size", "world evidence")
    batch_count = _integer(world, "batch_count", "world evidence")
    settle_seconds = _integer(world, "batch_settle_seconds", "world evidence")
    if batch_size <= 0 or batch_count <= 0 or settle_seconds < 0:
        raise CorpusSetError("world bounded-generation parameters are invalid")
    expected_batches_per_dimension = (
        section_representative_plan.CHUNKS_PER_DIMENSION + batch_size - 1
    ) // batch_size
    expected_batch_count = expected_batches_per_dimension * len(
        section_representative_plan.REPRESENTATIVE_DIMENSIONS
    )
    _equal(batch_count, expected_batch_count, "world batch count")
    raw_timings = world.get("batch_timings")
    if not isinstance(raw_timings, list) or len(raw_timings) != batch_count:
        raise CorpusSetError("world batch timing evidence is incomplete")
    tickets_by_dimension: Counter[str] = Counter()
    for index, raw_timing in enumerate(raw_timings):
        timing = _object(raw_timing, f"world batch timing[{index}]")
        _equal(
            _integer(timing, "index", f"world batch timing[{index}]"),
            index,
            "batch index",
        )
        dimension = _string(timing, "dimension", f"world batch timing[{index}]")
        if dimension not in section_representative_plan.DIMENSION_BY_KEY:
            raise CorpusSetError(f"world batch timing has unknown dimension {dimension!r}")
        ticket_count = _integer(timing, "ticket_count", f"world batch timing[{index}]")
        elapsed_ms = _integer(timing, "elapsed_ms", f"world batch timing[{index}]")
        if ticket_count <= 0 or ticket_count > batch_size or elapsed_ms < 0:
            raise CorpusSetError(f"world batch timing[{index}] has invalid metrics")
        tickets_by_dimension[dimension] += ticket_count
    expected_dimension_tickets = Counter(
        {
            descriptor.key: section_representative_plan.CHUNKS_PER_DIMENSION
            for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS
        }
    )
    _equal(tickets_by_dimension, expected_dimension_tickets, "world batch dimension coverage")
    return {
        "generator": WORLD_GENERATOR,
        "selection_command_sha256": SELECTION_COMMAND_SHA256,
        "batch_size": batch_size,
        "batch_count": batch_count,
        "batch_settle_seconds": settle_seconds,
    }


def _validate_python_dimensions(
    manifest: dict[str, Any],
    expected_dimensions: dict[str, int],
    global_cardinality: Counter[str],
) -> dict[str, dict[str, object]]:
    raw_dimensions = _object(manifest.get("per_dimension"), "Python per_dimension")
    _equal(set(raw_dimensions), set(expected_dimensions), "Python per-dimension key set")
    merged_cardinality: Counter[str] = Counter()
    validated: dict[str, dict[str, object]] = {}
    for dimension, expected_sections in expected_dimensions.items():
        label = f"Python per_dimension {dimension}"
        summary = _object(raw_dimensions.get(dimension), label)
        _equal(_integer(summary, "section_count", label), expected_sections, f"{dimension} section count")
        _equal(
            _integer(summary, "total_cells", label),
            expected_sections * 4096,
            f"{dimension} total cells",
        )
        distinct = _integer(summary, "distinct_state_ids", label)
        if distinct <= 0:
            raise CorpusSetError(f"{dimension} distinct_state_ids must be positive")
        histogram = _counter(summary.get("cardinality_histogram"), f"{label} histogram")
        if sum(histogram.values()) != expected_sections:
            raise CorpusSetError(f"{dimension} histogram does not sum to section count")
        merged_cardinality.update(histogram)
        validated[dimension] = {
            "section_count": expected_sections,
            "total_cells": expected_sections * 4096,
            "distinct_state_ids": distinct,
            "cardinality_histogram": dict(sorted(histogram.items(), key=lambda item: int(item[0]))),
            "cell_facts": _nonnegative_mapping(summary.get("cell_facts"), f"{label} cell_facts"),
            "section_classes": _nonnegative_mapping(
                summary.get("section_classes"), f"{label} section_classes"
            ),
        }
    _equal(merged_cardinality, global_cardinality, "per-dimension/global Python histogram")
    return validated


def _validate_rust_dimensions(
    rust: dict[str, Any],
    python_dimensions: dict[str, dict[str, object]],
    global_cardinality: Counter[str],
    global_candidates: dict[str, dict[str, object]],
) -> dict[str, dict[str, object]]:
    raw_dimensions = _object(rust.get("per_dimension"), "Rust per_dimension")
    _equal(set(raw_dimensions), set(python_dimensions), "Rust/Python per-dimension key set")
    merged_cardinality: Counter[str] = Counter()
    merged_candidates = _empty_candidate_totals()
    validated: dict[str, dict[str, object]] = {}
    for dimension, python_summary in python_dimensions.items():
        expected_sections = int(python_summary["section_count"])
        label = f"Rust per_dimension {dimension}"
        summary = _object(raw_dimensions.get(dimension), label)
        _equal(_integer(summary, "section_count", label), expected_sections, f"{dimension} section count")
        _equal(
            _integer(summary, "total_cells", label),
            int(python_summary["total_cells"]),
            f"{dimension} total cells",
        )
        distinct = _integer(summary, "distinct_state_ids", label)
        _equal(distinct, int(python_summary["distinct_state_ids"]), f"{dimension} distinct states")
        histogram = _counter(summary.get("cardinality_histogram"), f"{label} histogram")
        _equal(
            histogram,
            Counter(python_summary["cardinality_histogram"]),
            f"{dimension} Python/Rust histogram",
        )
        candidates = _validate_candidate_rows(summary.get("candidates"), expected_sections, label)
        merged_cardinality.update(histogram)
        _merge_candidate_metrics(merged_candidates, candidates)
        validated[dimension] = {
            **python_summary,
            "candidates": candidates,
        }
    _equal(merged_cardinality, global_cardinality, "per-dimension/global Rust histogram")
    _equal(
        _normalize_candidate_totals(merged_candidates),
        global_candidates,
        "per-dimension/global Rust candidate diagnostics",
    )
    return validated


def validate_member(
    *,
    seed_index: int,
    plan: dict[str, object],
    state_target: dict[str, object],
    pinned_server_sha256: str,
    world: dict[str, Any],
    extraction: dict[str, Any],
    manifest: dict[str, Any],
    rust: dict[str, Any],
) -> dict[str, object]:
    seeds = plan["seeds"]
    assert isinstance(seeds, list)
    expected_seed = int(seeds[seed_index])
    plan_sha = _require_sha256(plan["plan_sha256"], "representative plan digest")
    generation = _validate_world_generation(world, plan=plan)
    _equal(world.get("minecraft_version"), state_target["minecraft_version"], "world target")
    _equal(_sha256(world, "server_sha256", "world evidence"), pinned_server_sha256, "server SHA-256")
    _equal(world.get("representative_policy"), plan["policy"], "world representative policy")
    _equal(world.get("plan_sha256"), plan_sha, "world plan digest")
    _equal(_integer(world, "seed_index", "world evidence"), seed_index, "world seed index")
    _equal(_integer(world, "seed", "world evidence"), expected_seed, "world seed")

    _equal(_integer(extraction, "schema", "extraction evidence"), 1, "extraction schema")
    _equal(_string(extraction, "policy", "extraction evidence"), MEMBER_EXTRACTOR, "extractor")
    _equal(extraction.get("representative_policy"), plan["policy"], "extraction policy")
    _equal(extraction.get("plan_sha256"), plan_sha, "extraction plan digest")
    _equal(_integer(extraction, "seed_index", "extraction evidence"), seed_index, "extraction seed index")
    _equal(_integer(extraction, "seed", "extraction evidence"), expected_seed, "extraction seed")
    _equal(extraction.get("selected_chunks"), _expected_selection(plan), "selected chunk schedule")
    expected_chunk_count = (
        len(section_representative_plan.REPRESENTATIVE_DIMENSIONS)
        * section_representative_plan.CHUNKS_PER_DIMENSION
    )
    status_histogram = _counter(extraction.get("chunk_status_histogram"), "chunk status histogram")
    _equal(
        status_histogram,
        Counter({FULL_CHUNK_STATUS: expected_chunk_count}),
        "representative full-chunk status census",
    )

    lattice_raw = _object(extraction.get("section_lattice"), "section lattice")
    lattice: dict[str, list[int]] = {}
    expected_dimensions: dict[str, int] = {}
    for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS:
        dimension = descriptor.key
        raw_values = lattice_raw.get(dimension)
        if not isinstance(raw_values, list) or not raw_values:
            raise CorpusSetError(f"section lattice for {dimension} must be a non-empty list")
        values = [_integer({"value": value}, "value", f"{dimension} lattice") for value in raw_values]
        if values != list(range(values[0], values[-1] + 1)):
            raise CorpusSetError(f"section lattice for {dimension} is not contiguous")
        lattice[dimension] = values
        expected_dimensions[dimension] = section_representative_plan.CHUNKS_PER_DIMENSION * len(values)
    expected_sections = sum(expected_dimensions.values())
    _equal(
        _integer(extraction, "section_count", "extraction evidence"),
        expected_sections,
        "extraction section count",
    )

    manifest_target = _object(manifest.get("target"), "member manifest target")
    for key, expected in state_target.items():
        _equal(manifest_target.get(key), expected, f"member manifest target {key}")
    source = _object(manifest.get("source"), "member manifest source")
    _equal(source.get("kind"), "vanilla-save", "member source kind")
    _equal(source.get("extractor"), MEMBER_EXTRACTOR, "member source extractor")
    inventory_sha = _sha256(extraction, "inventory_sha256", "extraction evidence")
    _equal(source.get("inventory_sha256"), inventory_sha, "member source inventory")
    corpus_sha = _sha256(extraction, "corpus_sha256", "extraction evidence")
    _equal(manifest.get("corpus_sha256"), corpus_sha, "member corpus SHA-256")
    section_count = _integer(manifest, "section_count", "member manifest")
    _equal(section_count, expected_sections, "member manifest section count")
    _equal(manifest.get("dimensions"), expected_dimensions, "member dimension section counts")
    _equal(_integer(manifest, "total_cells", "member manifest"), section_count * 4096, "member total cells")
    cardinality = _counter(manifest.get("cardinality_histogram"), "member cardinality histogram")
    if sum(cardinality.values()) != section_count:
        raise CorpusSetError("member cardinality histogram does not sum to section count")
    python_dimensions = _validate_python_dimensions(manifest, expected_dimensions, cardinality)

    _equal(_integer(rust, "schema", "Rust member evidence"), 1, "Rust schema")
    _equal(rust.get("kind"), "section-corpus-import-check", "Rust evidence kind")
    for key, expected in state_target.items():
        _equal(rust.get(key), expected, f"Rust target {key}")
    _equal(rust.get("source_inventory_sha256"), inventory_sha, "Rust source inventory")
    _equal(rust.get("extractor"), MEMBER_EXTRACTOR, "Rust extractor")
    _equal(rust.get("purpose"), MEMBER_PURPOSE, "Rust member purpose")
    _equal(_boolean(rust, "decision_requested", "Rust member evidence"), False, "member decision request")
    _equal(_boolean(rust, "decision_eligible", "Rust member evidence"), False, "member decision eligibility")
    _equal(_integer(rust, "section_count", "Rust member evidence"), section_count, "Rust section count")
    _equal(_integer(rust, "total_cells", "Rust member evidence"), section_count * 4096, "Rust total cells")
    _equal(rust.get("dimensions"), expected_dimensions, "Rust dimensions")
    rust_cardinality = _counter(rust.get("cardinality_histogram"), "Rust cardinality histogram")
    _equal(rust_cardinality, cardinality, "Python/Rust cardinality histogram")
    _equal(
        _integer(rust, "distinct_state_ids", "Rust member evidence"),
        _integer(manifest, "distinct_state_ids", "member manifest"),
        "Python/Rust distinct states",
    )
    global_candidates = _validate_candidate_rows(rust.get("candidates"), section_count, "Rust member")
    per_dimension = _validate_rust_dimensions(
        rust, python_dimensions, rust_cardinality, global_candidates
    )

    return {
        "seed_index": seed_index,
        "seed": expected_seed,
        "world_generation": generation,
        "source_inventory_sha256": inventory_sha,
        "corpus_sha256": corpus_sha,
        "chunk_status_histogram": dict(sorted(status_histogram.items())),
        "section_count": section_count,
        "total_cells": section_count * 4096,
        "distinct_state_ids": _integer(manifest, "distinct_state_ids", "member manifest"),
        "section_lattice": lattice,
        "cardinality_histogram": dict(sorted(cardinality.items(), key=lambda item: int(item[0]))),
        "cell_facts": _nonnegative_mapping(manifest.get("cell_facts"), "member cell_facts"),
        "section_classes": _nonnegative_mapping(manifest.get("section_classes"), "member section_classes"),
        "per_dimension": per_dimension,
        "candidates": global_candidates,
    }


def _population_identity(
    *,
    plan: dict[str, object],
    state_target: dict[str, object],
    pinned_server_sha256: str,
    weighting: dict[str, Any],
    section_lattice: object,
    members: list[dict[str, object]],
) -> dict[str, object]:
    return {
        "schema": SCHEMA,
        "kind": POPULATION_KIND,
        "policy": plan["policy"],
        "plan_sha256": plan["plan_sha256"],
        "target": state_target,
        "server_sha256": pinned_server_sha256,
        "weighting": weighting,
        "section_lattice": section_lattice,
        "member_count": len(members),
        "members": [
            {
                "seed_index": member["seed_index"],
                "seed": member["seed"],
                "corpus_sha256": member["corpus_sha256"],
            }
            for member in members
        ],
    }


def build_set(
    *,
    plan: dict[str, object],
    state_manifest: dict[str, Any],
    pinned_server_sha256: str,
    member_inputs: list[
        tuple[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]
    ],
) -> dict[str, object]:
    section_representative_plan.validate_plan(plan)
    pinned_server_sha256 = _require_sha256(pinned_server_sha256, "pinned server SHA-256")
    seeds = plan["seeds"]
    assert isinstance(seeds, list)
    if len(member_inputs) != len(seeds):
        raise CorpusSetError(
            f"representative set requires exactly {len(seeds)} members; got {len(member_inputs)}"
        )
    state_target = _target_from_state_manifest(state_manifest)
    plan_target = _object(plan["target"], "plan target")
    for key in ("minecraft_version", "protocol_version", "data_version"):
        _equal(plan_target.get(key), state_target[key], f"plan/state target {key}")
    weighting = _object(plan.get("weighting"), "representative weighting")
    _equal(weighting.get("seed"), "equal", "seed weighting")
    _equal(weighting.get("dimension"), "report-separately", "dimension weighting")
    _equal(
        weighting.get("section"),
        "natural-within-selected-generated-chunks",
        "section weighting",
    )

    members = [
        validate_member(
            seed_index=index,
            plan=plan,
            state_target=state_target,
            pinned_server_sha256=pinned_server_sha256,
            world=world,
            extraction=extraction,
            manifest=manifest,
            rust=rust,
        )
        for index, (world, extraction, manifest, rust) in enumerate(member_inputs)
    ]
    reference_lattice = members[0]["section_lattice"]
    for member in members[1:]:
        _equal(member["section_lattice"], reference_lattice, "cross-seed section lattice")
    corpus_shas = [str(member["corpus_sha256"]) for member in members]
    if len(set(corpus_shas)) != len(corpus_shas):
        raise CorpusSetError("representative members must have distinct corpus SHA-256 identities")

    per_dimension: dict[str, dict[str, object]] = {}
    for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS:
        dimension = descriptor.key
        histogram: Counter[str] = Counter()
        candidate_totals = _empty_candidate_totals()
        member_distinct_states: list[int] = []
        section_count = 0
        for member in members:
            member_dimensions = _object(member["per_dimension"], "member per_dimension")
            summary = _object(member_dimensions.get(dimension), f"member {dimension}")
            member_sections = _integer(summary, "section_count", f"member {dimension}")
            section_count += member_sections
            histogram.update(_counter(summary.get("cardinality_histogram"), f"member {dimension} histogram"))
            member_distinct_states.append(_integer(summary, "distinct_state_ids", f"member {dimension}"))
            candidates = _object(summary.get("candidates"), f"member {dimension} candidates")
            _merge_candidate_metrics(candidate_totals, candidates)
        normalized_candidates = _normalize_candidate_totals(candidate_totals)
        for name, metrics in normalized_candidates.items():
            _equal(metrics["sections"], section_count, f"{dimension} {name} aggregate section count")
        if sum(histogram.values()) != section_count:
            raise CorpusSetError(f"{dimension} aggregate histogram does not sum to sections")
        per_dimension[dimension] = {
            "seed_weighting": "equal",
            "member_count": len(members),
            "section_count": section_count,
            "total_cells": section_count * 4096,
            "member_distinct_state_ids": member_distinct_states,
            "cardinality_histogram": dict(sorted(histogram.items(), key=lambda item: int(item[0]))),
            "candidates": normalized_candidates,
        }

    total_sections = sum(int(member["section_count"]) for member in members)
    descriptive_dimension_counts = {
        dimension: int(summary["section_count"])
        for dimension, summary in per_dimension.items()
    }
    _equal(sum(descriptive_dimension_counts.values()), total_sections, "descriptive section total")

    population_identity = _population_identity(
        plan=plan,
        state_target=state_target,
        pinned_server_sha256=pinned_server_sha256,
        weighting=weighting,
        section_lattice=reference_lattice,
        members=members,
    )
    population_sha256 = _canonical_digest(population_identity)
    result: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "policy": plan["policy"],
        "plan_sha256": plan["plan_sha256"],
        "decision_eligible": True,
        "decision_scope": "dimension-separated-only",
        "cross_dimension_score_allowed": False,
        "target": state_target,
        "server_sha256": pinned_server_sha256,
        "weighting": weighting,
        "section_lattice": reference_lattice,
        "member_count": len(members),
        "population_identity": population_identity,
        "population_sha256": population_sha256,
        "members": members,
        "per_dimension": per_dimension,
        "aggregate": {
            "descriptive_only": True,
            "section_count": total_sections,
            "total_cells": total_sections * 4096,
            "dimensions": descriptive_dimension_counts,
        },
    }
    result["evidence_sha256"] = _canonical_digest(result)
    return result


def load_member_inputs(
    root: Path,
    count: int,
) -> list[tuple[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]]:
    members = []
    for index in range(count):
        directory = root / f"seed-{index}"
        members.append(
            (
                _load_json(directory / "world-evidence.json"),
                _load_json(directory / "extraction-evidence.json"),
                _load_json(directory / "corpus-manifest.json"),
                _load_json(directory / "rust-import.json"),
            )
        )
    return members


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--members-root", type=Path, required=True)
    parser.add_argument("--state-manifest", type=Path, default=DEFAULT_STATE_MANIFEST)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        plan = section_representative_plan.load_plan(args.plan)
        state_manifest = _load_json(args.state_manifest)
        with args.lock.open("rb") as handle:
            lock = tomllib.load(handle)
        runtime = _object(lock.get("runtime"), "vanilla lock runtime")
        server_sha = _sha256(runtime, "server_sha256", "vanilla lock runtime")
        seeds = plan["seeds"]
        assert isinstance(seeds, list)
        member_inputs = load_member_inputs(args.members_root, len(seeds))
        result = build_set(
            plan=plan,
            state_manifest=state_manifest,
            pinned_server_sha256=server_sha,
            member_inputs=member_inputs,
        )
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (
        OSError,
        json.JSONDecodeError,
        tomllib.TOMLDecodeError,
        CorpusSetError,
        section_representative_plan.PlanError,
    ) as error:
        print(f"representative corpus-set error: {error}")
        return 1

    aggregate = result["aggregate"]
    assert isinstance(aggregate, dict)
    print(
        "representative corpus set: "
        f"members={result['member_count']} sections={aggregate['section_count']} "
        f"population_sha256={result['population_sha256']} "
        f"evidence_sha256={result['evidence_sha256']} PASS"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
