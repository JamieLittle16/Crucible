#!/usr/bin/env python3
"""Cross-check Python corpus evidence against the independent Rust importer result."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

EXPECTED_CANDIDATES = {
    "direct-reference": False,
    "direct": True,
    "adaptive": True,
    "fast-local": True,
    "packed-local": True,
}
EXPECTED_PURPOSE = "parser-admission"
EXPECTED_EXTRACTOR = "vanilla-save-region-v1-stored-sections"


class EvidenceError(ValueError):
    """Raised when the Python and Rust corpus evidence disagree."""


def _mapping(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceError(f"{label} must be a JSON object")
    return value


def _integer(mapping: dict[str, Any], key: str, label: str) -> int:
    value = mapping.get(key)
    if isinstance(value, bool) or not isinstance(value, int):
        raise EvidenceError(f"{label}.{key} must be an integer")
    return value


def _string(mapping: dict[str, Any], key: str, label: str) -> str:
    value = mapping.get(key)
    if not isinstance(value, str) or not value:
        raise EvidenceError(f"{label}.{key} must be a non-empty string")
    return value


def _boolean(mapping: dict[str, Any], key: str, label: str) -> bool:
    value = mapping.get(key)
    if not isinstance(value, bool):
        raise EvidenceError(f"{label}.{key} must be a boolean")
    return value


def _equal(actual: object, expected: object, label: str) -> None:
    if actual != expected:
        raise EvidenceError(f"{label} mismatch: expected {expected!r}, got {actual!r}")


def crosscheck(corpus_manifest: dict[str, Any], rust_import: dict[str, Any]) -> None:
    """Require the Rust importer to describe exactly the Python-validated corpus."""

    corpus = _mapping(corpus_manifest, "corpus manifest")
    rust = _mapping(rust_import, "Rust import evidence")
    target = _mapping(corpus.get("target"), "corpus manifest target")
    source = _mapping(corpus.get("source"), "corpus manifest source")

    _equal(_integer(rust, "schema", "Rust import evidence"), 1, "Rust schema")
    _equal(
        _string(rust, "kind", "Rust import evidence"),
        "section-corpus-import-check",
        "Rust evidence kind",
    )

    for rust_key, target_key in (
        ("minecraft_version", "minecraft_version"),
        ("protocol_version", "protocol_version"),
        ("data_version", "data_version"),
        ("state_data_generation_sha256", "state_data_generation_sha256"),
        ("state_data_input_sha256", "state_data_input_sha256"),
    ):
        _equal(rust.get(rust_key), target.get(target_key), f"target {rust_key}")

    _equal(
        _integer(rust, "state_count", "Rust import evidence"),
        _integer(target, "state_count", "corpus manifest target"),
        "target state_count",
    )
    _equal(
        _string(rust, "source_inventory_sha256", "Rust import evidence"),
        _string(source, "inventory_sha256", "corpus manifest source"),
        "source inventory SHA-256",
    )
    _equal(
        _string(rust, "extractor", "Rust import evidence"),
        _string(source, "extractor", "corpus manifest source"),
        "source extractor",
    )
    _equal(rust["extractor"], EXPECTED_EXTRACTOR, "parser-admission extractor")
    _equal(_string(rust, "purpose", "Rust import evidence"), EXPECTED_PURPOSE, "corpus purpose")
    _equal(_boolean(rust, "decision_requested", "Rust import evidence"), False, "decision request")
    _equal(_boolean(rust, "decision_eligible", "Rust import evidence"), False, "decision eligibility")

    section_count = _integer(corpus, "section_count", "corpus manifest")
    _equal(_integer(rust, "section_count", "Rust import evidence"), section_count, "section_count")
    _equal(
        _integer(rust, "total_cells", "Rust import evidence"),
        section_count * 4096,
        "total_cells",
    )
    _equal(
        _integer(rust, "distinct_state_ids", "Rust import evidence"),
        _integer(corpus, "distinct_state_ids", "corpus manifest"),
        "distinct_state_ids",
    )
    _equal(rust.get("cardinality_histogram"), corpus.get("cardinality_histogram"), "cardinality histogram")
    _equal(rust.get("dimensions"), corpus.get("dimensions"), "dimension histogram")

    raw_candidates = rust.get("candidates")
    if not isinstance(raw_candidates, list):
        raise EvidenceError("Rust import evidence.candidates must be an array")
    if len(raw_candidates) != len(EXPECTED_CANDIDATES):
        raise EvidenceError(
            f"Rust import evidence must contain {len(EXPECTED_CANDIDATES)} candidates"
        )

    observed_names: set[str] = set()
    for index, raw_candidate in enumerate(raw_candidates):
        candidate = _mapping(raw_candidate, f"candidate[{index}]")
        name = _string(candidate, "candidate", f"candidate[{index}]")
        if name in observed_names:
            raise EvidenceError(f"duplicate Rust candidate {name!r}")
        observed_names.add(name)
        if name not in EXPECTED_CANDIDATES:
            raise EvidenceError(f"unexpected Rust candidate {name!r}")
        _equal(
            _boolean(candidate, "production_candidate", f"candidate[{index}]"),
            EXPECTED_CANDIDATES[name],
            f"{name} production flag",
        )
        _equal(
            _integer(candidate, "sections", f"candidate[{index}]"),
            section_count,
            f"{name} section count",
        )
        total_owned = _integer(candidate, "total_owned_bytes", f"candidate[{index}]")
        max_owned = _integer(candidate, "max_owned_bytes", f"candidate[{index}]")
        if total_owned < max_owned:
            raise EvidenceError(f"{name} total owned bytes are smaller than max section bytes")
        representations = _mapping(candidate.get("representations"), f"{name} representations")
        representation_sections = 0
        for representation, count in representations.items():
            if not isinstance(representation, str) or not representation:
                raise EvidenceError(f"{name} has an invalid representation name")
            if isinstance(count, bool) or not isinstance(count, int) or count < 0:
                raise EvidenceError(f"{name} representation count must be a nonnegative integer")
            representation_sections += count
        _equal(representation_sections, section_count, f"{name} representation section total")

    _equal(observed_names, set(EXPECTED_CANDIDATES), "candidate set")


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--rust-import", type=Path, required=True)
    return parser


def main() -> int:
    args = _parser().parse_args()
    try:
        corpus_manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        rust_import = json.loads(args.rust_import.read_text(encoding="utf-8"))
        crosscheck(corpus_manifest, rust_import)
    except (EvidenceError, OSError, json.JSONDecodeError) as error:
        print(f"section corpus import evidence error: {error}")
        return 1
    print("section corpus Python/Rust evidence cross-check PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
