from __future__ import annotations

import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import section_representative_plan as plan


class RepresentativeSectionPlanTests(unittest.TestCase):
    def test_frozen_seed_derivation_and_plan_digest(self) -> None:
        self.assertEqual(
            plan.derive_seeds(),
            [
                3250117973538344636,
                -1983012757746938611,
                4735876718611431443,
                3964809773196812219,
            ],
        )
        built = plan.build_plan()
        self.assertEqual(
            built["plan_sha256"],
            "fecb9c9bc77aa9689ceaf6d88fa9af96019a48d9533269f3bd15824f7dfc7191",
        )
        self.assertEqual(plan.digest_plan(built), built["plan_sha256"])

    def test_each_dimension_has_exact_unique_content_independent_schedule(self) -> None:
        built = plan.build_plan()
        dimensions = built["dimensions"]
        self.assertIsInstance(dimensions, dict)
        for dimension in plan.DIMENSIONS:
            with self.subTest(dimension=dimension):
                entry = dimensions[dimension]
                chunks = entry["chunks"]
                self.assertEqual(len(chunks), plan.CHUNKS_PER_DIMENSION)
                self.assertEqual(len({tuple(chunk) for chunk in chunks}), len(chunks))
                self.assertEqual(chunks, plan.derive_chunks(dimension))

    def test_end_schedule_contains_central_and_outer_samples(self) -> None:
        chunks = [tuple(chunk) for chunk in plan.derive_chunks("minecraft:the_end")]
        self.assertIn((0, 0), chunks)
        self.assertIn((80, 0), chunks)
        self.assertIn((-80, 0), chunks)
        outer = [point for point in chunks if max(abs(point[0]), abs(point[1])) >= 80]
        self.assertGreaterEqual(len(outer), plan.CHUNKS_PER_DIMENSION - 5)

    def test_overworld_and_nether_hashed_samples_cover_multiple_quadrants(self) -> None:
        for dimension in ("minecraft:overworld", "minecraft:the_nether"):
            chunks = [tuple(chunk) for chunk in plan.derive_chunks(dimension)]
            quadrants = {
                (1 if x > 0 else -1 if x < 0 else 0, 1 if z > 0 else -1 if z < 0 else 0)
                for x, z in chunks
            }
            self.assertIn((1, 1), quadrants)
            self.assertIn((1, -1), quadrants)
            self.assertIn((-1, 1), quadrants)
            self.assertIn((-1, -1), quadrants)

    def test_plan_is_fail_closed_against_every_policy_identity_mutation(self) -> None:
        mutations = []
        for key, value in (
            ("schema", 2),
            ("policy", "other"),
            ("chunks_per_dimension", 63),
            ("plan_sha256", "0" * 64),
        ):
            mutated = copy.deepcopy(plan.build_plan())
            mutated[key] = value
            mutations.append(mutated)

        mutated = copy.deepcopy(plan.build_plan())
        mutated["target"]["data_version"] += 1
        mutations.append(mutated)

        mutated = copy.deepcopy(plan.build_plan())
        mutated["seeds"][0] += 1
        mutations.append(mutated)

        mutated = copy.deepcopy(plan.build_plan())
        mutated["dimensions"]["minecraft:overworld"]["chunks"][10][0] += 1
        mutations.append(mutated)

        mutated = copy.deepcopy(plan.build_plan())
        mutated["weighting"]["dimension"] = "equal"
        mutations.append(mutated)

        for index, mutated in enumerate(mutations):
            with self.subTest(index=index):
                with self.assertRaises(plan.PlanError):
                    plan.validate_plan(mutated)

    def test_write_load_roundtrip_is_exact(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "plan.json"
            plan.write_plan(path)
            raw = path.read_bytes()
            self.assertTrue(raw.endswith(b"\n"))
            self.assertFalse(raw.endswith(b"\n\n"))
            loaded = plan.load_plan(path)
            self.assertEqual(loaded, plan.build_plan())
            self.assertEqual(json.loads(raw), loaded)

    def test_unsupported_dimension_is_rejected(self) -> None:
        with self.assertRaises(plan.PlanError):
            plan.derive_chunks("minecraft:moon")


if __name__ == "__main__":
    unittest.main()
