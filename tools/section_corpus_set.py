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
MEMBER_EXTRACTOR = "vanilla-save-region-v2-representative-member"
MEMBER_PURPOSE = "representative-member"
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


def _canonical_digest(payload: dict[str, object]) -> str:
    unsigned = {key: value for key, value in payload.items() if key != "set_sha256"}
    encoded = json.dumps(
        unsigned, sort_keys=True, separators=(",", ":"), ensure_ascii=True
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
    rust: dict[str, Any],
    section_count: int,
) -> dict[str, dict[str, object]]:
    rows = rust.get("candidates")
    if not isinstance(rows, list) or len(rows) != len(EXPECTED_CANDIDATES):
        raise CorpusSetError("Rust member evidence has the wrong candidate row count")
    observed: dict[str, dict[str, object]] = {}
    for index, raw_row in enumerate(rows):
        row = _object(raw_row, f"Rust candidate[{index}]")
        name = _string(row, "candidate", f"Rust candidate[{index}]")
        if name in observed or name not in EXPECTED_CANDIDATES:
            raise CorpusSetError(f"unexpected/duplicate Rust candidate {name!r}")
        _equal(
            _boolean(row, "production_candidate", f"Rust candidate[{index}]"),
            EXPECTED_CANDIDATES[name],
            f"{name} production flag",
        )
        _equal(
            _integer(row, "sections", f"Rust candidate[{index}]"),
            section_count,
            f"{name} section count",
        )
        total_owned = _integer(row, "total_owned_bytes", f"Rust candidate[{index}]")
        max_owned = _integer(row, "max_owned_bytes", f"Rust candidate[{index}]")
        transitions = _integer(
            row, "construction_transitions", f"Rust candidate[{index}]"
        )
        allocations = _integer(
            row, "logical_backing_allocations", f"Rust candidate[{index}]"
        )
        if min(total_owned, max_owned, transitions, allocations) < 0:
            raise CorpusSetError(f"{name} candidate metrics must be non-negative")
        representations = _counter(row.get("representations"), f"{name} representations")
        if sum(representations.values()) != section_count:
            raise CorpusSetError(f"{name} representation counts do not sum to sections")
        observed[name] = {
            "production_candidate": EXPECTED_CANDIDATES[name],
            "total_owned_bytes": total_owned,
            "max_owned_bytes": max_owned,
            "construction_transitions": transitions,
            "logical_backing_allocations": allocations,
            "representations": dict(sorted(representations.items())),
        }
    _equal(set(observed), set(EXPECTED_CANDIDATES), "Rust candidate set")
    return observed


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
    expected_batch_count = (
        expected_batches_per_dimension
        * len(section_representative_plan.REPRESENTATIVE_DIMENSIONS)
    )
    _equal(batch_count, expected_batch_count, "world batch count")

    raw_timings = world.get("batch_timings")
    if not isinstance(raw_timings, list) or len(raw_timings) != batch_count:
        raise CorpusSetError("world batch timing evidence is incomplete")
    tickets_by_dimension: Counter[str] = Counter()
    for index, raw_timing in enumerate(raw_timings):
        timing = _object(raw_timing, f"world batch timing[{index}]")
        _equal(_integer(timing, "index", f"world batch timing[{index}]"), index, "batch index")
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

    # Elapsed timings are deliberately validated but not returned: runner speed is not
    # part of stable representative corpus identity.
    return {
        "generator": WORLD_GENERATOR,
        "selection_command_sha256": SELECTION_COMMAND_SHA256,
        "batch_size": batch_size,
        "batch_count": batch_count,
        "batch_settle_seconds": settle_seconds,
    }


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
    _equal(
        _sha256(world, "server_sha256", "world evidence"),
        pinned_server_sha256,
        "server SHA-256",
    )
    _equal(world.get("representative_policy"), plan["policy"], "world representative policy")
    _equal(world.get("plan_sha256"), plan_sha, "world plan digest")
    _equal(_integer(world, "seed_index", "world evidence"), seed_index, "world seed index")
    _equal(_integer(world, "seed", "world evidence"), expected_seed, "world seed")

    _equal(_integer(extraction, "schema", "extraction evidence"), 1, "extraction schema")
    _equal(_string(extraction, "policy", "extraction evidence"), MEMBER_EXTRACTOR, "extractor")
    _equal(extraction.get("representative_policy"), plan["policy"], "extraction policy")
    _equal(extraction.get("plan_sha256"), plan_sha, "extraction plan digest")
    _equal(
        _integer(extraction, "seed_index", "extraction evidence"),
        seed_index,
        "extraction seed index",
    )
    _equal(_integer(extraction, "seed", "extraction evidence"), expected_seed, "extraction seed")
    _equal(extraction.get("selected_chunks"), _expected_selection(plan), "selected chunk schedule")

    lattice_raw = _object(extraction.get("section_lattice"), "section lattice")
    lattice: dict[str, list[int]] = {}
    expected_dimensions: dict[str, int] = {}
    for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS:
        dimension = descriptor.key
        raw_values = lattice_raw.get(dimension)
        if not isinstance(raw_values, list) or not raw_values:
            raise CorpusSetError(f"section lattice for {dimension} must be a non-empty list")
        values = [
            _integer({"value": value}, "value", f"{dimension} lattice")
            for value in raw_values
        ]
        if values != list(range(values[0], values[-1] + 1)):
            raise CorpusSetError(f"section lattice for {dimension} is not contiguous")
        lattice[dimension] = values
        expected_dimensions[dimension] = (
            section_representative_plan.CHUNKS_PER_DIMENSION * len(values)
        )
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
    _equal(
        _integer(manifest, "total_cells", "member manifest"),
        section_count * 4096,
        "member total cells",
    )
    cardinality = _counter(manifest.get("cardinality_histogram"), "member cardinality histogram")
    if sum(cardinality.values()) != section_count:
        raise CorpusSetError("member cardinality histogram does not sum to section count")

    _equal(_integer(rust, "schema", "Rust member evidence"), 1, "Rust schema")
    _equal(rust.get("kind"), "section-corpus-import-check", "Rust evidence kind")
    for key, expected in state_target.items():
        _equal(rust.get(key), expected, f"Rust target {key}")
    _equal(rust.get("source_inventory_sha256"), inventory_sha, "Rust source inventory")
    _equal(rust.get("extractor"), MEMBER_EXTRACTOR, "Rust extractor")
    _equal(rust.get("purpose"), MEMBER_PURPOSE, "Rust member purpose")
    _equal(
        _boolean(rust, "decision_requested", "Rust member evidence"),
        False,
        "member decision request",
    )
    _equal(
        _boolean(rust, "decision_eligible", "Rust member evidence"),
        False,
        "member decision eligibility",
    )
    _equal(
        _integer(rust, "section_count", "Rust member evidence"),
        section_count,
        "Rust section count",
    )
    _equal(rust.get("dimensions"), expected_dimensions, "Rust dimensions")
    _equal(
        rust.get("cardinality_histogram"),
        manifest.get("cardinality_histogram"),
        "Python/Rust cardinality histogram",
    )
    _equal(
        _integer(rust, "distinct_state_ids", "Rust member evidence"),
        _integer(manifest, "distinct_state_ids", "member manifest"),
        "Python/Rust distinct states",
    )
    candidates = _validate_candidate_rows(rust, section_count)

    return {
        "seed_index": seed_index,
        "seed": expected_seed,
        "world_generation": generation,
        "source_inventory_sha256": inventory_sha,
        "corpus_sha256": corpus_sha,
        "section_count": section_count,
        "total_cells": section_count * 4096,
        "distinct_state_ids": _integer(manifest, "distinct_state_ids", "member manifest"),
        "dimensions": expected_dimensions,
        "section_lattice": lattice,
        "cardinality_histogram": dict(
            sorted(cardinality.items(), key=lambda item: int(item[0]))
        ),
        "cell_facts": manifest.get("cell_facts"),
        "section_classes": manifest.get("section_classes"),
        "candidates": candidates,
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

    aggregate_dimensions: Counter[str] = Counter()
    aggregate_cardinality: Counter[str] = Counter()
    aggregate_cell_facts: Counter[str] = Counter()
    aggregate_section_classes: Counter[str] = Counter()
    candidate_totals: dict[str, dict[str, object]] = {
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
    for member in members:
        aggregate_dimensions.update(member["dimensions"])
        aggregate_cardinality.update(member["cardinality_histogram"])
        aggregate_cell_facts.update(_counter(member["cell_facts"], "member cell_facts"))
        aggregate_section_classes.update(
            _counter(member["section_classes"], "member section_classes")
        )
        member_candidates = member["candidates"]
        assert isinstance(member_candidates, dict)
        for name, metrics in member_candidates.items():
            target = candidate_totals[name]
            target["sections"] = int(target["sections"]) + int(member["section_count"])
            target["total_owned_bytes"] = int(target["total_owned_bytes"]) + int(
                metrics["total_owned_bytes"]
            )
            target["max_owned_bytes"] = max(
                int(target["max_owned_bytes"]), int(metrics["max_owned_bytes"])
            )
            target["construction_transitions"] = int(
                target["construction_transitions"]
            ) + int(metrics["construction_transitions"])
            target["logical_backing_allocations"] = int(
                target["logical_backing_allocations"]
            ) + int(metrics["logical_backing_allocations"])
            reps = target["representations"]
            assert isinstance(reps, Counter)
            reps.update(metrics["representations"])

    total_sections = sum(int(member["section_count"]) for member in members)
    normalized_candidates = {}
    for name, metrics in candidate_totals.items():
        reps = metrics.pop("representations")
        assert isinstance(reps, Counter)
        metrics["sections"] = total_sections
        metrics["representations"] = dict(sorted(reps.items()))
        normalized_candidates[name] = metrics

    result: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "policy": plan["policy"],
        "plan_sha256": plan["plan_sha256"],
        "decision_eligible": True,
        "target": state_target,
        "server_sha256": pinned_server_sha256,
        "weighting": plan["weighting"],
        "section_lattice": reference_lattice,
        "member_count": len(members),
        "members": members,
        "aggregate": {
            "section_count": total_sections,
            "total_cells": total_sections * 4096,
            "dimensions": dict(sorted(aggregate_dimensions.items())),
            "cardinality_histogram": dict(
                sorted(aggregate_cardinality.items(), key=lambda item: int(item[0]))
            ),
            "cell_facts": dict(sorted(aggregate_cell_facts.items())),
            "section_classes": dict(sorted(aggregate_section_classes.items())),
            "candidates": normalized_candidates,
        },
    }
    result["set_sha256"] = _canonical_digest(result)
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
        f"set_sha256={result['set_sha256']} PASS"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
