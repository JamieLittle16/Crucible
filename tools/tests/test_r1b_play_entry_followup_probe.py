from __future__ import annotations

import sqlite3
import unittest
from unittest import mock

from tools import r1b_play_entry_followup_probe as probe


class R1BPlayEntryFollowupProbeTests(unittest.TestCase):
    def _connection(self) -> sqlite3.Connection:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        conn.executescript(
            """
            CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE types(
              id INTEGER PRIMARY KEY,
              qualified_name TEXT NOT NULL
            );
            CREATE TABLE methods(
              id INTEGER PRIMARY KEY,
              type_id INTEGER NOT NULL,
              name TEXT NOT NULL,
              signature TEXT NOT NULL,
              param_count INTEGER NOT NULL,
              start_line INTEGER NOT NULL
            );
            CREATE TABLE method_calls(
              id INTEGER PRIMARY KEY,
              caller_method_id INTEGER NOT NULL,
              owner_text TEXT,
              callee_name TEXT NOT NULL,
              arg_count INTEGER NOT NULL,
              line INTEGER NOT NULL,
              resolution TEXT,
              resolved_method_id INTEGER
            );
            """
        )
        conn.executemany("INSERT INTO meta(key,value) VALUES(?,?)", probe.EXPECTED_META.items())
        conn.executemany(
            "INSERT INTO types(id,qualified_name) VALUES(?,?)",
            [(1, "example.Anchor"), (2, "example.Helper")],
        )
        conn.executemany(
            "INSERT INTO methods(id,type_id,name,signature,param_count,start_line) VALUES(?,?,?,?,?,?)",
            [
                (10, 1, "work", "work()", 0, 10),
                (20, 2, "helper", "helper(final int value)", 1, 20),
                (21, 2, "<clinit>", "<clinit>()", 0, 5),
            ],
        )
        conn.executemany(
            """INSERT INTO method_calls(
                 id,caller_method_id,owner_text,callee_name,arg_count,line,resolution,resolved_method_id
               ) VALUES(?,?,?,?,?,?,?,?)""",
            [
                (1, 10, "Helper", "helper", 1, 11, "imported-type", 20),
                (2, 10, "dynamic", "unknown", 0, 12, None, None),
            ],
        )
        return conn

    def test_report_is_source_free_and_preserves_resolved_and_unresolved_edges(self) -> None:
        conn = self._connection()
        try:
            with (
                mock.patch.object(probe, "ANCHORS", ("example.Anchor#work()",)),
                mock.patch.object(probe, "SYMBOL_NAMES", ("helper",)),
                mock.patch.object(probe, "TYPE_NAMES", ("example.Helper",)),
            ):
                report = probe.build_report(conn)
        finally:
            conn.close()

        self.assertFalse(report["contains_official_source_text"])
        self.assertEqual(report["anchors"][0]["identity"], "example.Anchor#work()")
        calls = report["anchors"][0]["calls"]
        self.assertEqual(calls[0]["resolved_identity"], "example.Helper#helper(final int value)")
        self.assertIsNone(calls[1]["resolved_identity"])
        self.assertEqual(report["symbol_matches"]["helper"], ["example.Helper#helper(final int value)"])
        self.assertEqual(
            report["type_methods"]["example.Helper"],
            ["example.Helper#<clinit>()", "example.Helper#helper(final int value)"],
        )
        self.assertNotIn("source_excerpt", str(report))

    def test_source_pin_mismatch_fails_closed(self) -> None:
        conn = self._connection()
        try:
            conn.execute("UPDATE meta SET value='999' WHERE key='protocol_version'")
            with self.assertRaises(probe.ProbeError):
                probe._verify_meta(conn)
        finally:
            conn.close()

    def test_anchor_must_resolve_exactly_once(self) -> None:
        conn = self._connection()
        try:
            with self.assertRaises(probe.ProbeError):
                probe._method_row(conn, "example.Anchor#missing()")
        finally:
            conn.close()


if __name__ == "__main__":
    unittest.main()
