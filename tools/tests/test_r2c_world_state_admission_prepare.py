from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_admission_prepare as prepare
from tools import r2c_world_state_source_review_finalize as review


def pretty(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def candidate(candidate_id: str, identity: str, token: str) -> dict[str, object]:
    owner, signature = identity.split("#", 1)
    return {
        "candidate_id": candidate_id,
        "source_identity": identity,
        "source": {
            "type": owner,
            "signature": signature,
            "fingerprint_algorithm": "java-token-v2-literal-sensitive",
            "normalized_sha256": hashlib.sha256(f"norm:{token}".encode()).hexdigest(),
            "body_sha256": hashlib.sha256(f"body:{token}".encode()).hexdigest(),
        },
        "source_location": {
            "path": f"src/{token}.java",
            "start_line": 10,
            "end_line": 20,
        },
        "atlas_observed_hazards": ["CODEC"],
        "atlas_classifications": ["OBSERVABLE"],
        "calls": {
            "call_sites": 1,
            "resolved_targets": [],
            "unresolved_call_sites": 0,
            "top_unresolved_callees": [],
        },
    }


BIOME = "net.minecraft.world.level.chunk.LevelChunkSection#getBiomes()"
SHARED = "net.minecraft.world.level.chunk.PalettedContainer#write(FriendlyByteBuf)"
HEIGHT = "net.minecraft.world.level.levelgen.Heightmap#update(int,int,int,BlockState)"
LIGHT = "net.minecraft.network.protocol.game.ClientboundLightUpdatePacketData#ClientboundLightUpdatePacketData()"


def review_result() -> dict[str, object]:
    biome = candidate("DISC-NET-R2C-WORLD-0001", BIOME, "biome")
    shared_biomes = candidate("DISC-NET-R2C-WORLD-0002", SHARED, "shared")
    shared_heightmaps = candidate("DISC-NET-R2C-WORLD-0002", SHARED, "shared")
    height = candidate("DISC-NET-R2C-WORLD-0003", HEIGHT, "height")
    light = candidate("DISC-NET-R2C-WORLD-0004", LIGHT, "light")
    groups = [
        {
            "group_id": "R2C-BIOMES",
            "selected_sources": [biome, shared_biomes],
            "rejected_source_identities": [],
            "hazards_reviewed": ["CODEC"],
            "semantic_observations": ["reviewed biome law"],
        },
        {
            "group_id": "R2C-HEIGHTMAPS",
            "selected_sources": [shared_heightmaps, height],
            "rejected_source_identities": [],
            "hazards_reviewed": ["CODEC"],
            "semantic_observations": ["reviewed height law"],
        },
        {
            "group_id": "R2C-LIGHT",
            "selected_sources": [light],
            "rejected_source_identities": [],
            "hazards_reviewed": ["CODEC"],
            "semantic_observations": ["reviewed light law"],
        },
    ]
    return {
        "schema": 1,
        "kind": review.RESULT_KIND,
        "id": review.RESULT_ID,
        "commit_policy": review.RESULT_COMMIT_POLICY,
        "source_archive_sha256": review.EXPECTED_SOURCE_SHA256,
        "discovery_sha256": hashlib.sha256(b"discovery").hexdigest(),
        "review_pack_sha256": hashlib.sha256(b"pack").hexdigest(),
        "worksheet_sha256": hashlib.sha256(b"worksheet").hexdigest(),
        "contains_official_source_text": False,
        "all_groups_review_complete": True,
        "groups": groups,
        "production_admitted": False,
        "next_step": "fixture",
    }


class R2cWorldStateAdmissionPrepareTests(unittest.TestCase):
    def write_fixture(self, root: Path) -> tuple[Path, Path, dict[str, object]]:
        value = review_result()
        source = root / "review-result.json"
        output = root / "admission-worksheet.json"
        source.write_bytes(pretty(value))
        return source, output, value

    def test_prepare_freezes_sources_but_infers_no_semantic_rules(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, output, _value = self.write_fixture(Path(temporary))
            summary = prepare.prepare(source, output)
            worksheet = json.loads(output.read_text())
            text = output.read_text()

            self.assertEqual(summary["groups"], 3)
            self.assertEqual(summary["selected_sources"], 4)
            self.assertFalse(summary["production_admitted"])
            self.assertFalse(worksheet["contains_official_source_text"])
            self.assertFalse(worksheet["production_admitted"])
            self.assertFalse(worksheet["all_groups_admission_complete"])
            self.assertTrue(
                worksheet["semantic_rule_contract"]["automatic_semantic_inference_forbidden"]
            )
            self.assertTrue(all(group["semantic_rules"] == [] for group in worksheet["groups"]))
            self.assertTrue(all(group["admission_complete"] is False for group in worksheet["groups"]))
            self.assertNotIn("source_excerpt", text)

    def test_shared_selected_source_is_deduplicated_and_var_ids_are_stable(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, output, _value = self.write_fixture(Path(temporary))
            prepare.prepare(source, output)
            worksheet = json.loads(output.read_text())
            sources = worksheet["selected_sources"]
            self.assertEqual(len(sources), 4)
            by_identity = {item["source_identity"]: item for item in sources}
            self.assertEqual(by_identity[BIOME]["var_id"], "VAR-NET-R2C-WORLD-0001")
            self.assertEqual(by_identity[SHARED]["var_id"], "VAR-NET-R2C-WORLD-0002")
            self.assertEqual(by_identity[HEIGHT]["var_id"], "VAR-NET-R2C-WORLD-0003")
            self.assertEqual(by_identity[LIGHT]["var_id"], "VAR-NET-R2C-WORLD-0004")

    def test_review_result_identity_drift_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, output, value = self.write_fixture(Path(temporary))
            value["production_admitted"] = True
            source.write_bytes(pretty(value))
            with self.assertRaisesRegex(prepare.PrepareError, "identity mismatch"):
                prepare.prepare(source, output)

    def test_noncanonical_candidate_id_fails_instead_of_guessing_var_id(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, output, value = self.write_fixture(Path(temporary))
            value["groups"][0]["selected_sources"][0]["candidate_id"] = "BIOME-METHOD"
            source.write_bytes(pretty(value))
            with self.assertRaisesRegex(prepare.PrepareError, "cannot derive stable VAR id"):
                prepare.prepare(source, output)

    def test_shared_source_metadata_drift_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, output, value = self.write_fixture(Path(temporary))
            value["groups"][1]["selected_sources"][0]["source"]["body_sha256"] = "0" * 64
            source.write_bytes(pretty(value))
            with self.assertRaisesRegex(
                prepare.PrepareError, "selected source metadata differs across groups"
            ):
                prepare.prepare(source, output)

    def test_duplicate_candidate_suffix_cannot_create_duplicate_var_id(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, output, value = self.write_fixture(Path(temporary))
            value["groups"][2]["selected_sources"][0]["candidate_id"] = "DISC-NET-R2C-WORLD-0001"
            source.write_bytes(pretty(value))
            with self.assertRaisesRegex(prepare.PrepareError, "derived duplicate VAR id"):
                prepare.prepare(source, output)

    def test_empty_selected_group_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, output, value = self.write_fixture(Path(temporary))
            value["groups"][2]["selected_sources"] = []
            source.write_bytes(pretty(value))
            with self.assertRaisesRegex(prepare.PrepareError, "has no selected sources"):
                prepare.prepare(source, output)

    def test_output_refuses_overwrite_and_input_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, output, _value = self.write_fixture(root)
            output.write_text("existing")
            with self.assertRaisesRegex(prepare.PrepareError, "must not already exist"):
                prepare.prepare(source, output)

            linked = root / "linked-review.json"
            linked.symlink_to(source)
            fresh = root / "fresh.json"
            with self.assertRaisesRegex(prepare.PrepareError, "non-symlink"):
                prepare.prepare(linked, fresh)

    def test_serialization_is_deterministic(self) -> None:
        value = {"z": [3, 1], "a": False}
        self.assertEqual(prepare._pretty_bytes(value), prepare._pretty_bytes(value))
        self.assertEqual(
            prepare._sha256_bytes(prepare._pretty_bytes(value)),
            prepare._sha256_bytes(prepare._pretty_bytes(value)),
        )


if __name__ == "__main__":
    unittest.main()
