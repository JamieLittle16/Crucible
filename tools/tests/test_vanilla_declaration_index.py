from __future__ import annotations

import importlib.util
import json
import sqlite3
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]


def _load(name: str, path: Path):  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


atlas = _load("vanilla_atlas", TOOLS / "vanilla_atlas.py")
declarations = _load("vanilla_declaration_index", TOOLS / "vanilla_declaration_index.py")

SOURCE = r'''
package net.minecraft.network.protocol.test;

public final class Protocols {
    public static final Object FIRST = create("first");
    private final Object ordinary = create("instance");
    public static final Object LAMBDA = factory(() -> {
        consume("inside");
    });

    static {
        register(FIRST);
    }

    public static Object helper() {
        return create("method");
    }

    public static final class Nested {
        static final Object VALUE = create("nested");
    }
}

enum Intent {
    STATUS(1),
    LOGIN(2) {
        @Override
        public String toString() {
            return "login";
        }
    };

    private final int id;

    Intent(int id) {
        this.id = id;
    }
}
'''


class DeclarationIndexTests(unittest.TestCase):
    def _archive(self, root: Path, *, source: str = SOURCE, name: str = "source.zip") -> Path:
        archive = root / name
        version = {
            "id": "test",
            "world_version": 1,
            "protocol_version": 2,
        }
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
            zf.writestr("src/version.json", json.dumps(version))
            zf.writestr("src/net/minecraft/network/protocol/test/Protocols.java", source)
        return archive

    def test_class_initialization_is_one_reviewable_node_per_type(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = self._archive(root)
            db = root / "atlas.sqlite"
            atlas.index_archive(archive, db, None, None)

            report = declarations.index_declarations(archive, db, check=False)
            self.assertEqual(report["synthetic_clinit"], 3)
            self.assertEqual(report["protocol_synthetic_clinit"], 3)

            conn = sqlite3.connect(db)
            rows = conn.execute(
                """SELECT t.qualified_name,m.signature,m.normalized_sha256,m.body_sha256
                   FROM methods m JOIN types t ON t.id=m.type_id
                   WHERE m.name=? ORDER BY t.qualified_name""",
                (declarations.SYNTHETIC_NAME,),
            ).fetchall()
            self.assertEqual(
                [row[0] for row in rows],
                [
                    "net.minecraft.network.protocol.test.Intent",
                    "net.minecraft.network.protocol.test.Protocols",
                    "net.minecraft.network.protocol.test.Protocols$Nested",
                ],
            )
            self.assertTrue(all(row[1] == "<clinit>()" for row in rows))
            self.assertTrue(all(len(row[2]) == 64 and len(row[3]) == 64 for row in rows))

            outer_id = conn.execute(
                """SELECT m.id FROM methods m JOIN types t ON t.id=m.type_id
                   WHERE t.qualified_name=? AND m.signature='<clinit>()'""",
                ("net.minecraft.network.protocol.test.Protocols",),
            ).fetchone()[0]
            hazards = {
                row[0]
                for row in conn.execute(
                    "SELECT kind FROM hazards WHERE method_id=?",
                    (outer_id,),
                )
            }
            self.assertIn("CLIENT_OBSERVABLE", hazards)
            conn.close()

            # The exact class-initializer identity is consumable by unchanged Atlas lookup/VAR
            # tooling instead of being hidden in an unreviewable declaration.
            conn = atlas.connect_db(db)
            for query in (
                "net.minecraft.network.protocol.test.Protocols#<clinit>()",
                "net.minecraft.network.protocol.test.Intent#<clinit>()",
            ):
                self.assertEqual(len(atlas.resolve_methods(conn, query)), 1)
            conn.close()

    def test_enum_constant_literals_are_fingerprint_significant(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            first_archive = self._archive(root, name="first.zip")
            first_db = root / "first.sqlite"
            atlas.index_archive(first_archive, first_db, None, None)
            declarations.index_declarations(first_archive, first_db, check=False)
            first_conn = sqlite3.connect(first_db)
            first_hash = first_conn.execute(
                """SELECT m.normalized_sha256 FROM methods m JOIN types t ON t.id=m.type_id
                   WHERE t.qualified_name=? AND m.signature='<clinit>()'""",
                ("net.minecraft.network.protocol.test.Intent",),
            ).fetchone()[0]
            first_conn.close()

            changed_source = SOURCE.replace("STATUS(1)", "STATUS(7)")
            changed_archive = self._archive(root, source=changed_source, name="changed.zip")
            changed_db = root / "changed.sqlite"
            atlas.index_archive(changed_archive, changed_db, None, None)
            declarations.index_declarations(changed_archive, changed_db, check=False)
            changed_conn = sqlite3.connect(changed_db)
            changed_hash = changed_conn.execute(
                """SELECT m.normalized_sha256 FROM methods m JOIN types t ON t.id=m.type_id
                   WHERE t.qualified_name=? AND m.signature='<clinit>()'""",
                ("net.minecraft.network.protocol.test.Intent",),
            ).fetchone()[0]
            changed_conn.close()

            self.assertNotEqual(first_hash, changed_hash)

    def test_index_is_idempotent_and_check_detects_no_drift(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = self._archive(root)
            db = root / "atlas.sqlite"
            atlas.index_archive(archive, db, None, None)

            declarations.index_declarations(archive, db, check=False)
            first = declarations.index_declarations(archive, db, check=True)
            declarations.index_declarations(archive, db, check=False)
            second = declarations.index_declarations(archive, db, check=True)

            self.assertEqual(first["synthetic_clinit"], second["synthetic_clinit"])
            conn = sqlite3.connect(db)
            count = conn.execute(
                "SELECT COUNT(*) FROM methods WHERE name=?",
                (declarations.SYNTHETIC_NAME,),
            ).fetchone()[0]
            self.assertEqual(count, 3)
            conn.close()

    def test_source_identity_mismatch_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = self._archive(root)
            db = root / "atlas.sqlite"
            atlas.index_archive(archive, db, None, None)

            other = root / "other.zip"
            with zipfile.ZipFile(other, "w", zipfile.ZIP_DEFLATED) as zf:
                zf.writestr(
                    "src/version.json",
                    json.dumps({"id": "other", "world_version": 1, "protocol_version": 2}),
                )
                zf.writestr(
                    "src/net/minecraft/network/protocol/test/Protocols.java",
                    SOURCE,
                )

            with self.assertRaises(declarations.DeclarationIndexError):
                declarations.index_declarations(other, db, check=False)


if __name__ == "__main__":
    unittest.main()
