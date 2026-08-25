from __future__ import annotations

import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r1b_play_inventory_sync_probe as probe


class R1BPlayInventorySyncProbeTests(unittest.TestCase):
    def _connection(self) -> sqlite3.Connection:
        connection = sqlite3.connect(":memory:")
        connection.row_factory = sqlite3.Row
        connection.executescript(
            """
            CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE types(id INTEGER PRIMARY KEY,qualified_name TEXT NOT NULL);
            CREATE TABLE methods(
              id INTEGER PRIMARY KEY,
              type_id INTEGER NOT NULL,
              name TEXT NOT NULL,
              signature TEXT NOT NULL,
              param_count INTEGER NOT NULL,
              start_line INTEGER NOT NULL,
              end_line INTEGER NOT NULL
            );
            CREATE TABLE method_calls(
              id INTEGER PRIMARY KEY,
              caller_method_id INTEGER NOT NULL,
              line INTEGER NOT NULL,
              owner_text TEXT,
              callee_name TEXT NOT NULL,
              arg_count INTEGER NOT NULL,
              resolution TEXT NOT NULL,
              resolved_method_id INTEGER
            );
            """
        )
        connection.executemany(
            "INSERT INTO meta(key,value) VALUES(?,?)",
            [
                ("source_archive_sha256", probe.EXPECTED_SOURCE_SHA256),
                ("fingerprint_algorithm", probe.EXPECTED_FINGERPRINT),
                ("atlas_version", probe.EXPECTED_ATLAS_VERSION),
                ("minecraft_version", "26.2"),
                ("protocol_version", "776"),
                ("world_version", "4903"),
            ],
        )
        connection.executemany(
            "INSERT INTO types(id,qualified_name) VALUES(?,?)",
            [
                (1, "net.minecraft.world.inventory.AbstractContainerMenu"),
                (2, "net.minecraft.server.level.ServerPlayer$ContainerSynchronizer"),
            ],
        )
        connection.executemany(
            "INSERT INTO methods(id,type_id,name,signature,param_count,start_line,end_line) VALUES(?,?,?,?,?,?,?)",
            [
                (10, 1, "addSlotListener", "addSlotListener(final ContainerListener listener)", 1, 10, 12),
                (11, 1, "setSynchronizer", "setSynchronizer(final ContainerSynchronizer synchronizer)", 1, 20, 22),
                (12, 1, "sendAllDataToRemote", "sendAllDataToRemote()", 0, 30, 33),
                (20, 2, "sendInitialData", "sendInitialData(final AbstractContainerMenu menu , final NonNullList < ItemStack > items , final ItemStack carriedItem , final int [ ] data)", 4, 40, 44),
            ],
        )
        connection.execute(
            "INSERT INTO method_calls(id,caller_method_id,line,owner_text,callee_name,arg_count,resolution,resolved_method_id) VALUES(1,11,21,'this','sendAllDataToRemote',0,'same-type',12)"
        )
        return connection

    def test_probe_resolves_exact_roots_and_named_synchronizer(self) -> None:
        connection = self._connection()
        with mock.patch.object(probe.vanilla_atlas, "connect_db", return_value=connection):
            result = probe.probe(Path("ignored.sqlite"))
        self.assertEqual(len(result["roots"]), 3)
        self.assertEqual(result["roots"][1]["matches"][0]["calls"][0]["resolved_identity"], "net.minecraft.world.inventory.AbstractContainerMenu#sendAllDataToRemote()")
        names = {item["name"] for item in result["named_synchronizer_methods"]}
        self.assertIn("sendInitialData", names)
        self.assertFalse(result["contains_official_source_text"])

    def test_source_pin_mismatch_fails_closed(self) -> None:
        connection = self._connection()
        connection.execute("UPDATE meta SET value='bad' WHERE key='source_archive_sha256'")
        with mock.patch.object(probe.vanilla_atlas, "connect_db", return_value=connection):
            with self.assertRaisesRegex(probe.ProbeError, "source archive"):
                probe.probe(Path("ignored.sqlite"))


if __name__ == "__main__":
    unittest.main()
