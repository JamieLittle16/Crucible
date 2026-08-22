from __future__ import annotations

import copy
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
TESTS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
sys.path.insert(0, str(TESTS))

import official_representative_section_world as worldgen
import section_corpus_set
import section_population_admission as admission
import section_representative_plan as plan
import test_section_corpus_set as fixtures


class PopulationAdmissionTests(unittest.TestCase):
    def materialize(self):
        built = plan.build_plan()
        inputs = fixtures.member_inputs(built)
        for seed_index, seed in enumerate(built["seeds"]):
            inputs[seed_index][0]["server_properties"] = worldgen.server_properties(
                int(seed)
            ).splitlines()
            manifest = inputs[seed_index][2]
            per_dimension = manifest["per_dimension"]
            manifest["cell_facts"] = {
                key: sum(summary["cell_facts"][key] for summary in per_dimension.values())
                for key in admission.CELL_FACT_KEYS
            }
            manifest["section_classes"] = {
                key: sum(
                    summary["section_classes"][key]
                    for summary in per_dimension.values()
                )
                for key in admission.SECTION_CLASS_KEYS
            }

        raw_set = section_corpus_set.build_set(
            plan=built,
            state_manifest=fixtures.state_manifest(),
            pinned_server_sha256=fixtures.SERVER_SHA,
            member_inputs=inputs,
        )
        temporary = tempfile.TemporaryDirectory()
        root = Path(temporary.name)
        for seed_index, (world, _extraction, manifest, _rust) in enumerate(inputs):
            directory = root / f"seed-{seed_index}"
            directory.mkdir()
            (directory / "world-evidence.json").write_text(
                json.dumps(world, sort_keys=True) + "\n", encoding="utf-8"
            )
            (directory / "corpus-manifest.json").write_text(
                json.dumps(manifest, sort_keys=True) + "\n", encoding="utf-8"
            )
        set_bytes = (json.dumps(raw_set, sort_keys=True) + "\n").encode()
        return temporary, root, built, raw_set, hashlib.sha256(set_bytes).hexdigest()

    def test_complete_population_requires_second_independent_admission(self) -> None:
        temporary, root, built, raw_set, set_sha = self.materialize()
        self.addCleanup(temporary.cleanup)
        result = admission.build_admission(
            plan=built,
            set_record=raw_set,
            members_root=root,
            set_file_sha256=set_sha,
        )
        self.assertTrue(result["decision_eligible"])
        self.assertTrue(result["benchmark_handoff_eligible"])
        self.assertEqual(result["member_count"], 4)
        self.assertEqual(result["decision_scope"], "dimension-separated-only")
        self.assertFalse(result["cross_dimension_score_allowed"])
        self.assertEqual(set(result["per_dimension"]), set(plan.DIMENSIONS))
        self.assertEqual(len(result["admission_sha256"]), 64)
        for member in result["members"]:
            self.assertEqual(len(member["server_properties_sha256"]), 64)
        for summary in result["per_dimension"].values():
            self.assertEqual(set(summary["cell_facts"]), admission.CELL_FACT_KEYS)
            self.assertEqual(
                set(summary["section_classes"]), admission.SECTION_CLASS_KEYS
            )

    def test_terrain_affecting_server_property_drift_is_rejected(self) -> None:
        temporary, root, built, raw_set, set_sha = self.materialize()
        self.addCleanup(temporary.cleanup)
        path = root / "seed-2" / "world-evidence.json"
        world = json.loads(path.read_text())
        index = world["server_properties"].index("generate-structures=true")
        world["server_properties"][index] = "generate-structures=false"
        path.write_text(json.dumps(world) + "\n")
        with self.assertRaises(admission.AdmissionError):
            admission.build_admission(
                plan=built,
                set_record=raw_set,
                members_root=root,
                set_file_sha256=set_sha,
            )

    def test_missing_or_reordered_server_property_contract_is_rejected(self) -> None:
        for mode in ("missing", "reordered"):
            temporary, root, built, raw_set, set_sha = self.materialize()
            self.addCleanup(temporary.cleanup)
            path = root / "seed-1" / "world-evidence.json"
            world = json.loads(path.read_text())
            if mode == "missing":
                world["server_properties"].pop()
            else:
                world["server_properties"][0], world["server_properties"][1] = (
                    world["server_properties"][1],
                    world["server_properties"][0],
                )
            path.write_text(json.dumps(world) + "\n")
            with self.subTest(mode=mode), self.assertRaises(admission.AdmissionError):
                admission.build_admission(
                    plan=built,
                    set_record=raw_set,
                    members_root=root,
                    set_file_sha256=set_sha,
                )

    def test_global_semantic_summary_must_recompose_from_dimensions(self) -> None:
        temporary, root, built, raw_set, set_sha = self.materialize()
        self.addCleanup(temporary.cleanup)
        path = root / "seed-0" / "corpus-manifest.json"
        manifest = json.loads(path.read_text())
        manifest["cell_facts"]["non_air"] += 1
        path.write_text(json.dumps(manifest) + "\n")
        with self.assertRaises(admission.AdmissionError):
            admission.build_admission(
                plan=built,
                set_record=raw_set,
                members_root=root,
                set_file_sha256=set_sha,
            )

    def test_global_section_classes_must_recompose_from_dimensions(self) -> None:
        temporary, root, built, raw_set, set_sha = self.materialize()
        self.addCleanup(temporary.cleanup)
        path = root / "seed-0" / "corpus-manifest.json"
        manifest = json.loads(path.read_text())
        manifest["section_classes"]["all_air"] += 1
        path.write_text(json.dumps(manifest) + "\n")
        with self.assertRaises(admission.AdmissionError):
            admission.build_admission(
                plan=built,
                set_record=raw_set,
                members_root=root,
                set_file_sha256=set_sha,
            )

    def test_semantic_keysets_and_bounds_are_fail_closed(self) -> None:
        mutations = ("extra-key", "cell-overflow", "class-overflow")
        for mutation in mutations:
            temporary, root, built, raw_set, set_sha = self.materialize()
            self.addCleanup(temporary.cleanup)
            path = root / "seed-3" / "corpus-manifest.json"
            manifest = json.loads(path.read_text())
            if mutation == "extra-key":
                manifest["cell_facts"]["invented"] = 0
            elif mutation == "cell-overflow":
                manifest["cell_facts"]["non_air"] = manifest["total_cells"] + 1
            else:
                manifest["section_classes"]["all_air"] = manifest["section_count"] + 1
            path.write_text(json.dumps(manifest) + "\n")
            with self.subTest(mutation=mutation), self.assertRaises(
                admission.AdmissionError
            ):
                admission.build_admission(
                    plan=built,
                    set_record=raw_set,
                    members_root=root,
                    set_file_sha256=set_sha,
                )

    def test_semantic_subset_relations_are_enforced(self) -> None:
        temporary, root, built, raw_set, set_sha = self.materialize()
        self.addCleanup(temporary.cleanup)
        path = root / "seed-0" / "corpus-manifest.json"
        manifest = json.loads(path.read_text())
        overworld = manifest["per_dimension"]["minecraft:overworld"]
        overworld["cell_facts"]["random_fluid"] = (
            overworld["cell_facts"]["counted_fluid"] + 1
        )
        manifest["cell_facts"]["random_fluid"] += 1
        path.write_text(json.dumps(manifest) + "\n")
        with self.assertRaises(admission.AdmissionError):
            admission.build_admission(
                plan=built,
                set_record=raw_set,
                members_root=root,
                set_file_sha256=set_sha,
            )

    def test_structural_set_flags_cannot_be_weakened(self) -> None:
        for key, value in (
            ("decision_eligible", False),
            ("cross_dimension_score_allowed", True),
            ("decision_scope", "global"),
        ):
            temporary, root, built, raw_set, set_sha = self.materialize()
            self.addCleanup(temporary.cleanup)
            changed = copy.deepcopy(raw_set)
            changed[key] = value
            with self.subTest(key=key), self.assertRaises(admission.AdmissionError):
                admission.build_admission(
                    plan=built,
                    set_record=changed,
                    members_root=root,
                    set_file_sha256=set_sha,
                )

    def test_raw_set_evidence_digest_is_reverified(self) -> None:
        temporary, root, built, raw_set, set_sha = self.materialize()
        self.addCleanup(temporary.cleanup)
        changed = copy.deepcopy(raw_set)
        changed["aggregate"]["section_count"] += 1
        with self.assertRaises(admission.AdmissionError):
            admission.build_admission(
                plan=built,
                set_record=changed,
                members_root=root,
                set_file_sha256=set_sha,
            )


if __name__ == "__main__":
    unittest.main()
