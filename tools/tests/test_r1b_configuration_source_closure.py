from __future__ import annotations

import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r1b_configuration_source_closure as closure


class R1BConfigurationSourceClosureTests(unittest.TestCase):
    def test_candidate_ids_and_effective_selectors_are_unique(self) -> None:
        ids = [candidate.var_id for candidate in closure.CANDIDATES]
        selectors = [closure._selector_key(candidate) for candidate in closure.CANDIDATES]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(len(selectors), len(set(selectors)))
        self.assertGreater(len(ids), 25)

    def _connection(self) -> sqlite3.Connection:
        connection = sqlite3.connect(":memory:")
        connection.row_factory = sqlite3.Row
        connection.executescript(
            """
            CREATE TABLE source_files(id INTEGER PRIMARY KEY, path TEXT NOT NULL);
            CREATE TABLE types(id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL, qualified_name TEXT NOT NULL);
            CREATE TABLE methods(
              id INTEGER PRIMARY KEY, type_id INTEGER NOT NULL, name TEXT NOT NULL,
              signature TEXT NOT NULL, param_count INTEGER NOT NULL,
              start_line INTEGER NOT NULL, end_line INTEGER NOT NULL
            );
            INSERT INTO source_files VALUES(1, 'src/X.java');
            INSERT INTO types VALUES(1, 1, 'example.X');
            INSERT INTO methods VALUES(1, 1, 'work', 'work(final int value)', 1, 10, 12);
            """
        )
        return connection

    def test_arity_selector_requires_exactly_one_match(self) -> None:
        connection = self._connection()
        candidate = closure.Candidate(
            "VAR-X", "example.X", "work", 1, ("SEM-X",), ("review",)
        )
        row = closure._resolve(connection, candidate)
        self.assertEqual(row["signature"], "work(final int value)")

        connection.execute(
            "INSERT INTO methods VALUES(2, 1, 'work', 'work(final String value)', 1, 20, 22)"
        )
        with self.assertRaises(closure.ClosureError):
            closure._resolve(connection, candidate)

    def test_exact_signature_disambiguates_same_arity_overloads(self) -> None:
        connection = self._connection()
        connection.execute(
            "INSERT INTO methods VALUES(2, 1, 'work', 'work(final String value)', 1, 20, 22)"
        )
        candidate = closure.Candidate(
            "VAR-X",
            "example.X",
            "work",
            1,
            ("SEM-X",),
            ("review",),
            "work(final String value)",
        )
        row = closure._resolve(connection, candidate)
        self.assertEqual(row["id"], 2)
        self.assertEqual(row["signature"], "work(final String value)")

    def test_exact_signature_must_agree_with_declared_method_and_arity(self) -> None:
        connection = self._connection()
        candidate = closure.Candidate(
            "VAR-X",
            "example.X",
            "other",
            1,
            ("SEM-X",),
            ("review",),
            "work(final int value)",
        )
        with self.assertRaisesRegex(closure.ClosureError, "disagrees with declared method/arity"):
            closure._resolve(connection, candidate)

    def test_selector_preflight_reports_all_failures(self) -> None:
        connection = self._connection()
        connection.execute(
            "INSERT INTO methods VALUES(2, 1, 'work', 'work(final String value)', 1, 20, 22)"
        )
        ambiguous = closure.Candidate(
            "VAR-AMBIG", "example.X", "work", 1, ("SEM-X",), ("review",)
        )
        missing = closure.Candidate(
            "VAR-MISSING", "example.X", "missing", 0, ("SEM-X",), ("review",)
        )
        with mock.patch.object(closure, "CANDIDATES", (ambiguous, missing)):
            with self.assertRaises(closure.ClosureError) as captured:
                closure._resolve_all(connection)
        message = str(captured.exception)
        self.assertIn("selector preflight failed", message)
        self.assertIn("VAR-AMBIG", message)
        self.assertIn("VAR-MISSING", message)

    def test_source_rich_output_is_rejected_inside_repository(self) -> None:
        with tempfile.TemporaryDirectory(dir=closure.REPO_ROOT) as temporary:
            path = Path(temporary) / "closure"
            with self.assertRaises(closure.ClosureError):
                closure._external_fresh_dir(path)

    def test_finalize_requires_explicit_inspection_and_hazard_disposition(self) -> None:
        candidate = closure.Candidate(
            "VAR-X", "example.X", "work", 0, ("SEM-X",), ("review",)
        )
        source = {
            "type": "example.X",
            "signature": "work()",
            "fingerprint_algorithm": "test",
            "normalized_sha256": "a" * 64,
            "body_sha256": "b" * 64,
        }
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(
            closure, "CANDIDATES", (candidate,)
        ):
            root = Path(temporary)
            review = root / "review"
            output = root / "reviewed"
            (review / "records").mkdir(parents=True)
            (review / "gate").mkdir()
            record = {
                "schema": 1,
                "id": "VAR-X",
                "status": "INDEXED",
                "source": source,
                "classifications": [],
                "hazards_reviewed": [],
                "semantic_rules": [],
                "evidence": [],
                "notes": [],
            }
            (review / "records" / "VAR-X.json").write_text(json.dumps(record))
            gate = {
                "schema": 1,
                "id": closure.GATE_ID,
                "minimum_status": "VAR_REVIEWED",
                "require_semantic_rules": True,
                "require_hazards_reviewed": True,
                "methods": [{"query": "example.X#work()", "var_id": "VAR-X"}],
            }
            (review / "gate" / f"{closure.GATE_ID}.json").write_text(json.dumps(gate))
            worksheet = {
                "schema": 1,
                "kind": closure.WORKSHEET_KIND,
                "contains_official_source_text": False,
                "candidate_count": 1,
                "candidates": [
                    {
                        "var_id": "VAR-X",
                        "source": source,
                        "atlas_observed_hazards": ["CODEC"],
                        "decision": {
                            "source_inspected": False,
                            "accepted": False,
                            "reviewer": "",
                            "note": "",
                            "hazards_reviewed": [],
                            "semantic_rules": [],
                        },
                    }
                ],
            }
            worksheet_path = review / "review-worksheet.json"
            worksheet_path.write_text(json.dumps(worksheet))
            with self.assertRaises(closure.ClosureError):
                closure.finalize(review, output)
            self.assertFalse(output.exists())

            decision = worksheet["candidates"][0]["decision"]
            decision.update(
                source_inspected=True,
                accepted=True,
                reviewer="reviewer",
                note="Reviewed exact source body.",
                semantic_rules=["SEM-X"],
            )
            worksheet_path.write_text(json.dumps(worksheet))
            with self.assertRaises(closure.ClosureError):
                closure.finalize(review, output)
            self.assertFalse(output.exists())

            decision["hazards_reviewed"] = ["CODEC"]
            worksheet_path.write_text(json.dumps(worksheet))
            closure.finalize(review, output)
            reviewed = json.loads((output / "records" / "VAR-X.json").read_text())
            self.assertEqual(reviewed["status"], "VAR_REVIEWED")
            self.assertEqual(reviewed["hazards_reviewed"], ["CODEC"])
            self.assertEqual(reviewed["semantic_rules"], ["SEM-X"])


if __name__ == "__main__":
    unittest.main()
