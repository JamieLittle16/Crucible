from __future__ import annotations

import sqlite3
import unittest
from unittest.mock import patch

from tools import vanilla_source_gate as gate


class VanillaSourceGateDeclarationRootTests(unittest.TestCase):
    def connection(self) -> sqlite3.Connection:
        conn = sqlite3.connect(":memory:")
        conn.execute(
            "CREATE TABLE types(id INTEGER PRIMARY KEY, qualified_name TEXT NOT NULL)"
        )
        conn.execute(
            "INSERT INTO types(id,qualified_name) VALUES(1,'net.minecraft.ExactDeclaration')"
        )
        return conn

    def test_exact_declaration_only_root_is_valid_context_anchor(self) -> None:
        conn = self.connection()
        self.addCleanup(conn.close)
        config = {"root_queries": ["net.minecraft.ExactDeclaration"]}
        with patch.object(gate.atlas, "resolve_methods", return_value=[]):
            gate._require_each_frontier_root(conn, config, "fixture")

    def test_missing_declaration_only_root_still_fails_closed(self) -> None:
        conn = self.connection()
        self.addCleanup(conn.close)
        config = {"root_queries": ["net.minecraft.MissingDeclaration"]}
        with patch.object(gate.atlas, "resolve_methods", return_value=[]), self.assertRaisesRegex(
            gate.GateError,
            "resolved neither methods nor an exact type",
        ):
            gate._require_each_frontier_root(conn, config, "fixture")


if __name__ == "__main__":
    unittest.main()
