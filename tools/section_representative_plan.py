#!/usr/bin/env python3
"""Define and validate Crucible's deterministic vanilla section sampling plan."""

from __future__ import annotations

import argparse
import hashlib
import json
from dataclasses import dataclass
from pathlib import Path

import vanilla_dimensions

POLICY_ID = "vanilla-section-representative-v1"
SCHEMA = 1
MINECRAFT_VERSION = "26.2"
PROTOCOL_VERSION = 776
DATA_VERSION = 4903
SEED_COUNT = 4
CHUNKS_PER_DIMENSION = 64
SEED_DOMAIN = "Crucible|Minecraft-Java-26.2|section-representative-v1"


@dataclass(frozen=True, slots=True)
class RepresentativeDimensionDescriptor:
    """Sampling policy for one vanilla dimension.

    Target/save identity comes from ``vanilla_dimensions``.  This descriptor owns only
    the representative sampling policy, keeping generation/extraction mechanisms free
    of dimension-name branches.
    """

    vanilla: vanilla_dimensions.VanillaDimensionDescriptor
    anchors: tuple[tuple[int, int], ...]
    radius_chunks: int
    coordinate_policy: str
    minimum_chebyshev_radius: int = 0

    @property
    def key(self) -> str:
        return self.vanilla.key

    def accepts_candidate(self, candidate: tuple[int, int]) -> bool:
        return max(abs(candidate[0]), abs(candidate[1])) >= self.minimum_chebyshev_radius


REPRESENTATIVE_DIMENSIONS: tuple[RepresentativeDimensionDescriptor, ...] = (
    RepresentativeDimensionDescriptor(
        vanilla=vanilla_dimensions.require_standard_dimension("minecraft:overworld"),
        anchors=(
            (0, 0),
            (8, 0),
            (-8, 0),
            (0, 8),
            (0, -8),
            (128, 128),
            (-128, 128),
            (256, -256),
        ),
        radius_chunks=2048,
        coordinate_policy="anchors+sha256-square-v1",
    ),
    RepresentativeDimensionDescriptor(
        vanilla=vanilla_dimensions.require_standard_dimension("minecraft:the_nether"),
        anchors=(
            (0, 0),
            (8, 0),
            (-8, 0),
            (0, 8),
            (0, -8),
            (128, 128),
            (-128, 128),
            (256, -256),
        ),
        radius_chunks=2048,
        coordinate_policy="anchors+sha256-square-v1",
    ),
    RepresentativeDimensionDescriptor(
        vanilla=vanilla_dimensions.require_standard_dimension("minecraft:the_end"),
        anchors=(
            (0, 0),
            (4, 0),
            (-4, 0),
            (0, 4),
            (0, -4),
            (80, 0),
            (-80, 0),
            (0, 80),
            (0, -80),
        ),
        radius_chunks=512,
        coordinate_policy="central-anchors+sha256-outer-square-v1",
        minimum_chebyshev_radius=80,
    ),
)

DIMENSIONS = tuple(descriptor.key for descriptor in REPRESENTATIVE_DIMENSIONS)
DIMENSION_BY_KEY = {descriptor.key: descriptor for descriptor in REPRESENTATIVE_DIMENSIONS}

if len(DIMENSION_BY_KEY) != len(REPRESENTATIVE_DIMENSIONS):
    raise RuntimeError("duplicate representative dimension key")

# Compatibility/readability views.  They are derived from descriptors so there remains
# only one source of truth for the sampling policy.
ANCHORS = {descriptor.key: descriptor.anchors for descriptor in REPRESENTATIVE_DIMENSIONS}
RANGES = {descriptor.key: descriptor.radius_chunks for descriptor in REPRESENTATIVE_DIMENSIONS}


class PlanError(ValueError):
    """Raised when a representative sampling plan is malformed or stale."""


def _signed64(raw: bytes) -> int:
    value = int.from_bytes(raw, "big")
    return value if value < (1 << 63) else value - (1 << 64)


def derive_seeds() -> list[int]:
    digest = hashlib.sha512(SEED_DOMAIN.encode("utf-8")).digest()
    return [_signed64(digest[index * 8 : (index + 1) * 8]) for index in range(SEED_COUNT)]


def require_dimension(dimension: str) -> RepresentativeDimensionDescriptor:
    try:
        return DIMENSION_BY_KEY[dimension]
    except KeyError as error:
        raise PlanError(f"unsupported representative dimension: {dimension}") from error


def _candidate(dimension: str, counter: int, radius: int) -> tuple[int, int]:
    digest = hashlib.sha256(
        f"{POLICY_ID}|{dimension}|chunk|{counter}".encode("utf-8")
    ).digest()
    width = radius * 2 + 1
    x = int.from_bytes(digest[:8], "big") % width - radius
    z = int.from_bytes(digest[8:16], "big") % width - radius
    return x, z


def derive_chunks(dimension: str) -> list[list[int]]:
    descriptor = require_dimension(dimension)
    chunks = list(descriptor.anchors)
    seen = set(chunks)
    counter = 0
    while len(chunks) < CHUNKS_PER_DIMENSION:
        candidate = _candidate(dimension, counter, descriptor.radius_chunks)
        counter += 1
        if not descriptor.accepts_candidate(candidate):
            continue
        if candidate in seen:
            continue
        seen.add(candidate)
        chunks.append(candidate)
    return [[x, z] for x, z in chunks]


def _without_digest(plan: dict[str, object]) -> dict[str, object]:
    return {key: value for key, value in plan.items() if key != "plan_sha256"}


def digest_plan(plan: dict[str, object]) -> str:
    canonical = json.dumps(
        _without_digest(plan),
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=True,
    ).encode("utf-8")
    return hashlib.sha256(canonical).hexdigest()


def build_plan() -> dict[str, object]:
    plan: dict[str, object] = {
        "schema": SCHEMA,
        "policy": POLICY_ID,
        "target": {
            "minecraft_version": MINECRAFT_VERSION,
            "protocol_version": PROTOCOL_VERSION,
            "data_version": DATA_VERSION,
        },
        "seed_derivation": {
            "algorithm": "sha512-signed64-be-v1",
            "domain": SEED_DOMAIN,
            "count": SEED_COUNT,
        },
        "seeds": derive_seeds(),
        "chunks_per_dimension": CHUNKS_PER_DIMENSION,
        "dimensions": {
            descriptor.key: {
                "coordinate_policy": descriptor.coordinate_policy,
                "radius_chunks": descriptor.radius_chunks,
                "chunks": derive_chunks(descriptor.key),
            }
            for descriptor in REPRESENTATIVE_DIMENSIONS
        },
        "weighting": {
            "seed": "equal",
            "dimension": "report-separately",
            "section": "natural-within-selected-generated-chunks",
        },
        "selection_guards": {
            "content_independent": True,
            "same_coordinates_across_seeds": True,
            "require_every_selected_chunk": True,
            "require_contiguous_uniform_section_lattice_per_dimension": True,
            "allow_unplanned_chunks_in_source_world": True,
            "include_unplanned_chunks_in_corpus": False,
        },
    }
    plan["plan_sha256"] = digest_plan(plan)
    return plan


def _integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise PlanError(f"{label} must be an integer")
    return value


def validate_plan(plan: object) -> dict[str, object]:
    if not isinstance(plan, dict):
        raise PlanError("representative plan root must be an object")
    expected = build_plan()
    if plan != expected:
        observed_digest = plan.get("plan_sha256")
        expected_digest = expected["plan_sha256"]
        raise PlanError(
            "representative plan differs from the frozen deterministic policy: "
            f"expected {expected_digest}, got {observed_digest}"
        )

    dimensions = plan.get("dimensions")
    assert isinstance(dimensions, dict)
    for descriptor in REPRESENTATIVE_DIMENSIONS:
        dimension = descriptor.key
        entry = dimensions[dimension]
        assert isinstance(entry, dict)
        chunks = entry["chunks"]
        assert isinstance(chunks, list)
        if len(chunks) != CHUNKS_PER_DIMENSION:
            raise PlanError(f"{dimension} must contain {CHUNKS_PER_DIMENSION} chunks")
        seen: set[tuple[int, int]] = set()
        for index, chunk in enumerate(chunks):
            if not isinstance(chunk, list) or len(chunk) != 2:
                raise PlanError(f"{dimension} chunk[{index}] must be [x,z]")
            x = _integer(chunk[0], f"{dimension} chunk[{index}].x")
            z = _integer(chunk[1], f"{dimension} chunk[{index}].z")
            if (x, z) in seen:
                raise PlanError(f"{dimension} contains duplicate chunk {(x, z)}")
            seen.add((x, z))
    return plan


def write_plan(path: Path) -> None:
    plan = build_plan()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def load_plan(path: Path) -> dict[str, object]:
    return validate_plan(json.loads(path.read_text(encoding="utf-8")))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    write = subparsers.add_parser("write")
    write.add_argument("output", type=Path)
    verify = subparsers.add_parser("verify")
    verify.add_argument("plan", type=Path)
    inspect = subparsers.add_parser("inspect")
    inspect.add_argument("plan", type=Path)
    args = parser.parse_args()

    try:
        if args.command == "write":
            write_plan(args.output)
            print(f"representative section plan: {args.output} PASS")
            return 0
        plan = load_plan(args.plan)
        if args.command == "verify":
            print(f"representative section plan: {plan['plan_sha256']} PASS")
            return 0
        print(json.dumps(plan, indent=2, sort_keys=True))
        return 0
    except (OSError, json.JSONDecodeError, PlanError) as error:
        print(f"representative section plan error: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
