from __future__ import annotations

import json
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
R0_FRONTIER = REPO_ROOT / "vanilla/frontiers/r0-status.json"
P0_FRONTIER = REPO_ROOT / "vanilla/frontiers/p0-protocol-client.json"


class R0StatusSourceFrontierTests(unittest.TestCase):
    def setUp(self) -> None:
        self.r0 = json.loads(R0_FRONTIER.read_text(encoding="utf-8"))
        self.p0 = json.loads(P0_FRONTIER.read_text(encoding="utf-8"))

    def test_r0_roots_are_a_strict_subset_of_the_broader_p0_frontier(self) -> None:
        r0_roots = self.r0["root_queries"]
        p0_roots = self.p0["root_queries"]
        self.assertEqual(len(r0_roots), len(set(r0_roots)))
        self.assertTrue(set(r0_roots) < set(p0_roots))

    def test_r0_roots_cover_only_connection_handshake_and_status_anchors(self) -> None:
        joined = "\n".join(self.r0["root_queries"])
        for required in (
            "net.minecraft.network.Connection",
            "net.minecraft.network.ProtocolInfo",
            "protocol.handshake.ClientIntentionPacket",
            "ServerHandshakePacketListenerImpl",
            "ServerboundStatusRequestPacket",
            "ClientboundStatusResponsePacket",
            "ServerboundPingRequestPacket",
            "ClientboundPongResponsePacket",
        ):
            self.assertIn(required, joined)
        for forbidden in (
            "protocol.login",
            "protocol.configuration",
            "protocol.game",
            "ServerLoginPacketListenerImpl",
            "ServerConfigurationPacketListenerImpl",
        ):
            self.assertNotIn(forbidden, joined)

    def test_r0_preserves_the_network_only_discovery_boundary(self) -> None:
        self.assertEqual(self.r0["schema"], 1)
        self.assertEqual(self.r0["max_depth"], self.p0["max_depth"])
        self.assertEqual(
            self.r0["include_package_prefixes"], self.p0["include_package_prefixes"]
        )
        self.assertEqual(
            self.r0["exclude_package_prefixes"], self.p0["exclude_package_prefixes"]
        )
        self.assertNotIn("net.minecraft.world", self.r0["include_package_prefixes"])
        self.assertIn(
            "net.minecraft.network.protocol.game",
            self.r0["exclude_package_prefixes"],
        )

    def test_frontier_description_denies_semantic_authority(self) -> None:
        description = self.r0["description"].lower()
        self.assertIn("discovery-only", description)
        self.assertIn("not packet ids", description)
        self.assertIn("not", description)
        self.assertIn("semantic", description)


if __name__ == "__main__":
    unittest.main()
