from __future__ import annotations

import json
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
FRONTIER = REPO_ROOT / "vanilla/frontiers/r1a-login-policy.json"


class LoginPolicySourceFrontierTests(unittest.TestCase):
    def setUp(self) -> None:
        self.data = json.loads(FRONTIER.read_text(encoding="utf-8"))

    def test_frontier_is_narrow_login_only_review_input(self) -> None:
        self.assertEqual(self.data["schema"], 1)
        self.assertEqual(self.data["max_depth"], 8)
        roots = self.data["root_queries"]
        self.assertEqual(len(roots), len(set(roots)))
        joined = "\n".join(roots)
        for required in (
            "ClientIntent",
            "ServerHandshakePacketListenerImpl",
            "LoginPacketTypes",
            "LoginProtocols",
            "ServerboundHelloPacket",
            "ClientboundLoginFinishedPacket",
            "ServerboundLoginAcknowledgedPacket",
            "ServerLoginPacketListenerImpl",
        ):
            self.assertIn(required, joined)
        for forbidden in (
            "protocol.status",
            "protocol.configuration",
            "protocol.game",
            "ServerConfigurationPacketListenerImpl",
        ):
            self.assertNotIn(forbidden, joined)

    def test_frontier_preserves_network_only_boundary(self) -> None:
        self.assertIn("net.minecraft.network", self.data["include_package_prefixes"])
        self.assertIn("net.minecraft.server.network", self.data["include_package_prefixes"])
        self.assertNotIn("net.minecraft.world", self.data["include_package_prefixes"])
        self.assertIn(
            "net.minecraft.network.protocol.game",
            self.data["exclude_package_prefixes"],
        )

    def test_description_denies_finite_contract_authority(self) -> None:
        description = self.data["description"].lower()
        self.assertIn("discovery-only", description)
        self.assertIn("not a finite wire contract", description)


if __name__ == "__main__":
    unittest.main()
