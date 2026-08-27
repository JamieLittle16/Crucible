from __future__ import annotations

import importlib.util
import json
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "tools" / "state_source_qualification.py"
SPEC = importlib.util.spec_from_file_location("helve_state_source_qualification", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
qualification = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = qualification
SPEC.loader.exec_module(qualification)


class StateSourceQualificationTests(unittest.TestCase):
    def make_fixture(self, root: Path) -> tuple[Path, Path, Path]:
        lock = root / "vanilla.lock.toml"
        lock.write_text(
            """schema = 1
minecraft = "26.2"
protocol = 776
data_version = 4903

[source]
archive_sha256 = "source-sha"
java_files = 1

[atlas]
schema = 1
version = "0.1.1"
fingerprint_algorithm = "java-token-v2-literal-sensitive"
""",
            encoding="utf-8",
        )
        spec = root / "spec.json"
        spec.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "target": {
                        "minecraft_version": "26.2",
                        "protocol_version": 776,
                        "data_version": 4903,
                    },
                    "locators": [
                        {
                            "id": "TYPE",
                            "kind": "type",
                            "owner": "example.State",
                            "classification": "PROBE_SUPPORT",
                            "role": "type evidence",
                        },
                        {
                            "id": "FIELD",
                            "kind": "field",
                            "owner": "example.State",
                            "name": "REGISTRY",
                            "classification": "SEMANTIC_TARGET_DATA",
                            "role": "field evidence",
                        },
                        {
                            "id": "METHOD",
                            "kind": "method",
                            "owner": "example.State",
                            "name": "isAir",
                            "param_count": 0,
                            "classification": "SEMANTIC_TARGET_DATA",
                            "role": "method evidence",
                        },
                    ],
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )

        atlas = root / "atlas.sqlite"
        conn = sqlite3.connect(atlas)
        conn.executescript(
            """
            CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE source_files(
              id INTEGER PRIMARY KEY, path TEXT NOT NULL, sha256 TEXT NOT NULL
            );
            CREATE TABLE types(
              id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL,
              qualified_name TEXT NOT NULL, kind TEXT NOT NULL,
              normalized_sha256 TEXT NOT NULL,
              start_line INTEGER NOT NULL, end_line INTEGER NOT NULL
            );
            CREATE TABLE fields(
              id INTEGER PRIMARY KEY, type_id INTEGER NOT NULL,
              name TEXT NOT NULL, type_name TEXT NOT NULL,
              modifiers TEXT NOT NULL, line INTEGER NOT NULL
            );
            CREATE TABLE methods(
              id INTEGER PRIMARY KEY, type_id INTEGER NOT NULL,
              name TEXT NOT NULL, signature TEXT NOT NULL,
              return_type TEXT NOT NULL, modifiers TEXT NOT NULL,
              param_count INTEGER NOT NULL, body_sha256 TEXT NOT NULL,
              normalized_sha256 TEXT NOT NULL,
              start_line INTEGER NOT NULL, end_line INTEGER NOT NULL
            );
            """
        )
        conn.executemany(
            "INSERT INTO meta(key,value) VALUES(?,?)",
            [
                ("schema_version", "1"),
                ("atlas_version", "0.1.1"),
                ("fingerprint_algorithm", "java-token-v2-literal-sensitive"),
                ("minecraft_version", "26.2"),
                ("world_version", "4903"),
                ("protocol_version", "776"),
                ("source_archive_sha256", "source-sha"),
            ],
        )
        conn.execute(
            "INSERT INTO source_files(id,path,sha256) VALUES(1,?,?)",
            ("src/example/State.java", "file-sha"),
        )
        conn.execute(
            """
            INSERT INTO types(
              id,file_id,qualified_name,kind,normalized_sha256,start_line,end_line
            ) VALUES(1,1,?,?,?,?,?)
            """,
            ("example.State", "class", "type-sha", 3, 50),
        )
        conn.execute(
            """
            INSERT INTO fields(id,type_id,name,type_name,modifiers,line)
            VALUES(1,1,'REGISTRY','IdMapper<State>','static final',7)
            """
        )
        conn.execute(
            """
            INSERT INTO methods(
              id,type_id,name,signature,return_type,modifiers,param_count,
              body_sha256,normalized_sha256,start_line,end_line
            ) VALUES(1,1,'isAir','isAir()','boolean','public',0,
                     'body-sha','method-sha',10,12)
            """
        )
        conn.commit()
        conn.close()
        return lock, spec, atlas

    def test_qualification_binds_source_and_fingerprints(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            lock, spec, atlas = self.make_fixture(Path(td))
            first = qualification.qualify(lock, spec, atlas)
            second = qualification.qualify(lock, spec, atlas)

            self.assertEqual(first, second)
            self.assertEqual(first["source"]["archive_sha256"], "source-sha")
            self.assertEqual(len(first["evidence"]), 3)
            method = first["evidence"][2]["surface"]
            self.assertEqual(method["body_sha256"], "body-sha")
            self.assertEqual(method["normalized_sha256"], "method-sha")
            self.assertEqual(
                first["qualification_digest"],
                qualification.sha256_bytes(
                    qualification.canonical_json_bytes(
                        {key: value for key, value in first.items() if key != "qualification_digest"}
                    )
                ),
            )

    def test_source_archive_mismatch_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            lock, spec, atlas = self.make_fixture(Path(td))
            conn = sqlite3.connect(atlas)
            conn.execute(
                "UPDATE meta SET value='different-source' WHERE key='source_archive_sha256'"
            )
            conn.commit()
            conn.close()
            with self.assertRaisesRegex(ValueError, "Atlas source archive mismatch"):
                qualification.qualify(lock, spec, atlas)

    def test_missing_method_locator_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            lock, spec, atlas = self.make_fixture(Path(td))
            conn = sqlite3.connect(atlas)
            conn.execute("DELETE FROM methods")
            conn.commit()
            conn.close()
            with self.assertRaisesRegex(ValueError, "expected exactly one Atlas method"):
                qualification.qualify(lock, spec, atlas)

    def test_ambiguous_method_locator_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            lock, spec, atlas = self.make_fixture(Path(td))
            conn = sqlite3.connect(atlas)
            conn.execute(
                """
                INSERT INTO methods(
                  id,type_id,name,signature,return_type,modifiers,param_count,
                  body_sha256,normalized_sha256,start_line,end_line
                ) VALUES(2,1,'isAir','isAir()','boolean','public',0,
                         'body-sha-2','method-sha-2',20,22)
                """
            )
            conn.commit()
            conn.close()
            with self.assertRaisesRegex(ValueError, "found 2"):
                qualification.qualify(lock, spec, atlas)


if __name__ == "__main__":
    unittest.main()
