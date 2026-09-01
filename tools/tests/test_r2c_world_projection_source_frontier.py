from __future__ import annotations

import json
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
FRONTIER = REPO_ROOT / "vanilla/frontiers/r2c-world-projection.json"
PLAN = REPO_ROOT / "vanilla/reviews/network/r2c-world-projection-discovery-plan.json"
EXPECTED_GROUPS = [
    "R2C-WORLD-ENTRY",
    "R2C-CHUNK-SPAN",
    "R2C-BLOCK-SECTIONS",
    "R2C-BIOMES",
    "R2C-HEIGHTMAPS",
    "R2C-LIGHT",
    "R2C-BLOCK-ENTITIES",
    "R2C-PACING",
    "R2C-PACKET-IDS",
]


class R2cWorldProjectionSourceFrontierTests(unittest.TestCase):
    def test_frontier_freezes_selected_26_2_profile_without_admitting_wire_law(self) -> None:
        data = json.loads(FRONTIER.read_text(encoding="utf-8"))
        self.assertEqual(data["schema"], 1)
        self.assertEqual(
            data["target"],
            {
                "minecraft_version": "26.2",
                "protocol_version": 776,
                "world_version": 4903,
            },
        )
        self.assertEqual(data["semantic_groups"], EXPECTED_GROUPS)
        self.assertTrue(data["selected_profile"]["pregenerated_world"])
        self.assertTrue(data["selected_profile"]["initial_spawn_observation_only"])
        self.assertFalse(data["selected_profile"]["world_generation_required"])
        self.assertFalse(data["selected_profile"]["movement_interest_required"])
        self.assertFalse(data["selected_profile"]["captured_world_publication_allowed"])

    def test_frontier_roots_are_unique_and_bounded_to_world_projection(self) -> None:
        data = json.loads(FRONTIER.read_text(encoding="utf-8"))
        roots = data["root_queries"]
        self.assertEqual(len(roots), 25)
        self.assertEqual(len(roots), len(set(roots)))
        self.assertTrue(all(root.startswith("net.minecraft.") for root in roots))
        joined = "\n".join(roots)
        for required in (
            "PlayerChunkSender",
            "ClientboundLevelChunkWithLightPacket",
            "ClientboundLevelChunkPacketData",
            "ClientboundLightUpdatePacketData",
            "LevelChunkSection",
            "PalettedContainer",
            "PalettedContainer$Data",
            "PalettedContainer$Strategy",
            "PalettedContainer$Configuration",
            "PalettedContainerFactory",
            "SingleValuePalette",
            "LinearPalette",
            "HashMapPalette",
            "GlobalPalette",
            "SimpleBitStorage",
            "Heightmap",
            "Heightmap$Types",
            "Heightmap$Usage",
            "DataLayer",
            "LayerLightEventListener",
            "LevelLightEngine",
            "SerializableChunkData",
        ):
            self.assertIn(required, joined)
        for excluded in ("ServerGamePacketListenerImpl", "ServerPlayerGameMode", "WorldGenRegion"):
            self.assertNotIn(excluded, joined)

    def test_discovery_plan_covers_every_frontier_root_and_group(self) -> None:
        frontier = json.loads(FRONTIER.read_text(encoding="utf-8"))
        plan = json.loads(PLAN.read_text(encoding="utf-8"))
        self.assertEqual(plan["schema"], 1)
        self.assertEqual(
            plan["id"],
            "REVIEW-NET-R2C-WORLD-PROJECTION-DISCOVERY-26_2-001",
        )
        self.assertEqual(plan["frontier"], "vanilla/frontiers/r2c-world-projection.json")
        self.assertEqual([group["group_id"] for group in plan["groups"]], EXPECTED_GROUPS)
        planned_roots = {
            root
            for group in plan["groups"]
            for root in group["root_types"]
        }
        self.assertEqual(planned_roots, set(frontier["root_queries"]))
        groups = {group["group_id"]: group for group in plan["groups"]}
        self.assertIn(
            "net.minecraft.world.level.chunk.PalettedContainer$Data",
            groups["R2C-BIOMES"]["root_types"],
        )
        self.assertIn(
            "net.minecraft.world.level.levelgen.Heightmap$Types",
            groups["R2C-HEIGHTMAPS"]["root_types"],
        )
        self.assertIn(
            "net.minecraft.network.protocol.game.ClientboundLevelChunkPacketData",
            groups["R2C-HEIGHTMAPS"]["root_types"],
        )
        self.assertIn(
            "net.minecraft.world.level.chunk.DataLayer",
            groups["R2C-LIGHT"]["root_types"],
        )
        self.assertIn(
            "net.minecraft.world.level.chunk.storage.SerializableChunkData",
            groups["R2C-LIGHT"]["root_types"],
        )
        for group in plan["groups"]:
            self.assertTrue(group["review_focus"])
            self.assertTrue(group["root_types"])
            self.assertEqual(len(group["root_types"]), len(set(group["root_types"])))


if __name__ == "__main__":
    unittest.main()
