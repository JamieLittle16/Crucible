from __future__ import annotations

import hashlib
import json
import sqlite3
import tempfile
import unittest
import zipfile
from pathlib import Path

from tools import r1b_configuration_review as review
from tools import r1b_configuration_review_dossier as dossier
from tools import r1b_configuration_source_probe as source_probe


REPO_ROOT = Path(__file__).resolve().parents[2]
PLAN_PATH = REPO_ROOT / "vanilla/reviews/network/r1b-configuration-review-plan.json"
SEMANTICS_PATH = REPO_ROOT / "vanilla/semantics/network/R1_CONFIGURATION_SEMANTICS.md"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_lock(path: Path, source_sha: str) -> None:
    path.write_text(
        f'''schema = 1
minecraft = "26.2"
protocol = 776

[source]
archive_sha256 = "{source_sha}"

[atlas]
version = "0.1.1"
fingerprint_algorithm = "java-token-v2-literal-sensitive"
''',
        encoding="utf-8",
    )


def query_parts(query: str) -> tuple[str, str, str]:
    owner, member = query.split("#", 1)
    if member == "<clinit>()":
        return owner, "<clinit>", member
    return owner, member, f"{member}()"


def create_fixture(root: Path) -> tuple[Path, Path, Path]:
    source = root / "mc-src.zip"
    source_members: dict[str, str] = {}
    for index, (var_id, _query) in enumerate(source_probe.CANDIDATES):
        path = f"src/net/minecraft/synthetic/Candidate{index}.java"
        source_members[path] = f"// synthetic header {index}\nSOURCE {var_id}\n// synthetic tail {index}\n"
    with zipfile.ZipFile(source, "w", compression=zipfile.ZIP_STORED) as archive:
        for path, text in source_members.items():
            archive.writestr(path, text)
    source_sha = sha256(source)

    lock = root / "vanilla.lock.toml"
    write_lock(lock, source_sha)

    db = root / "atlas.sqlite"
    conn = sqlite3.connect(db)
    conn.execute("CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)")
    conn.executemany(
        "INSERT INTO meta(key,value) VALUES(?,?)",
        (
            ("source_archive_sha256", source_sha),
            ("minecraft_version", "26.2"),
            ("protocol_version", "776"),
            ("atlas_version", "0.1.1"),
            ("fingerprint_algorithm", "java-token-v2-literal-sensitive"),
        ),
    )
    conn.execute("CREATE TABLE source_files(id INTEGER PRIMARY KEY, path TEXT NOT NULL)")
    conn.execute(
        "CREATE TABLE types(id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL, qualified_name TEXT NOT NULL)"
    )
    conn.execute(
        """CREATE TABLE methods(
            id INTEGER PRIMARY KEY,
            type_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            signature TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            normalized_sha256 TEXT NOT NULL,
            body_sha256 TEXT NOT NULL
        )"""
    )
    conn.execute("CREATE TABLE hazards(method_id INTEGER NOT NULL, kind TEXT NOT NULL)")
    for index, (_var_id, query) in enumerate(source_probe.CANDIDATES):
        owner, name, signature = query_parts(query)
        row_id = index + 1
        path = f"src/net/minecraft/synthetic/Candidate{index}.java"
        conn.execute("INSERT INTO source_files(id,path) VALUES(?,?)", (row_id, path))
        conn.execute(
            "INSERT INTO types(id,file_id,qualified_name) VALUES(?,?,?)",
            (row_id, row_id, owner),
        )
        conn.execute(
            """INSERT INTO methods(
                id,type_id,name,signature,start_line,end_line,normalized_sha256,body_sha256
            ) VALUES(?,?,?,?,?,?,?,?)""",
            (
                row_id,
                row_id,
                name,
                signature,
                2,
                2,
                hashlib.sha256(f"normalized-{index}".encode()).hexdigest(),
                hashlib.sha256(f"body-{index}".encode()).hexdigest(),
            ),
        )
    conn.execute("INSERT INTO hazards(method_id,kind) VALUES(?,?)", (1, "CODEC"))
    conn.commit()
    conn.close()
    return db, source, lock


class R1BConfigurationReviewDossierTests(unittest.TestCase):
    def test_build_dossier_extracts_all_exact_indexed_source_spans(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            db, source, lock = create_fixture(root)
            result = dossier.build_dossier(
                db=db,
                source_archive=source,
                lock_path=lock,
                plan_path=PLAN_PATH,
                semantics_path=SEMANTICS_PATH,
            )
            self.assertEqual(result["kind"], dossier.DOSSIER_KIND)
            self.assertEqual(result["commit_policy"], "EPHEMERAL_DO_NOT_COMMIT")
            self.assertTrue(result["contains_official_source_text"])
            self.assertEqual(result["source_archive_sha256"], sha256(source))
            self.assertEqual(len(result["candidates"]), len(source_probe.CANDIDATES))
            for index, candidate in enumerate(result["candidates"]):
                self.assertEqual(candidate["var_id"], source_probe.CANDIDATES[index][0])
                self.assertEqual(candidate["query"], source_probe.CANDIDATES[index][1])
                self.assertEqual(candidate["source_excerpt"], f"SOURCE {candidate['var_id']}\n")
                self.assertEqual(candidate["start_line"], 2)
                self.assertEqual(candidate["end_line"], 2)
                self.assertTrue(candidate["semantic_rule_candidates"])
                self.assertTrue(candidate["review_focus"])
            self.assertEqual(result["candidates"][0]["atlas_observed_hazards"], ["CODEC"])

    def test_dossier_writer_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            db, source, lock = create_fixture(root)
            result = dossier.build_dossier(
                db=db,
                source_archive=source,
                lock_path=lock,
                plan_path=PLAN_PATH,
                semantics_path=SEMANTICS_PATH,
            )
            output = root / "dossier.json"
            dossier.write_dossier(output, result)
            first = output.read_bytes()
            dossier.write_dossier(output, result)
            self.assertEqual(first, output.read_bytes())
            decoded = json.loads(first)
            self.assertTrue(decoded["contains_official_source_text"])
            self.assertEqual(decoded["commit_policy"], "EPHEMERAL_DO_NOT_COMMIT")

    def test_build_dossier_rejects_ambiguous_source_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            db, source, lock = create_fixture(root)
            owner, name, signature = query_parts(source_probe.CANDIDATES[0][1])
            conn = sqlite3.connect(db)
            conn.execute("INSERT INTO source_files(id,path) VALUES(?,?)", (1001, "src/net/minecraft/synthetic/Candidate0.java"))
            conn.execute(
                "INSERT INTO types(id,file_id,qualified_name) VALUES(?,?,?)",
                (1001, 1001, owner),
            )
            conn.execute(
                """INSERT INTO methods(
                    id,type_id,name,signature,start_line,end_line,normalized_sha256,body_sha256
                ) VALUES(?,?,?,?,?,?,?,?)""",
                (1001, 1001, name, signature, 2, 2, "a" * 64, "b" * 64),
            )
            conn.commit()
            conn.close()
            with self.assertRaisesRegex(dossier.DossierError, "resolve exactly once"):
                dossier.build_dossier(
                    db=db,
                    source_archive=source,
                    lock_path=lock,
                    plan_path=PLAN_PATH,
                    semantics_path=SEMANTICS_PATH,
                )

    def test_build_dossier_rejects_source_lock_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            db, source, lock = create_fixture(root)
            write_lock(lock, "0" * 64)
            with self.assertRaisesRegex(source_probe.ProbeError, "source archive SHA-256"):
                dossier.build_dossier(
                    db=db,
                    source_archive=source,
                    lock_path=lock,
                    plan_path=PLAN_PATH,
                    semantics_path=SEMANTICS_PATH,
                )

    def test_indexed_source_span_rejects_unsafe_archive_path(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            archive_path = Path(temporary) / "source.zip"
            with zipfile.ZipFile(archive_path, "w") as writer:
                writer.writestr("../escape.java", "danger\n")
            with zipfile.ZipFile(archive_path) as archive:
                with self.assertRaisesRegex(dossier.DossierError, "unsafe Atlas source path"):
                    dossier.extract_indexed_source_span(
                        archive,
                        {"path": "../escape.java", "start_line": 1, "end_line": 1},
                    )

    def test_indexed_source_span_rejects_out_of_range_lines(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            archive_path = Path(temporary) / "source.zip"
            with zipfile.ZipFile(archive_path, "w") as writer:
                writer.writestr("src/Example.java", "one\ntwo\n")
            with zipfile.ZipFile(archive_path) as archive:
                with self.assertRaisesRegex(dossier.DossierError, "exceeds member length"):
                    dossier.extract_indexed_source_span(
                        archive,
                        {"path": "src/Example.java", "start_line": 1, "end_line": 3},
                    )

    def test_cli_requires_explicit_output_path(self) -> None:
        parser = dossier.build_parser()
        with self.assertRaises(SystemExit):
            parser.parse_args([])
        args = parser.parse_args(["--output", "/tmp/r1b-review-dossier.json"])
        self.assertEqual(args.output, Path("/tmp/r1b-review-dossier.json"))
        self.assertEqual(args.plan, review.DEFAULT_PLAN)


if __name__ == "__main__":
    unittest.main()
