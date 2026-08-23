from __future__ import annotations

import json
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
LOGIN_FRONTIER = REPO_ROOT / "vanilla/frontiers/p0-login.json"
P0_FRONTIER = REPO_ROOT / "vanilla/frontiers/p0-protocol-client.json"


class LoginSourceFrontierTests(unittest.TestCase):
    def setUp(self) -> None:
        self.login = json.loads(LOGIN_FRONTIER.read_text(encoding="utf-8"))
        self.p0 = json.loads(P0_FRONTIER.read_text(encoding="utf-8"))

    def test_login_roots_are_a_strict_subset_of_the_broader_p0_frontier(self) -> None:
        login_roots = self.login["root_queries"]
        p0_roots = self.p0["root_queries"]
        self.assertEqual(len(login_roots), len(set(login_roots)))
        self.assertTrue(set(login_roots) < set(p0_roots))

    def test_login_roots_cover_only_handoff_and_login_anchors(self) -> None:
        joined = "\n".join(self.login["root_queries"])
        for required in (
            "net.minecraft.network.Connection",
            "net.minecraft.network.ProtocolInfo",
            "protocol.handshake.ClientIntentionPacket",
            "ServerHandshakePacketListenerImpl",
            "protocol.login.ServerboundHelloPacket",
            "protocol.login.ClientboundLoginFinishedPacket",
            "ServerLoginPacketListenerImpl",
        ):
            self.assertIn(required, joined)
        for forbidden in (
            "protocol.status",
            "protocol.configuration",
            "protocol.game",
            "ServerConfigurationPacketListenerImpl",
            "ServerboundStatusRequestPacket",
        ):
            self.assertNotIn(forbidden, joined)

    def test_login_preserves_the_network_only_discovery_boundary(self) -> None:
        self.assertEqual(self.login["schema"], 1)
        self.assertEqual(self.login["max_depth"], self.p0["max_depth"])
        self.assertEqual(
            self.login["include_package_prefixes"], self.p0["include_package_prefixes"]
        )
        self.assertEqual(
            self.login["exclude_package_prefixes"], self.p0["exclude_package_prefixes"]
        )
        self.assertNotIn("net.minecraft.world", self.login["include_package_prefixes"])
        self.assertIn(
            "net.minecraft.network.protocol.game",
            self.login["exclude_package_prefixes"],
        )

    def test_frontier_description_denies_semantic_and_policy_authority(self) -> None:
        description = self.login["description"].lower()
        self.assertIn("discovery-only", description)
        self.assertIn("not packet ids", description)
        self.assertIn("not", description)
        self.assertIn("semantic", description)
        self.assertIn("authentication policy", description)


if __name__ == "__main__":
    unittest.main()
