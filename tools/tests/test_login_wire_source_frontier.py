from __future__ import annotations

import json
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
FRONTIER = REPO_ROOT / "vanilla/frontiers/r1a-login-wire.json"


class LoginWireSourceFrontierTests(unittest.TestCase):
    def setUp(self) -> None:
        self.data = json.loads(FRONTIER.read_text(encoding="utf-8"))

    def test_frontier_is_narrow_wire_review_input(self) -> None:
        self.assertEqual(self.data["schema"], 1)
        self.assertEqual(self.data["max_depth"], 4)
        roots = self.data["root_queries"]
        self.assertEqual(len(roots), len(set(roots)))
        joined = "\n".join(roots)
        for required in (
            "ServerboundHelloPacket",
            "ClientboundLoginFinishedPacket",
            "ServerboundLoginAcknowledgedPacket",
            "FriendlyByteBuf#readUUID",
            "FriendlyByteBuf#writeUUID",
            "FriendlyByteBuf#readNullable",
            "FriendlyByteBuf#writeNullable",
            "UUIDUtil#<clinit>()",
            "createOfflinePlayerUUID",
            "createOfflineProfile",
            "ByteBufCodecs#<clinit>()",
            "ByteBufCodecs#stringUtf8",
            "ByteBufCodecs#readCount",
            "ByteBufCodecs#writeCount",
            "StreamCodec",
        ):
            self.assertIn(required, joined)

    def test_frontier_excludes_later_protocol_phases(self) -> None:
        joined = "\n".join(self.data["root_queries"])
        for forbidden in (
            "ServerConfigurationPacketListenerImpl",
            "protocol.configuration",
            "protocol.game",
            "protocol.status",
        ):
            self.assertNotIn(forbidden, joined)
        excludes = self.data["exclude_package_prefixes"]
        self.assertIn("net.minecraft.network.protocol.configuration", excludes)
        self.assertIn("net.minecraft.network.protocol.game", excludes)
        self.assertIn("net.minecraft.network.protocol.status", excludes)

    def test_description_claims_only_finite_login_wire_scope(self) -> None:
        description = self.data["description"].lower()
        self.assertIn("finite login wire contract", description)
        self.assertIn("configuration and play remain out of scope", description)


if __name__ == "__main__":
    unittest.main()
