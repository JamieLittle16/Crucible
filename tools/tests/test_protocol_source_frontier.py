from __future__ import annotations

import json
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
FRONTIER = REPO_ROOT / "vanilla/frontiers/p0-protocol-client.json"


class ProtocolSourceFrontierTests(unittest.TestCase):
    def test_frontier_is_narrow_version_review_input(self) -> None:
        data = json.loads(FRONTIER.read_text(encoding="utf-8"))
        self.assertEqual(data["schema"], 1)
        self.assertEqual(data["max_depth"], 8)

        roots = data["root_queries"]
        self.assertGreaterEqual(len(roots), 10)
        self.assertEqual(len(roots), len(set(roots)))
        self.assertTrue(all(root.startswith("net.minecraft.") for root in roots))

        includes = data["include_package_prefixes"]
        self.assertIn("net.minecraft.network", includes)
        self.assertIn("net.minecraft.server.network", includes)
        self.assertNotIn("net.minecraft.world", includes)

        excludes = data["exclude_package_prefixes"]
        self.assertIn("net.minecraft.network.protocol.game", excludes)
        self.assertIn("net.minecraft.data", excludes)
        self.assertIn("net.minecraft.gametest", excludes)

    def test_frontier_covers_connection_and_pre_play_protocol_families(self) -> None:
        roots = json.loads(FRONTIER.read_text(encoding="utf-8"))["root_queries"]
        joined = "\n".join(roots)
        for required in (
            "net.minecraft.network.Connection",
            "protocol.handshake",
            "protocol.status",
            "protocol.login",
            "protocol.configuration",
            "ServerHandshakePacketListenerImpl",
            "ServerLoginPacketListenerImpl",
            "ServerConfigurationPacketListenerImpl",
        ):
            self.assertIn(required, joined)


if __name__ == "__main__":
    unittest.main()
