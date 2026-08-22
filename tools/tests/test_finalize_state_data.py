from __future__ import annotations

import json
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PIPELINE = ROOT / "tools" / "finalize_state_data.py"


class FinalizeStateDataTests(unittest.TestCase):
    def write_fixture(self, root: Path) -> dict[str, Path]:
        lock = root / "vanilla.lock.toml"
        lock.write_text(
            """schema = 1
minecraft = "26.2"
protocol = 776
data_version = 4903

[source]
archive_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
java_files = 1

[runtime]
kind = "official-server"
probe = "official-runtime-reflection-probe-v1"
server_sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

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
                            "id": "STATE-SOURCE-NON-AIR",
                            "kind": "method",
                            "owner": "example.State",
                            "name": "isAir",
                            "param_count": 0,
                            "classification": "SEMANTIC_TARGET_DATA",
                            "role": "air predicate",
                        }
                    ],
                },
                indent=2,
                sort_keys=True,
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
                (
                    "source_archive_sha256",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ),
            ],
        )
        conn.execute(
            "INSERT INTO source_files(id,path,sha256) VALUES(1,?,?)",
            ("src/example/State.java", "e" * 64),
        )
        conn.execute(
            """
            INSERT INTO types(
              id,file_id,qualified_name,kind,normalized_sha256,start_line,end_line
            ) VALUES(1,1,'example.State','class',?,1,20)
            """,
            ("f" * 64,),
        )
        conn.execute(
            """
            INSERT INTO methods(
              id,type_id,name,signature,return_type,modifiers,param_count,
              body_sha256,normalized_sha256,start_line,end_line
            ) VALUES(1,1,'isAir','isAir()','boolean','public',0,?,?,5,7)
            """,
            ("c" * 64, "d" * 64),
        )
        conn.commit()
        conn.close()

        runtime = root / "runtime.json"
        runtime.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "target": {
                        "minecraft_version": "26.2",
                        "protocol_version": 776,
                        "data_version": 4903,
                    },
                    "air_key": "minecraft:air",
                    "provenance": {
                        "server_sha256": "b" * 64,
                        "server_mappings_sha256": None,
                        "name_mapping": "identity-unobfuscated",
                        "source": "official-runtime-reflection-probe-v1",
                        "startup_sequence": [
                            "SharedConstants.tryDetectVersion",
                            "Bootstrap.bootStrap",
                        ],
                    },
                    "states": [
                        {
                            "key": "minecraft:air",
                            "vanilla_id": 0,
                            "non_air": False,
                            "counted_fluid": False,
                            "random_block": False,
                            "random_fluid": False,
                        },
                        {
                            "key": "minecraft:stone",
                            "vanilla_id": 1,
                            "non_air": True,
                            "counted_fluid": False,
                            "random_block": False,
                            "random_fluid": False,
                        },
                    ],
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

        return {
            "lock": lock,
            "spec": spec,
            "atlas": atlas,
            "runtime": runtime,
            "source": root / "source-qualification.json",
            "raw": root / "raw.json",
            "qualified": root / "qualified.json",
            "rust": root / "generated" / "lib.rs",
            "manifest": root / "manifest.json",
        }

    def command(self, paths: dict[str, Path], verify: bool = False) -> list[str]:
        result = [
            sys.executable,
            str(PIPELINE),
            "--lock",
            str(paths["lock"]),
            "--spec",
            str(paths["spec"]),
            "--atlas",
            str(paths["atlas"]),
            "--source-qualification",
            str(paths["source"]),
            "--runtime-data",
            str(paths["runtime"]),
            "--raw-runtime",
            str(paths["raw"]),
            "--qualified-runtime",
            str(paths["qualified"]),
            "--rust-output",
            str(paths["rust"]),
            "--manifest",
            str(paths["manifest"]),
        ]
        if verify:
            result.append("--verify")
        return result

    def test_pipeline_generates_and_verifies_complete_chain(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            paths = self.write_fixture(Path(td))
            generated = subprocess.run(
                self.command(paths),
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(generated.returncode, 0, generated.stderr + generated.stdout)
            self.assertTrue(paths["source"].is_file())
            self.assertTrue(paths["qualified"].is_file())
            self.assertTrue(paths["rust"].is_file())
            self.assertTrue(paths["manifest"].is_file())

            manifest = json.loads(paths["manifest"].read_text(encoding="utf-8"))
            self.assertEqual(manifest["state_count"], 2)
            self.assertEqual(manifest["repr"], "u16")
            self.assertEqual(manifest["mapping"], "identity")
            self.assertEqual(
                manifest["source_provenance"]["qualification"],
                "source+official-runtime",
            )

            verified = subprocess.run(
                self.command(paths, verify=True),
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(verified.returncode, 0, verified.stderr + verified.stdout)

    def test_verify_rejects_changed_source_fingerprint(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            paths = self.write_fixture(Path(td))
            generated = subprocess.run(
                self.command(paths),
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(generated.returncode, 0, generated.stderr + generated.stdout)

            conn = sqlite3.connect(paths["atlas"])
            conn.execute(
                "UPDATE methods SET normalized_sha256=? WHERE id=1",
                ("9" * 64,),
            )
            conn.commit()
            conn.close()

            verified = subprocess.run(
                self.command(paths, verify=True),
                cwd=ROOT,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertNotEqual(verified.returncode, 0)
            self.assertIn("qualification artifact differs", verified.stderr + verified.stdout)


if __name__ == "__main__":
    unittest.main()
