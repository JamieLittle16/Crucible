from __future__ import annotations

import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_projection_source_review as review


REPO_ROOT = Path(__file__).resolve().parents[2]
PLAN = REPO_ROOT / "vanilla/reviews/network/r2c-world-projection-discovery-plan.json"


def atlas_fixture() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.executescript(
        """
        CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE source_files(id INTEGER PRIMARY KEY, path TEXT NOT NULL);
        CREATE TABLE types(id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL, qualified_name TEXT NOT NULL);
        CREATE TABLE methods(
            id INTEGER PRIMARY KEY,
            type_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            signature TEXT NOT NULL,
            param_count INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            normalized_sha256 TEXT NOT NULL,
            body_sha256 TEXT NOT NULL
        );
        CREATE TABLE hazards(method_id INTEGER NOT NULL, kind TEXT NOT NULL, detail TEXT, line INTEGER);
        CREATE TABLE classifications(
            method_id INTEGER NOT NULL,
            label TEXT NOT NULL,
            confidence REAL,
            source TEXT NOT NULL,
            reason TEXT
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
    conn.executemany(
        "INSERT INTO meta(key,value) VALUES(?,?)",
        (
            ("minecraft_version", "26.2"),
            ("protocol_version", "776"),
            ("world_version", "4903"),
            ("atlas_version", "0.1.1"),
            ("fingerprint_algorithm", "java-token-v2-literal-sensitive"),
        ),
    )
    conn.execute("INSERT INTO source_files(id,path) VALUES(1,'src/A.java')")
    conn.execute("INSERT INTO source_files(id,path) VALUES(2,'src/AB.java')")
    conn.execute("INSERT INTO types(id,file_id,qualified_name) VALUES(1,1,'net.minecraft.A')")
    conn.execute("INSERT INTO types(id,file_id,qualified_name) VALUES(2,2,'net.minecraft.AB')")
    conn.execute(
        """INSERT INTO methods(
               id,type_id,name,signature,param_count,start_line,end_line,normalized_sha256,body_sha256
           ) VALUES(1,1,'alpha','alpha()',0,10,20,'norm-a','body-a')"""
    )
    conn.execute(
        """INSERT INTO methods(
               id,type_id,name,signature,param_count,start_line,end_line,normalized_sha256,body_sha256
           ) VALUES(2,1,'beta','beta(int)',1,30,45,'norm-b','body-b')"""
    )
    conn.execute(
        """INSERT INTO methods(
               id,type_id,name,signature,param_count,start_line,end_line,normalized_sha256,body_sha256
           ) VALUES(3,2,'wrong','wrong()',0,1,5,'norm-wrong','body-wrong')"""
    )
    conn.execute("INSERT INTO hazards(method_id,kind,detail,line) VALUES(1,'iteration-order','fixture',12)")
    conn.execute(
        """INSERT INTO classifications(method_id,label,confidence,source,reason)
           VALUES(1,'OBSERVABLE',0.5,'heuristic','fixture')"""
    )
    conn.execute(
        """INSERT INTO method_calls(
               id,caller_method_id,owner_text,callee_name,arg_count,line,resolution,resolved_method_id
           ) VALUES(1,1,'this','beta',1,15,'resolved',2)"""
    )
    conn.execute(
        """INSERT INTO method_calls(
               id,caller_method_id,owner_text,callee_name,arg_count,line,resolution,resolved_method_id
           ) VALUES(2,1,'helper','mystery',0,16,'unresolved',NULL)"""
    )
    return conn


class R2cWorldProjectionSourceReviewTests(unittest.TestCase):
    def test_committed_plan_and_frontier_validate_together(self) -> None:
        plan = review._load_plan(PLAN)
        frontier = review._load_frontier(plan.frontier, plan)
        self.assertEqual([group.group_id for group in plan.groups], list(review.EXPECTED_GROUPS))
        self.assertEqual(set(frontier["root_queries"]), {root for group in plan.groups for root in group.root_types})
        self.assertLessEqual(plan.max_methods_per_type, 4096)

    def test_exact_type_resolution_never_uses_substring_matches(self) -> None:
        conn = atlas_fixture()
        self.addCleanup(conn.close)
        rows = review._exact_type_methods(conn, "net.minecraft.A", 8)
        self.assertEqual([row["signature"] for row in rows], ["alpha()", "beta(int)"])
        with self.assertRaisesRegex(review.DiscoveryError, "zero Atlas methods"):
            review._exact_type_methods(conn, "net.minecraft.Missing", 8)

    def test_exact_type_resolution_enforces_bounded_method_cap(self) -> None:
        conn = atlas_fixture()
        self.addCleanup(conn.close)
        with self.assertRaisesRegex(review.DiscoveryError, "too broad"):
            review._exact_type_methods(conn, "net.minecraft.A", 1)

    def test_method_inventory_is_source_free_and_preserves_call_structure(self) -> None:
        conn = atlas_fixture()
        self.addCleanup(conn.close)
        row = review._exact_type_methods(conn, "net.minecraft.A", 8)[0]
        inventory = review._method_inventory(conn, row, "DISC-1")
        encoded = json.dumps(inventory, sort_keys=True)
        self.assertEqual(inventory["source_identity"], "net.minecraft.A#alpha()")
        self.assertEqual(inventory["atlas_observed_hazards"], ["iteration-order"])
        self.assertEqual(inventory["atlas_classifications"], ["OBSERVABLE"])
        calls = inventory["calls"]
        self.assertEqual(calls["call_sites"], 2)
        self.assertEqual(calls["resolved_targets"], ["net.minecraft.A#beta(int)"])
        self.assertEqual(calls["unresolved_call_sites"], 1)
        self.assertNotIn("source_excerpt", encoded)
        self.assertNotIn("official source", encoded.lower())

    def test_external_output_policy_rejects_repository_paths_and_existing_paths(self) -> None:
        with self.assertRaisesRegex(review.DiscoveryError, "outside the repository"):
            review._fresh_external_dir(REPO_ROOT / "target/r2c-discovery-new")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            existing = root / "existing"
            existing.mkdir()
            with self.assertRaisesRegex(review.DiscoveryError, "must not already exist"):
                review._fresh_external_dir(existing)
            fresh = root / "fresh"
            self.assertEqual(review._fresh_external_dir(fresh), fresh.resolve())

    def test_plan_loader_fails_closed_on_group_drift(self) -> None:
        value = json.loads(PLAN.read_text(encoding="utf-8"))
        value["groups"][0]["group_id"] = "R2C-RENAMED"
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "plan.json"
            path.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(review.DiscoveryError, "nine semantic groups"):
                review._load_plan(path)

    def test_pretty_serialization_and_digest_are_deterministic(self) -> None:
        value = {"b": [2, 1], "a": {"z": False}}
        first = review._pretty_bytes(value)
        second = review._pretty_bytes(value)
        self.assertEqual(first, second)
        self.assertEqual(review._sha256_bytes(first), review._sha256_bytes(second))
        self.assertTrue(first.endswith(b"\n"))


if __name__ == "__main__":
    unittest.main()
