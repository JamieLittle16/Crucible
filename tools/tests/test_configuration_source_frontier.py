from __future__ import annotations

import json
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CONFIG_FRONTIER = REPO_ROOT / "vanilla/frontiers/p0-configuration.json"
P0_FRONTIER = REPO_ROOT / "vanilla/frontiers/p0-protocol-client.json"


class ConfigurationSourceFrontierTests(unittest.TestCase):
    def setUp(self) -> None:
        self.configuration = json.loads(CONFIG_FRONTIER.read_text(encoding="utf-8"))
        self.p0 = json.loads(P0_FRONTIER.read_text(encoding="utf-8"))

    def test_configuration_roots_are_a_strict_subset_of_the_broader_p0_frontier(self) -> None:
        configuration_roots = self.configuration["root_queries"]
        p0_roots = self.p0["root_queries"]
        self.assertEqual(len(configuration_roots), len(set(configuration_roots)))
        self.assertTrue(set(configuration_roots) < set(p0_roots))

    def test_configuration_roots_cover_only_entry_finish_and_listener_anchors(self) -> None:
        joined = "\n".join(self.configuration["root_queries"])
        for required in (
            "net.minecraft.network.Connection",
            "net.minecraft.network.ProtocolInfo",
            "protocol.login.ClientboundLoginFinishedPacket",
            "protocol.configuration.ServerboundFinishConfigurationPacket",
            "protocol.configuration.ClientboundFinishConfigurationPacket",
            "ServerConfigurationPacketListenerImpl",
        ):
            self.assertIn(required, joined)
        for forbidden in (
            "protocol.status",
            "protocol.handshake",
            "protocol.game",
            "ServerHandshakePacketListenerImpl",
            "ServerLoginPacketListenerImpl",
            "ServerboundHelloPacket",
        ):
            self.assertNotIn(forbidden, joined)

    def test_configuration_preserves_the_network_only_discovery_boundary(self) -> None:
        self.assertEqual(self.configuration["schema"], 1)
        self.assertEqual(self.configuration["max_depth"], self.p0["max_depth"])
        self.assertEqual(
            self.configuration["include_package_prefixes"],
            self.p0["include_package_prefixes"],
        )
        self.assertEqual(
            self.configuration["exclude_package_prefixes"],
            self.p0["exclude_package_prefixes"],
        )
        self.assertNotIn(
            "net.minecraft.world", self.configuration["include_package_prefixes"]
        )
        self.assertIn(
            "net.minecraft.network.protocol.game",
            self.configuration["exclude_package_prefixes"],
        )

    def test_frontier_description_denies_semantic_and_registry_authority(self) -> None:
        description = self.configuration["description"].lower()
        self.assertIn("discovery-only", description)
        self.assertIn("not packet ids", description)
        self.assertIn("registry payloads", description)
        self.assertIn("semantic", description)


if __name__ == "__main__":
    unittest.main()
