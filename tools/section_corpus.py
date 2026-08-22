#!/usr/bin/env python3
"""Validate and summarize normalized vanilla-derived section corpora."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

MAGIC = "CRUCIBLE-SECTION-CORPUS|1"
FORMAT_NAME = "CRUCIBLE-SECTION-CORPUS/1"
MANIFEST_SCHEMA = 1
SECTION_CELLS = 4096
RESOURCE_LOCATION = re.compile(r"[a-z0-9_.-]+:[a-z0-9_./-]+\Z")
LOWER_SHA256 = re.compile(r"[0-9a-f]{64}\Z")
CANONICAL_INT = re.compile(r"(?:0|-?[1-9][0-9]*)\Z")
TOKEN = re.compile(r"[A-Za-z0-9_.-]+\Z")


class CorpusError(ValueError):
    """Raised when a corpus or its pinned target evidence is invalid."""


@dataclass(frozen=True, order=True)
class SectionKey:
    dimension: str
    chunk_x: int
    chunk_z: int
    section_y: int


@dataclass(frozen=True)
class TargetEvidence:
    minecraft_version: str
    protocol_version: int
    data_version: int
    state_count: int
    generation_sha256: str
    input_sha256: str
    flags: tuple[int, ...]


@dataclass(frozen=True)
class SourceEvidence:
    kind: str
    inventory_sha256: str
    extractor: str


@dataclass(frozen=True)
class ParsedCorpus:
    target: TargetEvidence
    source: SourceEvidence
    section_count: int
    total_cells: int
    distinct_state_ids: int
    cardinality_histogram: dict[int, int]
    dimensions: dict[str, int]
    cell_facts: dict[str, int]
    section_classes: dict[str, int]
    corpus_sha256: str

    def manifest(self) -> dict[str, object]:
        return {
            "schema": MANIFEST_SCHEMA,
            "format": FORMAT_NAME,
            "corpus_sha256": self.corpus_sha256,
            "target": {
                "minecraft_version": self.target.minecraft_version,
                "protocol_version": self.target.protocol_version,
                "data_version": self.target.data_version,
                "state_count": self.target.state_count,
                "state_data_generation_sha256": self.target.generation_sha256,
                "state_data_input_sha256": self.target.input_sha256,
            },
            "source": {
                "kind": self.source.kind,
                "inventory_sha256": self.source.inventory_sha256,
                "extractor": self.source.extractor,
            },
            "section_count": self.section_count,
            "total_cells": self.total_cells,
            "distinct_state_ids": self.distinct_state_ids,
            "cardinality_histogram": {
                str(key): value for key, value in sorted(self.cardinality_histogram.items())
            },
            "dimensions": dict(sorted(self.dimensions.items())),
            "cell_facts": self.cell_facts,
            "section_classes": self.section_classes,
        }


def _require_mapping(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise CorpusError(f"{label} must be a JSON object")
    return value


def _require_int(mapping: dict[str, object], key: str) -> int:
    value = mapping.get(key)
    if isinstance(value, bool) or not isinstance(value, int):
        raise CorpusError(f"{key} must be an integer")
    return value


def _require_str(mapping: dict[str, object], key: str) -> str:
    value = mapping.get(key)
    if not isinstance(value, str) or not value:
        raise CorpusError(f"{key} must be a non-empty string")
    return value


def load_target_evidence(state_manifest_path: Path, generated_rust_path: Path) -> TargetEvidence:
    manifest = _require_mapping(json.loads(state_manifest_path.read_text(encoding="utf-8")), "state manifest")
    target = _require_mapping(manifest.get("target"), "state manifest target")
    minecraft_version = _require_str(target, "minecraft_version")
    protocol_version = _require_int(target, "protocol_version")
    data_version = _require_int(target, "data_version")
    state_count = _require_int(manifest, "state_count")
    generation_sha256 = _require_str(manifest, "generation_digest")
    input_sha256 = _require_str(manifest, "input_digest")
    if not LOWER_SHA256.fullmatch(generation_sha256) or not LOWER_SHA256.fullmatch(input_sha256):
        raise CorpusError("state manifest digests must be lowercase SHA-256")

    generated = generated_rust_path.read_text(encoding="utf-8")
    flags_match = re.search(
        r"pub static STATE_MUTATION_FLAGS: \[u8; BLOCK_STATE_COUNT\] = \[(.*?)\];",
        generated,
        flags=re.DOTALL,
    )
    if flags_match is None:
        raise CorpusError("generated Rust does not contain STATE_MUTATION_FLAGS")
    raw_flags = [part.strip() for part in flags_match.group(1).split(",") if part.strip()]
    try:
        flags = tuple(int(part) for part in raw_flags)
    except ValueError as error:
        raise CorpusError("generated STATE_MUTATION_FLAGS contains a non-integer") from error
    if len(flags) != state_count:
        raise CorpusError(
            f"generated STATE_MUTATION_FLAGS has {len(flags)} entries; expected {state_count}"
        )
    if any(flag < 0 or flag > 0x0F for flag in flags):
        raise CorpusError("generated STATE_MUTATION_FLAGS contains bits outside the four fact flags")

    generated_markers = {
        "state_count": rf"pub const BLOCK_STATE_COUNT: usize = {state_count};",
        "generation": rf'pub const STATE_DATA_GENERATION_SHA256: &str = "{re.escape(generation_sha256)}";',
        "input": rf'pub const STATE_DATA_INPUT_SHA256: &str = "{re.escape(input_sha256)}";',
    }
    for label, pattern in generated_markers.items():
        if re.search(pattern, generated) is None:
            raise CorpusError(f"generated Rust disagrees with state manifest: {label}")

    return TargetEvidence(
        minecraft_version=minecraft_version,
        protocol_version=protocol_version,
        data_version=data_version,
        state_count=state_count,
        generation_sha256=generation_sha256,
        input_sha256=input_sha256,
        flags=flags,
    )


def _parse_canonical_int(raw: str, label: str) -> int:
    if not CANONICAL_INT.fullmatch(raw):
        raise CorpusError(f"{label} is not a canonical decimal integer: {raw!r}")
    value = int(raw)
    if value == 0 and raw != "0":
        raise CorpusError(f"{label} has a noncanonical zero")
    return value


def _parse_kv_line(line: str, prefix: str, expected_keys: tuple[str, ...]) -> dict[str, str]:
    parts = line.split("|")
    if not parts or parts[0] != prefix:
        raise CorpusError(f"expected {prefix} header")
    if len(parts) != len(expected_keys) + 1:
        raise CorpusError(f"{prefix} header has the wrong number of fields")
    parsed: dict[str, str] = {}
    for part, key in zip(parts[1:], expected_keys, strict=True):
        expected_prefix = f"{key}="
        if not part.startswith(expected_prefix):
            raise CorpusError(f"{prefix} header expected field {key}")
        value = part[len(expected_prefix) :]
        if not value:
            raise CorpusError(f"{prefix} header field {key} is empty")
        parsed[key] = value
    return parsed


def _validate_target_header(line: str, target: TargetEvidence) -> None:
    values = _parse_kv_line(
        line,
        "TARGET",
        ("minecraft", "protocol", "data", "state_count", "generation_sha256"),
    )
    expected = {
        "minecraft": target.minecraft_version,
        "protocol": str(target.protocol_version),
        "data": str(target.data_version),
        "state_count": str(target.state_count),
        "generation_sha256": target.generation_sha256,
    }
    if values != expected:
        raise CorpusError(f"corpus target header does not match frozen state data: {values!r}")


def _parse_source_header(line: str) -> SourceEvidence:
    values = _parse_kv_line(line, "SOURCE", ("kind", "inventory_sha256", "extractor"))
    if values["kind"] != "vanilla-save":
        raise CorpusError("corpus source kind must be vanilla-save in schema 1")
    if not LOWER_SHA256.fullmatch(values["inventory_sha256"]):
        raise CorpusError("source inventory_sha256 must be lowercase SHA-256")
    if not TOKEN.fullmatch(values["extractor"]):
        raise CorpusError("source extractor identifier is not canonical")
    return SourceEvidence(
        kind=values["kind"],
        inventory_sha256=values["inventory_sha256"],
        extractor=values["extractor"],
    )


def _section_key(parts: list[str], line_number: int) -> SectionKey:
    if not RESOURCE_LOCATION.fullmatch(parts[1]):
        raise CorpusError(f"line {line_number}: invalid dimension resource location {parts[1]!r}")
    return SectionKey(
        dimension=parts[1],
        chunk_x=_parse_canonical_int(parts[2], f"line {line_number} chunk_x"),
        chunk_z=_parse_canonical_int(parts[3], f"line {line_number} chunk_z"),
        section_y=_parse_canonical_int(parts[4], f"line {line_number} section_y"),
    )


def _parse_states(raw: str, line_number: int, state_count: int) -> tuple[int, ...]:
    parts = raw.split(",")
    if len(parts) != SECTION_CELLS:
        raise CorpusError(
            f"line {line_number}: section has {len(parts)} cells; expected {SECTION_CELLS}"
        )
    states: list[int] = []
    for cell, token in enumerate(parts):
        if not re.fullmatch(r"(?:0|[1-9][0-9]*)", token):
            raise CorpusError(f"line {line_number} cell {cell}: noncanonical state ID {token!r}")
        state = int(token)
        if state >= state_count:
            raise CorpusError(
                f"line {line_number} cell {cell}: state ID {state} outside 0..{state_count - 1}"
            )
        states.append(state)
    return tuple(states)


def _count_cell_facts(states: Iterable[int], flags: tuple[int, ...], totals: Counter[str]) -> None:
    for state in states:
        flag = flags[state]
        totals["non_air"] += int(bool(flag & 0x01))
        totals["counted_fluid"] += int(bool(flag & 0x02))
        totals["random_block"] += int(bool(flag & 0x04))
        totals["random_fluid"] += int(bool(flag & 0x08))


def validate_corpus(path: Path, target: TargetEvidence) -> ParsedCorpus:
    raw = path.read_bytes()
    if not raw:
        raise CorpusError("corpus is empty")
    if b"\r" in raw:
        raise CorpusError("corpus must use canonical LF line endings")
    if not raw.endswith(b"\n"):
        raise CorpusError("corpus must end with a newline")
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as error:
        raise CorpusError("corpus must be UTF-8") from error
    lines = text.splitlines()
    if len(lines) < 4:
        raise CorpusError("corpus must contain headers and at least one section")
    if any(not line for line in lines):
        raise CorpusError("corpus must not contain blank lines")
    if lines[0] != MAGIC:
        raise CorpusError(f"unsupported corpus magic/schema: {lines[0]!r}")
    _validate_target_header(lines[1], target)
    source = _parse_source_header(lines[2])

    previous_key: SectionKey | None = None
    all_states: set[int] = set()
    cardinality_histogram: Counter[int] = Counter()
    dimensions: Counter[str] = Counter()
    cell_facts: Counter[str] = Counter()
    section_classes: Counter[str] = Counter()

    for line_number, line in enumerate(lines[3:], start=4):
        parts = line.split("|", maxsplit=5)
        if len(parts) != 6 or parts[0] != "SECTION":
            raise CorpusError(f"line {line_number}: expected SECTION record with six fields")
        key = _section_key(parts, line_number)
        if previous_key is not None and key <= previous_key:
            relation = "duplicate" if key == previous_key else "out of order"
            raise CorpusError(f"line {line_number}: section coordinate is {relation}: {key}")
        previous_key = key
        states = _parse_states(parts[5], line_number, target.state_count)
        unique = set(states)
        cardinality_histogram[len(unique)] += 1
        dimensions[key.dimension] += 1
        all_states.update(unique)

        section_fact_counts: Counter[str] = Counter()
        _count_cell_facts(states, target.flags, section_fact_counts)
        cell_facts.update(section_fact_counts)
        section_classes["all_air"] += int(section_fact_counts["non_air"] == 0)
        section_classes["contains_fluid"] += int(section_fact_counts["counted_fluid"] > 0)
        section_classes["random_block_present"] += int(section_fact_counts["random_block"] > 0)
        section_classes["random_fluid_present"] += int(section_fact_counts["random_fluid"] > 0)

    section_count = len(lines) - 3
    return ParsedCorpus(
        target=target,
        source=source,
        section_count=section_count,
        total_cells=section_count * SECTION_CELLS,
        distinct_state_ids=len(all_states),
        cardinality_histogram=dict(cardinality_histogram),
        dimensions=dict(dimensions),
        cell_facts={
            "non_air": cell_facts["non_air"],
            "counted_fluid": cell_facts["counted_fluid"],
            "random_block": cell_facts["random_block"],
            "random_fluid": cell_facts["random_fluid"],
        },
        section_classes={
            "all_air": section_classes["all_air"],
            "contains_fluid": section_classes["contains_fluid"],
            "random_block_present": section_classes["random_block_present"],
            "random_fluid_present": section_classes["random_fluid_present"],
        },
        corpus_sha256=hashlib.sha256(raw).hexdigest(),
    )


def canonical_json(value: object) -> str:
    return json.dumps(value, indent=2, sort_keys=True) + "\n"


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("validate", "inspect"):
        sub = subparsers.add_parser(command)
        sub.add_argument("corpus", type=Path)
        sub.add_argument(
            "--state-manifest",
            type=Path,
            default=Path("vanilla/state-data/26.2-state-data-manifest.json"),
        )
        sub.add_argument(
            "--generated-rust",
            type=Path,
            default=Path("crates/data/crucible-generated/src/lib.rs"),
        )
        if command == "validate":
            sub.add_argument("--manifest", type=Path)
    return parser


def main() -> int:
    args = _parser().parse_args()
    try:
        target = load_target_evidence(args.state_manifest, args.generated_rust)
        parsed = validate_corpus(args.corpus, target)
    except (CorpusError, OSError, json.JSONDecodeError) as error:
        print(f"section corpus error: {error}")
        return 1

    manifest = canonical_json(parsed.manifest())
    if args.command == "inspect":
        print(manifest, end="")
        return 0
    if args.manifest is not None:
        args.manifest.parent.mkdir(parents=True, exist_ok=True)
        args.manifest.write_text(manifest, encoding="utf-8")
        print(f"section corpus manifest: {args.manifest}")
    print(
        "section corpus: "
        f"sections={parsed.section_count} cells={parsed.total_cells} "
        f"states={parsed.distinct_state_ids} sha256={parsed.corpus_sha256} PASS"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
