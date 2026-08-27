from __future__ import annotations

import importlib.util
import json
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]


def _load(name: str, path: Path):  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


wire = _load(
    "r2b_play_entry_wire_closure_source_review",
    TOOLS / "r2b_play_entry_wire_closure_source_review.py",
)


def _db() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.executescript(
        """
        CREATE TABLE source_files(id INTEGER PRIMARY KEY, path TEXT NOT NULL);
        CREATE TABLE types(
            id INTEGER PRIMARY KEY,
            file_id INTEGER NOT NULL,
            qualified_name TEXT NOT NULL
        );
        CREATE TABLE methods(
            id INTEGER PRIMARY KEY,
            type_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            signature TEXT NOT NULL,
            param_count INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL
        );
        """
    )
    return conn


def _type(conn: sqlite3.Connection, type_id: int, qname: str) -> None:
    conn.execute(
        "INSERT OR IGNORE INTO source_files(id,path) VALUES(?,?)",
        (type_id, f"src/{type_id}.java"),
    )
    conn.execute(
        "INSERT INTO types(id,file_id,qualified_name) VALUES(?,?,?)",
        (type_id, type_id, qname),
    )


def _method(
    conn: sqlite3.Connection,
    method_id: int,
    type_id: int,
    name: str,
    signature: str,
    param_count: int = 0,
    line: int | None = None,
) -> None:
    start = line if line is not None else method_id
    conn.execute(
        """INSERT INTO methods(
               id,type_id,name,signature,param_count,start_line,end_line
           ) VALUES(?,?,?,?,?,?,?)""",
        (method_id, type_id, name, signature, param_count, start, start + 1),
    )


class R2BPlayEntryWireClosureTests(unittest.TestCase):
    def test_committed_plan_is_seven_wire_only_families(self) -> None:
        groups = wire.load_plan()
        self.assertEqual(
            [group.group_id for group in groups],
            [
                "COMMAND_TREE",
                "RECIPE_BOOK_SETTINGS",
                "SYNCHRONIZED_RECIPES",
                "CLOCK_FULL_SYNC",
                "DEFAULT_SPAWN",
                "DIMENSION_TYPE",
                "INITIAL_INVENTORY",
            ],
        )
        self.assertTrue(all(group.review_focus for group in groups))

    def test_required_type_names_fail_closed_per_name(self) -> None:
        conn = _db()
        _type(conn, 1, "test.Commands")
        _method(conn, 1, 1, "first", "first()")
        selector = wire.Selector(
            mode="type_names",
            required=True,
            type_name="test.Commands",
            names=("first", "second"),
        )
        with self.assertRaisesRegex(wire.WireClosureError, "second"):
            wire._query_selector(conn, selector)
        conn.close()

    def test_resolution_deduplicates_and_excludes_prior_review(self) -> None:
        conn = _db()
        _type(conn, 1, "test.Packet")
        _method(conn, 10, 1, "encode", "encode()")
        _method(conn, 11, 1, "decode", "decode()")
        group = wire.Group(
            group_id="WIRE",
            review_focus="wire",
            selectors=(
                wire.Selector(
                    mode="type_all",
                    required=True,
                    type_name="test.Packet",
                ),
                wire.Selector(
                    mode="type_name_regex",
                    required=True,
                    type_name="test.Packet",
                    name_regex="encode",
                ),
            ),
        )
        resolved = wire.resolve_groups(
            conn,
            (group,),
            {"test.Packet#decode()"},
        )
        self.assertEqual(len(resolved), 1)
        row, group_ids, focus = resolved[0]
        self.assertEqual(row["signature"], "encode()")
        self.assertEqual(group_ids, ("WIRE",))
        self.assertEqual(focus, ("wire",))
        conn.close()

    def test_group_with_only_prior_rows_fails_closed(self) -> None:
        conn = _db()
        _type(conn, 1, "test.Packet")
        _method(conn, 10, 1, "encode", "encode()")
        group = wire.Group(
            group_id="WIRE",
            review_focus="wire",
            selectors=(
                wire.Selector(
                    mode="type_all",
                    required=True,
                    type_name="test.Packet",
                ),
            ),
        )
        with self.assertRaisesRegex(wire.WireClosureError, "already present"):
            wire.resolve_groups(conn, (group,), {"test.Packet#encode()"})
        conn.close()

    def test_prefix_selector_is_bounded_to_requested_family(self) -> None:
        conn = _db()
        _type(conn, 1, "test.Family$Nested")
        _type(conn, 2, "test.Other")
        _method(conn, 10, 1, "write", "write()")
        _method(conn, 11, 2, "write", "write()")
        selector = wire.Selector(
            mode="prefix_all",
            required=True,
            type_prefix="test.Family$",
        )
        rows = wire._query_selector(conn, selector)
        self.assertEqual(
            [row["qualified_name"] for row in rows],
            ["test.Family$Nested"],
        )
        conn.close()

    def test_regex_selector_filters_method_names_not_source_text(self) -> None:
        conn = _db()
        _type(conn, 1, "test.Item")
        _method(conn, 10, 1, "createStreamCodec", "createStreamCodec()")
        _method(conn, 11, 1, "copy", "copy()")
        selector = wire.Selector(
            mode="type_name_regex",
            required=True,
            type_name="test.Item",
            name_regex="(?i)(stream|codec|write)",
        )
        rows = wire._query_selector(conn, selector)
        self.assertEqual([row["name"] for row in rows], ["createStreamCodec"])
        conn.close()

    def test_prior_review_validation_requires_exact_final_67_identity(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "review-dossier.json"
            candidates = []
            for index in range(wire.EXPECTED_PRIOR_COUNT):
                identity = f"test.Type{index}#method()"
                if index == 0:
                    identity = (
                        "net.minecraft.server.level.ServerPlayer#"
                        "<fieldinit:containerSynchronizer>()"
                    )
                candidates.append(
                    {
                        "source_identity": identity,
                        "source": {
                            "fingerprint_algorithm": "java-token-v2-literal-sensitive",
                            "normalized_sha256": "a" * 64,
                            "body_sha256": "b" * 64,
                        },
                        "source_excerpt": "reviewed source body\n",
                    }
                )
            value = {
                "id": wire.PRIOR_REVIEW_ID,
                "kind": "r2b-play-entry-final-source-review",
                "contains_official_source_text": True,
                "candidate_count": wire.EXPECTED_PRIOR_COUNT,
                "source_archive_sha256": wire.EXPECTED_SOURCE_SHA256,
                "candidates": candidates,
            }
            path.write_text(json.dumps(value), encoding="utf-8")
            identities, digest = wire.validate_prior_review(path)
            self.assertEqual(len(identities), wire.EXPECTED_PRIOR_COUNT)
            self.assertEqual(len(digest), 64)

            value["candidate_count"] = 66
            path.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(wire.WireClosureError, "identity mismatch"):
                wire.validate_prior_review(path)

    def test_plan_rejects_scope_schema_drift(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "plan.json"
            path.write_text(
                json.dumps(
                    {
                        "schema": wire.SCHEMA,
                        "id": wire.REVIEW_ID,
                        "prior_review_id": wire.PRIOR_REVIEW_ID,
                        "source_archive_sha256": wire.EXPECTED_SOURCE_SHA256,
                        "groups": [],
                        "unexpected": True,
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(wire.WireClosureError, "unexpected fields"):
                wire.load_plan(path)


if __name__ == "__main__":
    unittest.main()
