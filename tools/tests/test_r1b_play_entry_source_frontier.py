from __future__ import annotations

import json
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
PLAY_ENTRY = REPO_ROOT / "vanilla/frontiers/r1b-play-entry-selected.json"
CONFIGURATION = REPO_ROOT / "vanilla/frontiers/r1b-configuration-selected.json"


class R1BPlayEntrySourceFrontierTests(unittest.TestCase):
    def setUp(self) -> None:
        self.play_entry = json.loads(PLAY_ENTRY.read_text(encoding="utf-8"))
        self.configuration = json.loads(CONFIGURATION.read_text(encoding="utf-8"))

    def test_frontier_is_unique_and_rooted_in_the_reviewed_configuration_handoff(self) -> None:
        roots = self.play_entry["root_queries"]
        self.assertEqual(len(roots), len(set(roots)))
        place_new_player = (
            "net.minecraft.server.players.PlayerList#placeNewPlayer(final Connection connection , "
            "final ServerPlayer player , final CommonListenerCookie cookie)"
        )
        self.assertIn(place_new_player, roots)
        self.assertIn(place_new_player, self.configuration["root_queries"])

    def test_direct_place_new_player_packet_types_are_discovery_anchors(self) -> None:
        joined = "\n".join(self.play_entry["root_queries"])
        for required in (
            "ClientboundLoginPacket",
            "ClientboundChangeDifficultyPacket",
            "ClientboundPlayerAbilitiesPacket",
            "ClientboundSetHeldSlotPacket",
            "ClientboundUpdateRecipesPacket",
            "ClientboundPlayerInfoUpdatePacket",
        ):
            self.assertIn(required, joined)

    def test_helper_driven_bootstrap_surfaces_are_explicit_discovery_anchors(self) -> None:
        joined = "\n".join(self.play_entry["root_queries"])
        for required in (
            "PlayerList#sendPlayerPermissionLevel",
            "PlayerList#updateEntireScoreboard",
            "PlayerList#sendLevelInfo",
            "PlayerList#sendActivePlayerEffects",
            "ServerGamePacketListenerImpl#teleport",
            "ServerPlayer#sendServerStatus",
            "ServerPlayer#initInventoryMenu",
            "ServerRecipeBook#sendInitialRecipeBook",
        ):
            self.assertIn(required, joined)

    def test_frontier_denies_packet_and_semantic_authority(self) -> None:
        description = self.play_entry["description"].lower()
        self.assertIn("discovery-only", description)
        self.assertIn("fresh-player", description)
        self.assertIn("not packet ids", description)
        self.assertIn("semantic", description)

    def test_frontier_does_not_expand_into_general_gameplay_or_chunk_work(self) -> None:
        roots = "\n".join(self.play_entry["root_queries"])
        for forbidden in (
            "ClientboundLevelChunkWithLightPacket",
            "ServerboundMovePlayerPacket",
            "ClientboundAddEntityPacket",
            "net.minecraft.network.protocol.game\"",
        ):
            self.assertNotIn(forbidden, roots)

        excluded = self.play_entry["exclude_package_prefixes"]
        for required in (
            "net.minecraft.network.protocol.configuration",
            "net.minecraft.network.protocol.handshake",
            "net.minecraft.network.protocol.login",
            "net.minecraft.network.protocol.status",
            "net.minecraft.world.level.chunk",
            "net.minecraft.world.level.levelgen",
        ):
            self.assertIn(required, excluded)

    def test_frontier_stays_small_enough_for_manual_review(self) -> None:
        self.assertEqual(self.play_entry["schema"], 1)
        self.assertLessEqual(self.play_entry["max_depth"], 4)
        self.assertLessEqual(len(self.play_entry["root_queries"]), 20)


if __name__ == "__main__":
    unittest.main()
