from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

from tools import r2c_world_state_source_review_pack as pack


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def candidate(
    candidate_id: str,
    identity: str,
    path: str,
    start: int,
    end: int,
) -> dict[str, object]:
    owner, signature = identity.split("#", 1)
    return {
        "candidate_id": candidate_id,
        "source_identity": identity,
        "source": {
            "type": owner,
            "signature": signature,
            "fingerprint_algorithm": "java-token-v2-literal-sensitive",
            "normalized_sha256": f"norm-{candidate_id}",
            "body_sha256": f"body-{candidate_id}",
        },
        "source_location": {"path": path, "start_line": start, "end_line": end},
        "atlas_observed_hazards": ["CODEC"],
        "atlas_classifications": ["OBSERVABLE"],
        "calls": {
            "call_sites": 1,
            "resolved_targets": [],
            "unresolved_call_sites": 0,
            "top_unresolved_callees": [],
        },
    }


def discovery(source_sha: str) -> dict[str, object]:
    shared = candidate(
        "DISC-1",
        "net.minecraft.world.level.chunk.LevelChunkSection#write(FriendlyByteBuf)",
        "src/net/minecraft/world/level/chunk/LevelChunkSection.java",
        2,
        4,
    )
    height = candidate(
        "DISC-2",
        "net.minecraft.world.level.levelgen.Heightmap#update(int,int,int,BlockState)",
        "src/net/minecraft/world/level/levelgen/Heightmap.java",
        1,
        3,
    )
    light = candidate(
        "DISC-3",
        "net.minecraft.network.protocol.game.ClientboundLightUpdatePacketData#ClientboundLightUpdatePacketData()",
        "src/net/minecraft/network/protocol/game/ClientboundLightUpdatePacketData.java",
        3,
        5,
    )
    return {
        "schema": 1,
        "kind": pack.DISCOVERY_KIND,
        "review_id": pack.DISCOVERY_REVIEW_ID,
        "source_text_included": False,
        "production_admitted": False,
        "source": {
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "world_version": 4903,
            "archive_sha256": source_sha,
        },
        "groups": [
            {
                "group_id": "R2C-BIOMES",
                "review_focus": "biome fixture",
                "candidate_methods": [shared],
                "production_admitted": False,
            },
            {
                "group_id": "R2C-HEIGHTMAPS",
                "review_focus": "height fixture",
                "candidate_methods": [shared, height],
                "production_admitted": False,
            },
            {
                "group_id": "R2C-LIGHT",
                "review_focus": "light fixture",
                "candidate_methods": [light],
                "production_admitted": False,
            },
        ],
    }


def write_lock(path: Path, source_sha: str) -> None:
    path.write_text(
        "\n".join(
            (
                "schema = 1",
                'minecraft = "26.2"',
                "protocol = 776",
                "data_version = 4903",
                "",
                "[source]",
                f'archive_sha256 = "{source_sha}"',
                "",
            )
        ),
        encoding="utf-8",
    )


def write_source(path: Path) -> None:
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(
            "src/net/minecraft/world/level/chunk/LevelChunkSection.java",
            "line1\nline2\nline3\nline4\nline5\n",
        )
        archive.writestr(
            "src/net/minecraft/world/level/levelgen/Heightmap.java",
            "height1\nheight2\nheight3\nheight4\n",
        )
        archive.writestr(
            "src/net/minecraft/network/protocol/game/ClientboundLightUpdatePacketData.java",
            "light1\nlight2\nlight3\nlight4\nlight5\nlight6\n",
        )


class R2cWorldStateSourceReviewPackTests(unittest.TestCase):
    def build_fixture(self, root: Path) -> tuple[Path, Path, Path, str]:
        source = root / "mc-src.zip"
        write_source(source)
        source_sha = sha256(source)
        discovery_path = root / "discovery.json"
        discovery_path.write_text(json.dumps(discovery(source_sha)), encoding="utf-8")
        lock = root / "vanilla.lock.toml"
        write_lock(lock, source_sha)
        return source, discovery_path, lock, source_sha

    def test_build_separates_source_rich_pack_from_source_free_worksheet(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, discovery_path, lock, source_sha = self.build_fixture(root)
            output = root / "out"
            with mock.patch.object(pack, "EXPECTED_SOURCE_SHA256", source_sha):
                result = pack.build(discovery_path, source, lock, output)

            self.assertEqual(result["unique_source_records"], 3)
            review_pack = json.loads((output / "review-pack.json").read_text(encoding="utf-8"))
            worksheet = json.loads((output / "worksheet.json").read_text(encoding="utf-8"))
            manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))

            self.assertTrue(review_pack["contains_official_source_text"])
            self.assertEqual(review_pack["commit_policy"], "EPHEMERAL_DO_NOT_COMMIT")
            self.assertFalse(review_pack["production_admitted"])
            self.assertEqual(len(review_pack["source_records"]), 3)
            shared = next(
                record
                for record in review_pack["source_records"]
                if record["source_identity"].startswith("net.minecraft.world.level.chunk.LevelChunkSection#")
            )
            self.assertEqual(shared["group_ids"], ["R2C-BIOMES", "R2C-HEIGHTMAPS"])
            self.assertEqual(shared["source_excerpt"], "line2\nline3\nline4\n")
            self.assertEqual(
                shared["source_excerpt_sha256"],
                hashlib.sha256(b"line2\nline3\nline4\n").hexdigest(),
            )

            worksheet_text = (output / "worksheet.json").read_text(encoding="utf-8")
            self.assertFalse(worksheet["contains_official_source_text"])
            self.assertFalse(worksheet["production_admitted"])
            self.assertNotIn("source_excerpt", worksheet_text)
            self.assertNotIn("line2", worksheet_text)
            self.assertEqual([group["group_id"] for group in worksheet["groups"]], list(pack.FOCUS_GROUPS))
            self.assertTrue(all(group["source_inspected"] is False for group in worksheet["groups"]))
            self.assertEqual(manifest["files"][0]["source_rich"], True)
            self.assertEqual(manifest["files"][1]["source_rich"], False)

    def test_source_archive_pin_mismatch_fails_before_output_creation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, discovery_path, lock, source_sha = self.build_fixture(root)
            with zipfile.ZipFile(source, "a") as archive:
                archive.writestr("extra.txt", "changed")
            output = root / "out"
            with mock.patch.object(pack, "EXPECTED_SOURCE_SHA256", source_sha):
                with self.assertRaisesRegex(pack.ReviewPackError, "source archive SHA-256 mismatch"):
                    pack.build(discovery_path, source, lock, output)
            self.assertFalse(output.exists())

    def test_missing_source_member_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, discovery_path, lock, source_sha = self.build_fixture(root)
            value = json.loads(discovery_path.read_text(encoding="utf-8"))
            value["groups"][0]["candidate_methods"][0]["source_location"]["path"] = "src/Missing.java"
            value["groups"][1]["candidate_methods"][0]["source_location"]["path"] = "src/Missing.java"
            discovery_path.write_text(json.dumps(value), encoding="utf-8")
            output = root / "out"
            with mock.patch.object(pack, "EXPECTED_SOURCE_SHA256", source_sha):
                with self.assertRaisesRegex(pack.ReviewPackError, "source archive member is missing"):
                    pack.build(discovery_path, source, lock, output)
            self.assertFalse(output.exists())

    def test_unsafe_and_invalid_candidate_ranges_are_rejected(self) -> None:
        base = candidate("DISC-X", "net.minecraft.A#x()", "../A.java", 1, 1)
        with self.assertRaisesRegex(pack.ReviewPackError, "unsafe candidate source path"):
            pack._source_free_candidate(base)

        base["source_location"] = {"path": "/tmp/A.java", "start_line": 1, "end_line": 1}
        with self.assertRaisesRegex(pack.ReviewPackError, "unsafe candidate source path"):
            pack._source_free_candidate(base)

        base["source_location"] = {"path": "src/A.java", "start_line": 0, "end_line": 1}
        with self.assertRaisesRegex(pack.ReviewPackError, "line range is invalid"):
            pack._source_free_candidate(base)

        base["source_location"] = {
            "path": "src/A.java",
            "start_line": 1,
            "end_line": pack.MAX_CANDIDATE_LINES + 1,
        }
        with self.assertRaisesRegex(pack.ReviewPackError, "exceeds review bound"):
            pack._source_free_candidate(base)

    def test_discovery_admission_or_group_drift_is_rejected(self) -> None:
        value = discovery(pack.EXPECTED_SOURCE_SHA256)
        value["production_admitted"] = True
        with self.assertRaisesRegex(pack.ReviewPackError, "source-free and non-admitted"):
            pack._validate_discovery(value)

        value = discovery(pack.EXPECTED_SOURCE_SHA256)
        value["groups"] = value["groups"][:-1]
        with self.assertRaisesRegex(pack.ReviewPackError, "missing focused groups"):
            pack._validate_discovery(value)

        value = discovery(pack.EXPECTED_SOURCE_SHA256)
        value["groups"][0]["production_admitted"] = True
        with self.assertRaisesRegex(pack.ReviewPackError, "unexpectedly admitted"):
            pack._validate_discovery(value)

    def test_shared_identity_metadata_drift_is_rejected(self) -> None:
        value = discovery(pack.EXPECTED_SOURCE_SHA256)
        value["groups"][1]["candidate_methods"][0] = dict(
            value["groups"][1]["candidate_methods"][0]
        )
        value["groups"][1]["candidate_methods"][0]["calls"] = {"call_sites": 99}
        groups, _ = pack._validate_discovery(value)
        with self.assertRaisesRegex(pack.ReviewPackError, "inconsistent discovery metadata"):
            pack._focused_candidates(groups)

    def test_external_output_policy_and_serialization_are_deterministic(self) -> None:
        with self.assertRaisesRegex(pack.ReviewPackError, "outside the repository"):
            pack._fresh_external_dir(pack.REPO_ROOT / "target/r2c-world-state-review-new")
        value = {"z": [3, 2, 1], "a": False}
        self.assertEqual(pack._pretty_bytes(value), pack._pretty_bytes(value))
        self.assertEqual(
            pack._sha256_bytes(pack._pretty_bytes(value)),
            pack._sha256_bytes(pack._pretty_bytes(value)),
        )


if __name__ == "__main__":
    unittest.main()
