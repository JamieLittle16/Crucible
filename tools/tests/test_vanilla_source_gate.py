from __future__ import annotations

import importlib.util
import json
import os
import sqlite3
import sys
import tempfile
import unittest
import zipfile
from contextlib import contextmanager
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

ATLAS_PATH = TOOLS / "vanilla_atlas.py"
atlas_spec = importlib.util.spec_from_file_location("vanilla_atlas", ATLAS_PATH)
assert atlas_spec and atlas_spec.loader
atlas = importlib.util.module_from_spec(atlas_spec)
sys.modules[atlas_spec.name] = atlas
atlas_spec.loader.exec_module(atlas)

GATE_PATH = TOOLS / "vanilla_source_gate.py"
gate_spec = importlib.util.spec_from_file_location("vanilla_source_gate", GATE_PATH)
assert gate_spec and gate_spec.loader
gate = importlib.util.module_from_spec(gate_spec)
sys.modules[gate_spec.name] = gate
gate_spec.loader.exec_module(gate)

SAMPLE = r'''
package net.minecraft.test;

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


@contextmanager
def working_directory(path: Path):
    previous = Path.cwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(previous)


class SourceAdmissionGateTests(unittest.TestCase):
    def _fixture(self, root: Path) -> tuple[Path, Path, Path]:
        archive = root / "source.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
            zf.writestr(
                "src/version.json",
                json.dumps({"id": "test", "world_version": 1, "protocol_version": 2}),
            )
            zf.writestr("src/net/minecraft/test/Sample.java", SAMPLE)
        db = root / "atlas.sqlite"
        atlas.index_archive(archive, db, None, None)

        conn = sqlite3.connect(db)
        conn.row_factory = sqlite3.Row
        row = conn.execute(
            """SELECT m.signature,m.normalized_sha256,m.body_sha256,t.qualified_name
               FROM methods m JOIN types t ON t.id=m.type_id WHERE m.name='mutate'"""
        ).fetchone()
        assert row is not None
        algorithm = dict(conn.execute("SELECT key,value FROM meta"))["fingerprint_algorithm"]
        conn.close()

        records = root / "records"
        records.mkdir()
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
            "hazards_reviewed": ["RNG"],
            "semantic_rules": ["SEM-TEST-001"],
            "evidence": [],
            "notes": [],
        }
        (records / "VAR-TEST-001.json").write_text(json.dumps(record), encoding="utf-8")

        frontier_dir = root / "vanilla" / "frontiers"
        frontier_dir.mkdir(parents=True)
        (frontier_dir / "test.json").write_text(
            json.dumps(
                {
                    "schema": 1,
                    "description": "test frontier",
                    "max_depth": 2,
                    "include_package_prefixes": ["net.minecraft.test"],
                    "root_queries": ["net.minecraft.test.Sample"],
                }
            ),
            encoding="utf-8",
        )

        gate_path = root / "gate.json"
        gate_path.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "id": "GATE-TEST-001",
                    "frontier": "test",
                    "minimum_status": "VAR_REVIEWED",
                    "require_semantic_rules": True,
                    "require_hazards_reviewed": True,
                    "methods": [
                        {
                            "query": f"{row['qualified_name']}#{row['signature']}",
                            "var_id": "VAR-TEST-001",
                        }
                    ],
                }
            ),
            encoding="utf-8",
        )
        return db, records, gate_path

    def _evaluate(self, root: Path) -> tuple[dict[str, object], Path, Path, Path]:
        db, records, gate_path = self._fixture(root)
        with working_directory(root):
            report = gate.evaluate(db_path=db, gate_path=gate_path, records_dir=records)
        return report, db, records, gate_path

    def test_admits_exact_reviewed_source_identity(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            report, _db, _records, _gate_path = self._evaluate(root)
            self.assertTrue(report["admitted"])
            self.assertEqual(report["failures"], [])
            required = report["required_methods"]
            self.assertEqual(len(required), 1)
            self.assertEqual(required[0]["var_id"], "VAR-TEST-001")
            self.assertEqual(required[0]["observed_hazards"], ["RNG"])
            self.assertEqual(required[0]["semantic_rules"], ["SEM-TEST-001"])
            self.assertEqual(report["frontier"]["name"], "test")
            self.assertEqual(report["source"]["protocol_version"], "2")

    def test_one_unresolved_explicit_frontier_root_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            _report, db, records, gate_path = self._evaluate(root)
            frontier_path = root / "vanilla" / "frontiers" / "test.json"
            frontier = json.loads(frontier_path.read_text(encoding="utf-8"))
            frontier["root_queries"].append("net.minecraft.test.DoesNotExist")
            frontier_path.write_text(json.dumps(frontier), encoding="utf-8")

            with working_directory(root), self.assertRaisesRegex(
                gate.GateError,
                r"frontier test root query resolved neither methods nor an exact type: net\.minecraft\.test\.DoesNotExist",
            ):
                gate.evaluate(db_path=db, gate_path=gate_path, records_dir=records)

    def test_report_is_deterministic_for_unchanged_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            _report, db, records, gate_path = self._evaluate(root)
            with working_directory(root):
                first = gate.evaluate(db_path=db, gate_path=gate_path, records_dir=records)
                second = gate.evaluate(db_path=db, gate_path=gate_path, records_dir=records)
            self.assertEqual(first, second)

    def test_stale_fingerprint_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            _report, db, records, gate_path = self._evaluate(root)
            path = records / "VAR-TEST-001.json"
            record = json.loads(path.read_text(encoding="utf-8"))
            record["source"]["normalized_sha256"] = "0" * 64
            path.write_text(json.dumps(record), encoding="utf-8")
            with working_directory(root):
                report = gate.evaluate(db_path=db, gate_path=gate_path, records_dir=records)
            self.assertFalse(report["admitted"])
            self.assertTrue(
                any("normalized source fingerprint is stale" in item for item in report["failures"])
            )

    def test_unreviewed_hazard_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            _report, db, records, gate_path = self._evaluate(root)
            path = records / "VAR-TEST-001.json"
            record = json.loads(path.read_text(encoding="utf-8"))
            record["hazards_reviewed"] = []
            path.write_text(json.dumps(record), encoding="utf-8")
            with working_directory(root):
                report = gate.evaluate(db_path=db, gate_path=gate_path, records_dir=records)
            self.assertFalse(report["admitted"])
            self.assertTrue(any("Atlas hazards not explicitly reviewed" in item for item in report["failures"]))

    def test_missing_sem_linkage_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            _report, db, records, gate_path = self._evaluate(root)
            path = records / "VAR-TEST-001.json"
            record = json.loads(path.read_text(encoding="utf-8"))
            record["semantic_rules"] = []
            path.write_text(json.dumps(record), encoding="utf-8")
            with working_directory(root):
                report = gate.evaluate(db_path=db, gate_path=gate_path, records_dir=records)
            self.assertFalse(report["admitted"])
            self.assertTrue(any("has no SEM linkage" in item for item in report["failures"]))

    def test_under_reviewed_status_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            _report, db, records, gate_path = self._evaluate(root)
            path = records / "VAR-TEST-001.json"
            record = json.loads(path.read_text(encoding="utf-8"))
            record["status"] = "INDEXED"
            path.write_text(json.dumps(record), encoding="utf-8")
            with working_directory(root):
                report = gate.evaluate(db_path=db, gate_path=gate_path, records_dir=records)
            self.assertFalse(report["admitted"])
            self.assertTrue(any("does not satisfy minimum" in item for item in report["failures"]))

    def test_duplicate_required_var_is_rejected_as_configuration_error(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            _report, _db, _records, gate_path = self._evaluate(root)
            config = json.loads(gate_path.read_text(encoding="utf-8"))
            duplicate = dict(config["methods"][0])
            duplicate["query"] += " "
            config["methods"].append(duplicate)
            gate_path.write_text(json.dumps(config), encoding="utf-8")
            with self.assertRaises(gate.GateError):
                gate.load_gate(gate_path)


if __name__ == "__main__":
    unittest.main()
