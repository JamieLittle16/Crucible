from __future__ import annotations

import importlib.util
import json
import sqlite3
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "vanilla_atlas.py"
spec = importlib.util.spec_from_file_location("vanilla_atlas", MODULE_PATH)
assert spec and spec.loader
atlas = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = atlas
spec.loader.exec_module(atlas)


SAMPLE = r'''
package net.minecraft.test;
import java.util.HashMap;

public final class Sample {
    private int count;
    private final RandomSource random;

    public Sample(RandomSource random) {
        this.random = random;
    }

    public int mutate(int amount) {
        this.count += amount;
        if (this.random.nextBoolean()) {
            this.count++;
        }
        return helper(this.count);
    }

    private int helper(int value) {
        return value + 1;
    }
}
'''


class LexicalTests(unittest.TestCase):
    def test_comments_and_literals_do_not_create_symbols(self) -> None:
        tokens = atlas.tokenize_java('class A { /* fakeCall() */ String s = "otherCall()"; void x(){ real(); } }')
        texts = [t.text for t in tokens]
        self.assertIn("real", texts)
        self.assertNotIn("fakeCall", texts)
        self.assertNotIn("otherCall", texts)

    def test_extracts_type_methods_fields_and_hazards(self) -> None:
        tokens = atlas.tokenize_java(SAMPLE)
        braces = atlas.matching_pairs(tokens, "{", "}")
        parens = atlas.matching_pairs(tokens, "(", ")")
        package, _imports = atlas.package_and_imports(tokens)
        types = atlas.extract_types(tokens, package, braces)
        self.assertEqual([t.qualified_name for t in types], ["net.minecraft.test.Sample"])
        methods, fields = atlas.extract_members(tokens, types[0], parens, braces)
        self.assertEqual({f.name for f in fields}, {"count", "random"})
        self.assertEqual({m.name for m in methods}, {"Sample", "mutate", "helper"})
        mutate = next(m for m in methods if m.name == "mutate")
        accesses = atlas.field_accesses(tokens, mutate, {"count", "random"})
        self.assertTrue(any(name == "count" and mode == "read_write" for name, mode, _ in accesses))
        hazards = atlas.hazards_for(tokens, mutate, package, "Sample")
        self.assertTrue(any(kind == "RNG" for kind, _, _ in hazards))


    def test_normalized_fingerprint_ignores_layout_but_preserves_literals(self) -> None:
        a = atlas.tokenize_java('void x(){ int n = 48; String s = "alpha"; }')
        b = atlas.tokenize_java('void x() { /* layout only */ int n=48; String s="alpha"; }')
        changed_number = atlas.tokenize_java('void x(){ int n = 64; String s = "alpha"; }')
        changed_string = atlas.tokenize_java('void x(){ int n = 48; String s = "beta"; }')
        self.assertEqual(atlas.normalized_fingerprint(a), atlas.normalized_fingerprint(b))
        self.assertNotEqual(atlas.normalized_fingerprint(a), atlas.normalized_fingerprint(changed_number))
        self.assertNotEqual(atlas.normalized_fingerprint(a), atlas.normalized_fingerprint(changed_string))


class IndexTests(unittest.TestCase):
    def _archive(self, root: Path) -> Path:
        archive = root / "source.zip"
        version = {
            "id": "test",
            "world_version": 1,
            "protocol_version": 2,
        }
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
            zf.writestr("src/version.json", json.dumps(version))
            zf.writestr("src/net/minecraft/test/Sample.java", SAMPLE)
        return archive

    def test_index_populates_sqlite_and_resolves_same_type_calls(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = self._archive(root)
            db = root / "atlas.sqlite"
            report = atlas.index_archive(archive, db, None, None)
            self.assertEqual(report["source"]["java_files"], 1)
            self.assertEqual(report["index"]["types"], 1)
            self.assertGreaterEqual(report["index"]["methods"], 3)
            conn = sqlite3.connect(db)
            resolved = conn.execute(
                "SELECT COUNT(*) FROM method_calls WHERE callee_name='helper' AND resolved_method_id IS NOT NULL"
            ).fetchone()[0]
            self.assertEqual(resolved, 1)
            rng = conn.execute("SELECT COUNT(*) FROM hazards WHERE kind='RNG'").fetchone()[0]
            self.assertGreaterEqual(rng, 1)
            conn.close()

    def test_committed_report_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = self._archive(root)
            outputs = []
            for i in range(2):
                db = root / f"atlas-{i}.sqlite"
                report_json = root / f"report-{i}.json"
                report_md = root / f"report-{i}.md"
                atlas.index_archive(archive, db, report_json, report_md)
                outputs.append((report_json.read_bytes(), report_md.read_bytes()))
            self.assertEqual(outputs[0], outputs[1])


class RecordSyncTests(unittest.TestCase):
    _archive = IndexTests._archive
    def test_sync_is_idempotent_and_record_removals_propagate(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = self._archive(root)
            db = root / "atlas.sqlite"
            atlas.index_archive(archive, db, None, None)
            conn = sqlite3.connect(db)
            conn.row_factory = sqlite3.Row
            row = conn.execute(
                """SELECT m.signature,m.normalized_sha256,m.body_sha256,t.qualified_name
                   FROM methods m JOIN types t ON t.id=m.type_id WHERE m.name='mutate'"""
            ).fetchone()
            algorithm = dict(conn.execute("SELECT key,value FROM meta"))["fingerprint_algorithm"]
            conn.close()
            records = root / "records"
            records.mkdir()
            record_path = records / "mutate.json"
            record = {
                "schema": 1,
                "id": "VAR-TEST-001",
                "status": "VAR_REVIEWED",
                "source": {
                    "type": row["qualified_name"],
                    "signature": row["signature"],
                    "fingerprint_algorithm": algorithm,
                    "normalized_sha256": row["normalized_sha256"],
                    "body_sha256": row["body_sha256"],
                },
                "classifications": ["SEMANTIC_GAMEPLAY"],
                "semantic_rules": ["SEM-TEST-001"],
                "evidence": ["EQUIV-TEST-001"],
                "hazards_reviewed": ["RNG"],
                "notes": [],
            }
            record_path.write_text(json.dumps(record), encoding="utf-8")
            self.assertEqual(atlas.cmd_sync_records(db, records), 0)
            self.assertEqual(atlas.cmd_sync_records(db, records), 0)
            conn = sqlite3.connect(db)
            self.assertEqual(conn.execute("SELECT COUNT(*) FROM semantic_edges WHERE var_id='VAR-TEST-001'").fetchone()[0], 2)
            self.assertEqual(conn.execute("SELECT COUNT(*) FROM classifications WHERE source='manual' AND reason='VAR-TEST-001'").fetchone()[0], 1)
            conn.close()

            record["classifications"] = []
            record["semantic_rules"] = []
            record["evidence"] = []
            record_path.write_text(json.dumps(record), encoding="utf-8")
            self.assertEqual(atlas.cmd_sync_records(db, records), 0)
            conn = sqlite3.connect(db)
            self.assertEqual(conn.execute("SELECT COUNT(*) FROM semantic_edges WHERE var_id='VAR-TEST-001'").fetchone()[0], 0)
            self.assertEqual(conn.execute("SELECT COUNT(*) FROM classifications WHERE source='manual' AND reason='VAR-TEST-001'").fetchone()[0], 0)
            conn.close()

    def test_stale_records_are_prioritized_by_next(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = self._archive(root)
            db = root / "atlas.sqlite"
            atlas.index_archive(archive, db, None, None)
            conn = sqlite3.connect(db)
            mutate_id = conn.execute("SELECT id FROM methods WHERE name='mutate'").fetchone()[0]
            conn.execute("UPDATE tracking SET review_status='STALE', var_id='VAR-TEST-STALE' WHERE method_id=?", (mutate_id,))
            conn.commit()
            conn.close()
            frontier = root / "frontier.json"
            frontier.write_text(json.dumps({
                "schema": 1,
                "description": "test",
                "max_depth": 1,
                "include_package_prefixes": ["net.minecraft.test"],
                "root_queries": ["net.minecraft.test.Sample"],
            }), encoding="utf-8")
            import contextlib, io
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(atlas.cmd_next(db, "test", frontier, 10), 0)
            lines = [line for line in out.getvalue().splitlines() if "net.minecraft.test.Sample#" in line]
            self.assertTrue(lines)
            self.assertIn("STALE", lines[0])


if __name__ == "__main__":
    unittest.main()
