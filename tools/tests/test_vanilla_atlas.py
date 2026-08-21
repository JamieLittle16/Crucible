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


if __name__ == "__main__":
    unittest.main()
