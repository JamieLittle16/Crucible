from __future__ import annotations

import unittest

from tools import r2b_play_entry_gate_probe as probe


class R2BPlayEntryGateProbeTests(unittest.TestCase):
    def test_probe_is_source_free_and_pin_specific(self) -> None:
        self.assertEqual(probe.KIND, "r2b-play-entry-gate-atlas-probe")
        self.assertEqual(probe.EXPECTED_META["minecraft_version"], "26.2")
        self.assertEqual(probe.EXPECTED_META["protocol_version"], "776")
        self.assertEqual(
            probe.EXPECTED_META["source_archive_sha256"],
            "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750",
        )

    def test_inventory_delegate_seams_are_explicit(self) -> None:
        self.assertIn(
            "net.minecraft.server.level.ServerPlayer#initMenu(final AbstractContainerMenu container)",
            probe.ANCHORS,
        )
        self.assertIn(
            "net.minecraft.world.inventory.AbstractContainerMenu#sendAllDataToRemote()",
            probe.ANCHORS,
        )
        for required in (
            "setSynchronizer",
            "sendInitialData",
            "sendSlotChange",
            "sendCarriedChange",
            "sendDataChange",
        ):
            self.assertIn(required, probe.SYMBOL_NAMES)

    def test_packet_scope_is_bootstrap_only_and_contains_inventory_content(self) -> None:
        self.assertIn(
            "net.minecraft.network.protocol.game.ClientboundContainerSetContentPacket",
            probe.PACKET_TYPES,
        )
        joined = "\n".join(probe.PACKET_TYPES)
        for forbidden in (
            "ClientboundLevelChunkWithLightPacket",
            "ClientboundAddEntityPacket",
            "ServerboundMovePlayerPacket",
        ):
            self.assertNotIn(forbidden, joined)

    def test_packet_codec_delegate_surface_contains_known_mandatory_bootstrap_types(self) -> None:
        joined = "\n".join(probe.PACKET_TYPES)
        for required in (
            "ClientboundLoginPacket",
            "ClientboundPlayerAbilitiesPacket",
            "ClientboundCommandsPacket",
            "ClientboundPlayerInfoUpdatePacket",
            "ClientboundPlayerPositionPacket",
            "ClientboundUpdateRecipesPacket",
        ):
            self.assertIn(required, joined)

    def test_probe_scope_remains_bounded(self) -> None:
        self.assertLessEqual(len(probe.ANCHORS), 4)
        self.assertLessEqual(len(probe.SYMBOL_NAMES), 8)
        self.assertLessEqual(len(probe.PACKET_TYPES), 20)


if __name__ == "__main__":
    unittest.main()
