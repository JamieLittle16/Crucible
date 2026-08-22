from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import section_corpus_import_evidence as evidence


GENERATION = "1" * 64
INPUT = "2" * 64
INVENTORY = "3" * 64


def manifest() -> dict[str, object]:
    return {
        "schema": 1,
        "format": "CRUCIBLE-SECTION-CORPUS/1",
        "corpus_sha256": "4" * 64,
        "target": {
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "data_version": 4903,
            "state_count": 32366,
            "state_data_generation_sha256": GENERATION,
            "state_data_input_sha256": INPUT,
        },
        "source": {
            "kind": "vanilla-save",
            "inventory_sha256": INVENTORY,
            "extractor": "vanilla-save-region-v1-stored-sections",
        },
        "section_count": 3,
        "total_cells": 12288,
        "distinct_state_ids": 17,
        "cardinality_histogram": {"1": 2, "17": 1},
        "dimensions": {"minecraft:overworld": 3},
    }


def rust_import() -> dict[str, object]:
    candidates = []
    for name, production in evidence.EXPECTED_CANDIDATES.items():
        candidates.append(
            {
                "candidate": name,
                "production_candidate": production,
                "sections": 3,
                "total_owned_bytes": 300,
                "max_owned_bytes": 200,
                "construction_transitions": 2,
                "logical_backing_allocations": 4,
                "representations": {"uniform": 2, "direct-n": 1},
            }
        )
    return {
        "schema": 1,
        "kind": "section-corpus-import-check",
        "minecraft_version": "26.2",
        "protocol_version": 776,
        "data_version": 4903,
        "state_count": 32366,
        "state_data_generation_sha256": GENERATION,
        "state_data_input_sha256": INPUT,
        "source_inventory_sha256": INVENTORY,
        "extractor": "vanilla-save-region-v1-stored-sections",
        "purpose": "parser-admission",
        "decision_requested": False,
        "decision_eligible": False,
        "section_count": 3,
        "total_cells": 12288,
        "distinct_state_ids": 17,
        "cardinality_histogram": {"1": 2, "17": 1},
        "dimensions": {"minecraft:overworld": 3},
        "candidates": candidates,
    }


class SectionCorpusImportEvidenceTests(unittest.TestCase):
    def test_matching_evidence_passes(self) -> None:
        evidence.crosscheck(manifest(), rust_import())

    def test_target_identity_drift_fails(self) -> None:
        for key, replacement in [
            ("minecraft_version", "26.3"),
            ("protocol_version", 777),
            ("data_version", 4904),
            ("state_count", 1),
            ("state_data_generation_sha256", "a" * 64),
            ("state_data_input_sha256", "b" * 64),
        ]:
            with self.subTest(key=key):
                actual = rust_import()
                actual[key] = replacement
                with self.assertRaises(evidence.EvidenceError):
                    evidence.crosscheck(manifest(), actual)

    def test_source_and_purpose_drift_fail(self) -> None:
        for key, replacement in [
            ("source_inventory_sha256", "a" * 64),
            ("extractor", "future-policy-v9"),
            ("purpose", "decision"),
            ("decision_requested", True),
            ("decision_eligible", True),
        ]:
            with self.subTest(key=key):
                actual = rust_import()
                actual[key] = replacement
                with self.assertRaises(evidence.EvidenceError):
                    evidence.crosscheck(manifest(), actual)

    def test_corpus_summary_drift_fails(self) -> None:
        mutations: list[tuple[str, object]] = [
            ("section_count", 4),
            ("total_cells", 4096),
            ("distinct_state_ids", 18),
            ("cardinality_histogram", {"1": 3}),
            ("dimensions", {"minecraft:the_nether": 3}),
        ]
        for key, replacement in mutations:
            with self.subTest(key=key):
                actual = rust_import()
                actual[key] = replacement
                with self.assertRaises(evidence.EvidenceError):
                    evidence.crosscheck(manifest(), actual)

    def test_candidate_set_and_flags_are_exact(self) -> None:
        missing = rust_import()
        candidates = missing["candidates"]
        assert isinstance(candidates, list)
        candidates.pop()
        with self.assertRaises(evidence.EvidenceError):
            evidence.crosscheck(manifest(), missing)

        duplicate = rust_import()
        candidates = duplicate["candidates"]
        assert isinstance(candidates, list)
        candidates[-1] = copy.deepcopy(candidates[0])
        with self.assertRaises(evidence.EvidenceError):
            evidence.crosscheck(manifest(), duplicate)

        wrong_flag = rust_import()
        candidates = wrong_flag["candidates"]
        assert isinstance(candidates, list)
        candidate = candidates[1]
        assert isinstance(candidate, dict)
        candidate["production_candidate"] = False
        with self.assertRaises(evidence.EvidenceError):
            evidence.crosscheck(manifest(), wrong_flag)

    def test_candidate_section_and_representation_totals_must_match(self) -> None:
        wrong_sections = rust_import()
        candidates = wrong_sections["candidates"]
        assert isinstance(candidates, list)
        candidate = candidates[2]
        assert isinstance(candidate, dict)
        candidate["sections"] = 2
        with self.assertRaises(evidence.EvidenceError):
            evidence.crosscheck(manifest(), wrong_sections)

        wrong_representation_total = rust_import()
        candidates = wrong_representation_total["candidates"]
        assert isinstance(candidates, list)
        candidate = candidates[2]
        assert isinstance(candidate, dict)
        candidate["representations"] = {"uniform": 1}
        with self.assertRaises(evidence.EvidenceError):
            evidence.crosscheck(manifest(), wrong_representation_total)

        impossible_memory = rust_import()
        candidates = impossible_memory["candidates"]
        assert isinstance(candidates, list)
        candidate = candidates[2]
        assert isinstance(candidate, dict)
        candidate["total_owned_bytes"] = 100
        candidate["max_owned_bytes"] = 200
        with self.assertRaises(evidence.EvidenceError):
            evidence.crosscheck(manifest(), impossible_memory)


if __name__ == "__main__":
    unittest.main()
