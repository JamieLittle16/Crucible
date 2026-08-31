from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_source_review_finalize as finalize
from tools import r2c_world_state_source_review_pack as packer


def pretty(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def source(identity: str, token: str) -> dict[str, str]:
    owner, signature = identity.split("#", 1)
    return {
        "type": owner,
        "signature": signature,
        "fingerprint_algorithm": "java-token-v2-literal-sensitive",
        "normalized_sha256": hashlib.sha256(f"norm:{token}".encode()).hexdigest(),
        "body_sha256": hashlib.sha256(f"body:{token}".encode()).hexdigest(),
    }


def candidate(identity: str, token: str, hazards: list[str]) -> dict[str, object]:
    return {
        "candidate_id": f"DISC-{token}",
        "source_identity": identity,
        "source": source(identity, token),
        "source_location": {
            "path": f"src/{token}.java",
            "start_line": 10,
            "end_line": 20,
        },
        "atlas_observed_hazards": hazards,
        "atlas_classifications": ["OBSERVABLE"],
        "calls": {
            "call_sites": 2,
            "resolved_targets": ["net.minecraft.Helper#work()"],
            "unresolved_call_sites": 1,
            "top_unresolved_callees": [{"callee": "helper/mystery/0", "sites": 1}],
        },
    }


BIOME = "net.minecraft.world.level.chunk.LevelChunkSection#getBiomes()"
SHARED = "net.minecraft.world.level.chunk.PalettedContainer#write(FriendlyByteBuf)"
HEIGHT = "net.minecraft.world.level.levelgen.Heightmap#update(int,int,int,BlockState)"
LIGHT = "net.minecraft.network.protocol.game.ClientboundLightUpdatePacketData#ClientboundLightUpdatePacketData()"


def fixture_values() -> tuple[dict[str, object], dict[str, object]]:
    candidates = {
        BIOME: candidate(BIOME, "biome", ["REGISTRY"]),
        SHARED: candidate(SHARED, "shared", ["CODEC"]),
        HEIGHT: candidate(HEIGHT, "height", []),
        LIGHT: candidate(LIGHT, "light", ["NETWORK_SEND"]),
    }
    memberships = {
        "R2C-BIOMES": [BIOME, SHARED],
        "R2C-HEIGHTMAPS": [HEIGHT, SHARED],
        "R2C-LIGHT": [LIGHT],
    }
    records = []
    for identity in sorted(candidates):
        group_ids = [group for group in packer.FOCUS_GROUPS if identity in memberships[group]]
        excerpt = f"source for {identity}\n"
        records.append(
            {
                **candidates[identity],
                "group_ids": group_ids,
                "source_excerpt": excerpt,
                "source_excerpt_sha256": hashlib.sha256(excerpt.encode()).hexdigest(),
            }
        )
    discovery_sha = hashlib.sha256(b"discovery").hexdigest()
    pack: dict[str, object] = {
        "schema": 1,
        "kind": packer.PACK_KIND,
        "review_id": packer.DISCOVERY_REVIEW_ID,
        "commit_policy": packer.PACK_COMMIT_POLICY,
        "contains_official_source_text": True,
        "source_archive_sha256": packer.EXPECTED_SOURCE_SHA256,
        "discovery_sha256": discovery_sha,
        "focused_groups": list(packer.FOCUS_GROUPS),
        "group_memberships": memberships,
        "source_records": records,
        "unique_source_records": len(records),
        "source_excerpt_bytes": sum(len(record["source_excerpt"].encode()) for record in records),
        "production_admitted": False,
    }
    pack_sha = hashlib.sha256(pretty(pack)).hexdigest()
    groups = []
    choices = {
        "R2C-BIOMES": ([BIOME], [SHARED], ["REGISTRY"]),
        "R2C-HEIGHTMAPS": ([HEIGHT, SHARED], [], ["CODEC"]),
        "R2C-LIGHT": ([LIGHT], [], ["NETWORK_SEND"]),
    }
    for group_id in packer.FOCUS_GROUPS:
        selected, rejected, hazards = choices[group_id]
        groups.append(
            {
                "group_id": group_id,
                "review_focus": f"review {group_id}",
                "candidates": [candidates[identity] for identity in memberships[group_id]],
                "source_inspected": True,
                "selected_source_identities": selected,
                "rejected_source_identities": rejected,
                "hazards_reviewed": hazards,
                "followup_dependencies": [],
                "semantic_observations": [f"Reviewed exact {group_id} source law."],
                "review_complete": True,
            }
        )
    worksheet: dict[str, object] = {
        "schema": 1,
        "kind": packer.WORKSHEET_KIND,
        "review_id": packer.DISCOVERY_REVIEW_ID,
        "commit_policy": packer.WORKSHEET_COMMIT_POLICY,
        "contains_official_source_text": False,
        "source_archive_sha256": packer.EXPECTED_SOURCE_SHA256,
        "discovery_sha256": discovery_sha,
        "review_pack_sha256": pack_sha,
        "groups": groups,
        "production_admitted": False,
    }
    return pack, worksheet


class R2cWorldStateSourceReviewFinalizeTests(unittest.TestCase):
    def write_fixture(self, root: Path) -> tuple[Path, Path, Path, dict[str, object], dict[str, object]]:
        pack, worksheet = fixture_values()
        pack_path = root / "review-pack.json"
        worksheet_path = root / "worksheet.json"
        output = root / "review-result.json"
        pack_path.write_bytes(pretty(pack))
        worksheet_path.write_bytes(pretty(worksheet))
        return pack_path, worksheet_path, output, pack, worksheet

    def test_complete_review_emits_source_free_commit_safe_result(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, _pack, _worksheet = self.write_fixture(Path(temporary))
            summary = finalize.finalize(pack_path, worksheet_path, output)
            result = json.loads(output.read_text())
            text = output.read_text()

            self.assertEqual(summary["groups"], 3)
            self.assertEqual(summary["selected_sources"], 4)
            self.assertFalse(summary["contains_official_source_text"])
            self.assertEqual(result["kind"], finalize.RESULT_KIND)
            self.assertEqual(result["id"], finalize.RESULT_ID)
            self.assertTrue(result["all_groups_review_complete"])
            self.assertFalse(result["production_admitted"])
            self.assertFalse(result["contains_official_source_text"])
            self.assertNotIn("source_excerpt", text)
            self.assertNotIn("source for net.minecraft", text)
            self.assertEqual([group["group_id"] for group in result["groups"]], list(packer.FOCUS_GROUPS))

    def test_tampered_pack_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, pack, _worksheet = self.write_fixture(Path(temporary))
            pack["source_excerpt_bytes"] = int(pack["source_excerpt_bytes"]) + 1
            pack_path.write_bytes(pretty(pack))
            with self.assertRaisesRegex(finalize.FinalizeError, "source_excerpt_bytes mismatch"):
                finalize.finalize(pack_path, worksheet_path, output)

    def test_structurally_valid_pack_digest_link_mismatch_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, _pack, worksheet = self.write_fixture(Path(temporary))
            worksheet["review_pack_sha256"] = "0" * 64
            worksheet_path.write_bytes(pretty(worksheet))
            with self.assertRaisesRegex(finalize.FinalizeError, "review_pack_sha256"):
                finalize.finalize(pack_path, worksheet_path, output)

    def test_source_pin_drift_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, pack, _worksheet = self.write_fixture(Path(temporary))
            pack["source_archive_sha256"] = "0" * 64
            pack_path.write_bytes(pretty(pack))
            with self.assertRaisesRegex(finalize.FinalizeError, "review pack identity mismatch"):
                finalize.finalize(pack_path, worksheet_path, output)

    def test_incomplete_review_flags_fail(self) -> None:
        for field in ("source_inspected", "review_complete"):
            with self.subTest(field=field), tempfile.TemporaryDirectory() as temporary:
                pack_path, worksheet_path, output, _pack, worksheet = self.write_fixture(Path(temporary))
                worksheet["groups"][0][field] = False
                worksheet_path.write_bytes(pretty(worksheet))
                with self.assertRaisesRegex(finalize.FinalizeError, field):
                    finalize.finalize(pack_path, worksheet_path, output)

    def test_unresolved_followup_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, _pack, worksheet = self.write_fixture(Path(temporary))
            worksheet["groups"][1]["followup_dependencies"] = ["inspect delegate"]
            worksheet_path.write_bytes(pretty(worksheet))
            with self.assertRaisesRegex(finalize.FinalizeError, "unresolved followup"):
                finalize.finalize(pack_path, worksheet_path, output)

    def test_selection_must_be_exact_disjoint_partition(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, _pack, worksheet = self.write_fixture(Path(temporary))
            worksheet["groups"][0]["rejected_source_identities"] = []
            worksheet_path.write_bytes(pretty(worksheet))
            with self.assertRaisesRegex(finalize.FinalizeError, "exactly partition"):
                finalize.finalize(pack_path, worksheet_path, output)

        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, _pack, worksheet = self.write_fixture(Path(temporary))
            worksheet["groups"][0]["rejected_source_identities"] = [BIOME, SHARED]
            worksheet_path.write_bytes(pretty(worksheet))
            with self.assertRaisesRegex(finalize.FinalizeError, "overlap"):
                finalize.finalize(pack_path, worksheet_path, output)

    def test_every_group_requires_selected_source(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, _pack, worksheet = self.write_fixture(Path(temporary))
            worksheet["groups"][2]["selected_source_identities"] = []
            worksheet["groups"][2]["rejected_source_identities"] = [LIGHT]
            worksheet_path.write_bytes(pretty(worksheet))
            with self.assertRaisesRegex(finalize.FinalizeError, "must not be empty"):
                finalize.finalize(pack_path, worksheet_path, output)

    def test_selected_hazards_must_be_fully_reviewed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, _pack, worksheet = self.write_fixture(Path(temporary))
            worksheet["groups"][2]["hazards_reviewed"] = []
            worksheet_path.write_bytes(pretty(worksheet))
            with self.assertRaisesRegex(finalize.FinalizeError, "hazards are not fully reviewed"):
                finalize.finalize(pack_path, worksheet_path, output)

    def test_semantic_observation_is_required(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, _pack, worksheet = self.write_fixture(Path(temporary))
            worksheet["groups"][1]["semantic_observations"] = []
            worksheet_path.write_bytes(pretty(worksheet))
            with self.assertRaisesRegex(finalize.FinalizeError, "must not be empty"):
                finalize.finalize(pack_path, worksheet_path, output)

    def test_candidate_metadata_drift_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, _pack, worksheet = self.write_fixture(Path(temporary))
            worksheet["groups"][0]["candidates"][0]["calls"]["call_sites"] = 3
            worksheet_path.write_bytes(pretty(worksheet))
            with self.assertRaisesRegex(finalize.FinalizeError, "candidate metadata drift"):
                finalize.finalize(pack_path, worksheet_path, output)

    def test_corrupted_source_excerpt_digest_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack_path, worksheet_path, output, pack, worksheet = self.write_fixture(Path(temporary))
            pack["source_records"][0]["source_excerpt_sha256"] = "0" * 64
            pack_path.write_bytes(pretty(pack))
            worksheet["review_pack_sha256"] = hashlib.sha256(pretty(pack)).hexdigest()
            worksheet_path.write_bytes(pretty(worksheet))
            with self.assertRaisesRegex(finalize.FinalizeError, "source excerpt digest mismatch"):
                finalize.finalize(pack_path, worksheet_path, output)

    def test_output_refuses_overwrite_and_input_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack_path, worksheet_path, output, _pack, _worksheet = self.write_fixture(root)
            output.write_text("existing")
            with self.assertRaisesRegex(finalize.FinalizeError, "must not already exist"):
                finalize.finalize(pack_path, worksheet_path, output)

            linked = root / "linked-pack.json"
            linked.symlink_to(pack_path)
            fresh = root / "fresh-result.json"
            with self.assertRaisesRegex(finalize.FinalizeError, "non-symlink"):
                finalize.finalize(linked, worksheet_path, fresh)

    def test_serialization_is_deterministic(self) -> None:
        value = {"b": [2, 1], "a": {"z": False}}
        self.assertEqual(finalize._pretty_bytes(value), finalize._pretty_bytes(value))
        self.assertEqual(
            finalize._sha256_bytes(finalize._pretty_bytes(value)),
            finalize._sha256_bytes(finalize._pretty_bytes(value)),
        )


if __name__ == "__main__":
    unittest.main()
