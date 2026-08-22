from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import section_corpus_set as corpus_set
import section_representative_plan as plan

SERVER_SHA = "c" * 64
GENERATION_SHA = "g" * 64
INPUT_SHA = "i" * 64


def state_manifest() -> dict[str, object]:
    return {
        "target": {
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "data_version": 4903,
        },
        "state_count": 32366,
        "generation_digest": GENERATION_SHA,
        "input_digest": INPUT_SHA,
    }


def selected_chunks(built: dict[str, object]) -> dict[str, list[list[int]]]:
    dimensions = built["dimensions"]
    return {
        dimension: sorted([list(chunk) for chunk in dimensions[dimension]["chunks"]])
        for dimension in plan.DIMENSIONS
    }


def candidate_rows(section_count: int) -> list[dict[str, object]]:
    rows = []
    for index, (name, production) in enumerate(corpus_set.EXPECTED_CANDIDATES.items()):
        rows.append(
            {
                "candidate": name,
                "production_candidate": production,
                "sections": section_count,
                "total_owned_bytes": section_count * (index + 1),
                "max_owned_bytes": index + 1,
                "construction_transitions": index,
                "logical_backing_allocations": index + 2,
                "representations": {f"repr-{index}": section_count},
            }
        )
    return rows


def member_inputs(built: dict[str, object]) -> list[tuple[dict, dict, dict, dict]]:
    target = {
        "minecraft_version": "26.2",
        "protocol_version": 776,
        "data_version": 4903,
        "state_count": 32366,
        "state_data_generation_sha256": GENERATION_SHA,
        "state_data_input_sha256": INPUT_SHA,
    }
    lattice = {
        "minecraft:overworld": [-4, -3],
        "minecraft:the_nether": [0],
        "minecraft:the_end": [0],
    }
    dimensions = {
        dimension: plan.CHUNKS_PER_DIMENSION * len(lattice[dimension])
        for dimension in plan.DIMENSIONS
    }
    section_count = sum(dimensions.values())
    cardinality = {"1": section_count - 1, "2": 1}
    result = []
    for seed_index, seed in enumerate(built["seeds"]):
        inventory = f"{seed_index + 1:064x}"
        corpus_sha = f"{seed_index + 100:064x}"
        world = {
            "schema": 1,
            "generator": "official-server-representative-section-world-v1",
            "minecraft_version": "26.2",
            "server_sha256": SERVER_SHA,
            "representative_policy": built["policy"],
            "plan_sha256": built["plan_sha256"],
            "seed_index": seed_index,
            "seed": seed,
            "command_count": len(plan.DIMENSIONS) * plan.CHUNKS_PER_DIMENSION,
            "command_sha256": "d" * 64,
        }
        extraction = {
            "schema": 1,
            "policy": corpus_set.MEMBER_EXTRACTOR,
            "representative_policy": built["policy"],
            "plan_sha256": built["plan_sha256"],
            "seed_index": seed_index,
            "seed": seed,
            "inventory_sha256": inventory,
            "selected_chunks": selected_chunks(built),
            "section_lattice": copy.deepcopy(lattice),
            "corpus_sha256": corpus_sha,
            "section_count": section_count,
            "distinct_state_ids": 2,
        }
        manifest = {
            "schema": 1,
            "format": "CRUCIBLE-SECTION-CORPUS/1",
            "target": copy.deepcopy(target),
            "source": {
                "kind": "vanilla-save",
                "inventory_sha256": inventory,
                "extractor": corpus_set.MEMBER_EXTRACTOR,
            },
            "corpus_sha256": corpus_sha,
            "section_count": section_count,
            "total_cells": section_count * 4096,
            "distinct_state_ids": 2,
            "dimensions": copy.deepcopy(dimensions),
            "cardinality_histogram": copy.deepcopy(cardinality),
            "cell_facts": {
                "non_air": 1,
                "counted_fluid": 0,
                "random_block": 0,
                "random_fluid": 0,
            },
            "section_classes": {
                "all_air": section_count - 1,
                "contains_fluid": 0,
                "random_block_present": 0,
                "random_fluid_present": 0,
            },
        }
        rust = {
            "schema": 1,
            "kind": "section-corpus-import-check",
            **copy.deepcopy(target),
            "source_inventory_sha256": inventory,
            "extractor": corpus_set.MEMBER_EXTRACTOR,
            "purpose": corpus_set.MEMBER_PURPOSE,
            "decision_requested": False,
            "decision_eligible": False,
            "section_count": section_count,
            "total_cells": section_count * 4096,
            "distinct_state_ids": 2,
            "dimensions": copy.deepcopy(dimensions),
            "cardinality_histogram": copy.deepcopy(cardinality),
            "candidates": candidate_rows(section_count),
        }
        result.append((world, extraction, manifest, rust))
    return result


class RepresentativeCorpusSetTests(unittest.TestCase):
    def build(self, inputs=None):
        built = plan.build_plan()
        if inputs is None:
            inputs = member_inputs(built)
        return corpus_set.build_set(
            plan=built,
            state_manifest=state_manifest(),
            pinned_server_sha256=SERVER_SHA,
            member_inputs=inputs,
        )

    def test_complete_population_is_decision_eligible_and_deterministic(self) -> None:
        result = self.build()
        self.assertTrue(result["decision_eligible"])
        self.assertEqual(result["member_count"], 4)
        self.assertEqual(result["policy"], plan.POLICY_ID)
        self.assertEqual(result["plan_sha256"], plan.build_plan()["plan_sha256"])
        self.assertEqual(len(result["set_sha256"]), 64)
        self.assertEqual(result["set_sha256"], corpus_set._canonical_digest(result))
        self.assertEqual(
            result["aggregate"]["section_count"],
            sum(member["section_count"] for member in result["members"]),
        )

    def test_missing_member_is_rejected(self) -> None:
        built = plan.build_plan()
        inputs = member_inputs(built)[:-1]
        with self.assertRaises(corpus_set.CorpusSetError):
            self.build(inputs)

    def test_wrong_seed_or_plan_digest_is_rejected(self) -> None:
        built = plan.build_plan()
        for mutate in ("seed", "plan"):
            inputs = member_inputs(built)
            if mutate == "seed":
                inputs[2][0]["seed"] += 1
            else:
                inputs[2][1]["plan_sha256"] = "0" * 64
            with self.subTest(mutate=mutate), self.assertRaises(corpus_set.CorpusSetError):
                self.build(inputs)

    def test_wrong_selected_chunk_schedule_is_rejected(self) -> None:
        built = plan.build_plan()
        inputs = member_inputs(built)
        inputs[1][1]["selected_chunks"]["minecraft:overworld"][0][0] += 1
        with self.assertRaises(corpus_set.CorpusSetError):
            self.build(inputs)

    def test_cross_seed_lattice_drift_is_rejected(self) -> None:
        built = plan.build_plan()
        inputs = member_inputs(built)
        extraction = inputs[3][1]
        extraction["section_lattice"]["minecraft:the_end"] = [0, 1]
        # Keep this member internally self-consistent so the failure is genuinely cross-seed.
        extraction["section_count"] += plan.CHUNKS_PER_DIMENSION
        manifest = inputs[3][2]
        rust = inputs[3][3]
        manifest["dimensions"]["minecraft:the_end"] += plan.CHUNKS_PER_DIMENSION
        manifest["section_count"] += plan.CHUNKS_PER_DIMENSION
        manifest["total_cells"] = manifest["section_count"] * 4096
        manifest["cardinality_histogram"]["1"] += plan.CHUNKS_PER_DIMENSION
        manifest["section_classes"]["all_air"] += plan.CHUNKS_PER_DIMENSION
        rust["dimensions"]["minecraft:the_end"] += plan.CHUNKS_PER_DIMENSION
        rust["section_count"] += plan.CHUNKS_PER_DIMENSION
        rust["total_cells"] = rust["section_count"] * 4096
        rust["cardinality_histogram"]["1"] += plan.CHUNKS_PER_DIMENSION
        for row in rust["candidates"]:
            row["sections"] = rust["section_count"]
            only = next(iter(row["representations"]))
            row["representations"][only] = rust["section_count"]
        with self.assertRaises(corpus_set.CorpusSetError):
            self.build(inputs)

    def test_python_rust_histogram_disagreement_is_rejected(self) -> None:
        built = plan.build_plan()
        inputs = member_inputs(built)
        inputs[0][3]["cardinality_histogram"] = {"1": inputs[0][3]["section_count"]}
        with self.assertRaises(corpus_set.CorpusSetError):
            self.build(inputs)

    def test_member_cannot_claim_individual_decision_eligibility(self) -> None:
        built = plan.build_plan()
        inputs = member_inputs(built)
        inputs[0][3]["decision_eligible"] = True
        with self.assertRaises(corpus_set.CorpusSetError):
            self.build(inputs)

    def test_wrong_official_server_identity_is_rejected(self) -> None:
        built = plan.build_plan()
        inputs = member_inputs(built)
        inputs[0][0]["server_sha256"] = "f" * 64
        with self.assertRaises(corpus_set.CorpusSetError):
            self.build(inputs)

    def test_candidate_set_and_representation_totals_are_strict(self) -> None:
        built = plan.build_plan()
        inputs = member_inputs(built)
        inputs[0][3]["candidates"][0]["candidate"] = "unknown"
        with self.assertRaises(corpus_set.CorpusSetError):
            self.build(inputs)

        inputs = member_inputs(built)
        inputs[0][3]["candidates"][1]["representations"] = {"direct-n": 1}
        with self.assertRaises(corpus_set.CorpusSetError):
            self.build(inputs)


if __name__ == "__main__":
    unittest.main()
