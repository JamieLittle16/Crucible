from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_admission_materialize as materialize
from tools import r2c_world_state_admission_prepare as prepare
from tools import r2c_world_state_source_review_finalize as review
from tools import vanilla_source_gate


REPO_ROOT = Path(__file__).resolve().parents[2]


def candidate(candidate_id: str, owner: str, signature: str, hazard: str) -> dict[str, object]:
    identity = f"{owner}#{signature}"
    digest_seed = candidate_id.removeprefix("DISC-NET-R2C-WORLD-") or "1"
    digit = int(digest_seed) % 16
    return {
        "candidate_id": candidate_id,
        "source_identity": identity,
        "source": {
            "type": owner,
            "signature": signature,
            "fingerprint_algorithm": "java-token-v2-literal-sensitive",
            "normalized_sha256": f"{digit:x}" * 64,
            "body_sha256": f"{(digit + 1) % 16:x}" * 64,
        },
        "source_location": {"path": f"src/{owner.replace('.', '/')}.java", "start_line": 10, "end_line": 20},
        "atlas_observed_hazards": [hazard],
        "atlas_classifications": ["OBSERVABLE"],
        "calls": {
            "call_sites": 1,
            "resolved_targets": [],
            "unresolved_call_sites": 0,
            "top_unresolved_callees": [],
        },
    }


def review_result() -> dict[str, object]:
    shared = candidate(
        "DISC-NET-R2C-WORLD-0001",
        "net.minecraft.world.level.chunk.LevelChunkSection",
        "write(FriendlyByteBuf)",
        "CODEC",
    )
    biome_only = candidate(
        "DISC-NET-R2C-WORLD-0002",
        "net.minecraft.world.level.chunk.PalettedContainer",
        "write(FriendlyByteBuf)",
        "CODEC",
    )
    light = candidate(
        "DISC-NET-R2C-WORLD-0003",
        "net.minecraft.network.protocol.game.ClientboundLightUpdatePacketData",
        "ClientboundLightUpdatePacketData()",
        "NETWORK_SEND",
    )
    return {
        "schema": 1,
        "kind": review.RESULT_KIND,
        "id": review.RESULT_ID,
        "commit_policy": review.RESULT_COMMIT_POLICY,
        "source_archive_sha256": review.EXPECTED_SOURCE_SHA256,
        "discovery_sha256": "1" * 64,
        "review_pack_sha256": "2" * 64,
        "worksheet_sha256": "3" * 64,
        "contains_official_source_text": False,
        "all_groups_review_complete": True,
        "groups": [
            {
                "group_id": "R2C-BIOMES",
                "selected_sources": [shared, biome_only],
                "rejected_source_identities": [],
                "hazards_reviewed": ["CODEC"],
                "semantic_observations": ["reviewed biome semantics"],
            },
            {
                "group_id": "R2C-HEIGHTMAPS",
                "selected_sources": [shared],
                "rejected_source_identities": [],
                "hazards_reviewed": ["CODEC"],
                "semantic_observations": ["reviewed height semantics"],
            },
            {
                "group_id": "R2C-LIGHT",
                "selected_sources": [light],
                "rejected_source_identities": [],
                "hazards_reviewed": ["NETWORK_SEND"],
                "semantic_observations": ["reviewed light semantics"],
            },
        ],
        "production_admitted": False,
        "next_step": "fixture",
    }


def author_complete_worksheet(path: Path) -> None:
    value = json.loads(path.read_text(encoding="utf-8"))
    rules = {
        "R2C-BIOMES": {
            "id": "SEM-NET-R2C-WORLD-001",
            "statement": "Biome section projection follows the reviewed semantic rule fixture.",
            "source_identities": value["groups"][0]["selected_source_identities"],
        },
        "R2C-HEIGHTMAPS": {
            "id": "SEM-NET-R2C-WORLD-002",
            "statement": "Heightmap projection follows the reviewed semantic rule fixture.",
            "source_identities": value["groups"][1]["selected_source_identities"],
        },
        "R2C-LIGHT": {
            "id": "SEM-NET-R2C-WORLD-003",
            "statement": "Light projection follows the reviewed semantic rule fixture.",
            "source_identities": value["groups"][2]["selected_source_identities"],
        },
    }
    for group in value["groups"]:
        group["semantic_rules"] = [rules[group["group_id"]]]
        group["admission_complete"] = True
    value["all_groups_admission_complete"] = True
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


class R2cWorldStateAdmissionMaterializeTests(unittest.TestCase):
    def fixture(self, root: Path) -> tuple[Path, Path]:
        review_path = root / "review-result.json"
        review_path.write_text(json.dumps(review_result(), indent=2, sort_keys=True) + "\n", encoding="utf-8")
        worksheet = root / "admission.json"
        prepare.prepare(review_path, worksheet)
        author_complete_worksheet(worksheet)
        return review_path, worksheet

    def test_materializes_source_free_records_semantics_and_valid_gate(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            review_path, worksheet = self.fixture(root)
            output = root / "staged"
            summary = materialize.materialize(review_path, worksheet, output)

            self.assertEqual(summary["var_records"], 3)
            self.assertEqual(summary["semantic_rules"], 3)
            self.assertFalse(summary["production_admitted"])
            self.assertFalse(summary["contains_official_source_text"])

            gate, _raw = vanilla_source_gate.load_gate(output / "gate.json")
            self.assertEqual(gate["id"], materialize.GATE_ID)
            self.assertEqual(gate["frontier"], materialize.FRONTIER)
            self.assertEqual(gate["minimum_status"], "VAR_REVIEWED")
            self.assertTrue(gate["require_semantic_rules"])
            self.assertTrue(gate["require_hazards_reviewed"])
            self.assertEqual(len(gate["methods"]), 3)

            records = sorted((output / "records").glob("*.json"))
            self.assertEqual(len(records), 3)
            decoded = [json.loads(path.read_text(encoding="utf-8")) for path in records]
            self.assertTrue(all(record["status"] == "VAR_REVIEWED" for record in decoded))
            self.assertTrue(all(record["semantic_rules"] for record in decoded))
            self.assertTrue(all(record["hazards_reviewed"] for record in decoded))

            semantics = (output / "semantics" / materialize.SEMANTICS_FILE).read_text(encoding="utf-8")
            self.assertIn("SEM-NET-R2C-WORLD-001", semantics)
            self.assertIn("SEM-NET-R2C-WORLD-002", semantics)
            self.assertIn("SEM-NET-R2C-WORLD-003", semantics)
            self.assertNotIn("source_excerpt", semantics)

            manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            self.assertTrue(manifest["independent_gate_required"])
            self.assertFalse(manifest["production_admitted"])
            self.assertEqual(manifest["var_records"], 3)
            self.assertEqual(manifest["semantic_rules"], 3)

    def test_incomplete_group_fails_before_output_creation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            review_path, worksheet = self.fixture(root)
            value = json.loads(worksheet.read_text(encoding="utf-8"))
            value["groups"][1]["admission_complete"] = False
            value["all_groups_admission_complete"] = False
            worksheet.write_text(json.dumps(value), encoding="utf-8")
            output = root / "staged"
            with self.assertRaisesRegex(materialize.MaterializeError, "completion mismatch"):
                materialize.materialize(review_path, worksheet, output)
            self.assertFalse(output.exists())

    def test_rule_cannot_cite_source_from_another_group(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            review_path, worksheet = self.fixture(root)
            value = json.loads(worksheet.read_text(encoding="utf-8"))
            light_identity = value["groups"][2]["selected_source_identities"][0]
            value["groups"][0]["semantic_rules"][0]["source_identities"] = [light_identity]
            worksheet.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(materialize.MaterializeError, "not selected for this group"):
                materialize.materialize(review_path, worksheet, root / "staged")

    def test_every_selected_source_must_support_a_semantic_rule(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            review_path, worksheet = self.fixture(root)
            value = json.loads(worksheet.read_text(encoding="utf-8"))
            value["groups"][0]["semantic_rules"][0]["source_identities"] = [
                value["groups"][0]["selected_source_identities"][0]
            ]
            worksheet.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(materialize.MaterializeError, "every selected source"):
                materialize.materialize(review_path, worksheet, root / "staged")

    def test_duplicate_semantic_rule_id_across_groups_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            review_path, worksheet = self.fixture(root)
            value = json.loads(worksheet.read_text(encoding="utf-8"))
            value["groups"][1]["semantic_rules"][0]["id"] = value["groups"][0]["semantic_rules"][0]["id"]
            worksheet.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(materialize.MaterializeError, "duplicate semantic rule id"):
                materialize.materialize(review_path, worksheet, root / "staged")

    def test_selected_source_metadata_must_match_completed_review(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            review_path, worksheet = self.fixture(root)
            value = json.loads(worksheet.read_text(encoding="utf-8"))
            value["selected_sources"][0]["candidate"]["source"]["body_sha256"] = "f" * 64
            worksheet.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(materialize.MaterializeError, "exact reviewed source"):
                materialize.materialize(review_path, worksheet, root / "staged")

    def test_review_result_missing_hazard_closure_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            review_path, worksheet = self.fixture(root)
            value = json.loads(review_path.read_text(encoding="utf-8"))
            value["groups"][2]["hazards_reviewed"] = []
            review_path.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(materialize.MaterializeError, "missing reviewed hazards"):
                materialize.materialize(review_path, worksheet, root / "staged")

    def test_output_policy_rejects_repository_and_existing_paths(self) -> None:
        with self.assertRaisesRegex(materialize.MaterializeError, "outside the repository"):
            materialize._fresh_external_dir(REPO_ROOT / "target/r2c-world-state-admission")
        with tempfile.TemporaryDirectory() as temporary:
            existing = Path(temporary) / "existing"
            existing.mkdir()
            with self.assertRaisesRegex(materialize.MaterializeError, "must not already exist"):
                materialize._fresh_external_dir(existing)

    def test_pretty_serialization_is_deterministic(self) -> None:
        value = {"z": [3, 2, 1], "a": False}
        first = materialize._pretty_bytes(value)
        second = materialize._pretty_bytes(value)
        self.assertEqual(first, second)
        self.assertEqual(materialize._sha256_bytes(first), materialize._sha256_bytes(second))
        self.assertTrue(first.endswith(b"\n"))


if __name__ == "__main__":
    unittest.main()
