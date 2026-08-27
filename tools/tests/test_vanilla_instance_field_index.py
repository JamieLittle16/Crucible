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
fields = _load("vanilla_instance_field_index", TOOLS / "vanilla_instance_field_index.py")

SOURCE = r'''
package net.minecraft.server.test;

public final class Holder {
    private final Object ordinary = create("instance");
    private static final Object STATIC = create("static");
    private final Object noInitializer;
    private final Handler synchronizer = new Handler() {
        @Override
        public void sendInitialData() {
            emit(new ClientboundExamplePacket("initial"));
        }
    };

    public void method() {
        Object local = create("local");
    }

    public static final class Nested {
        private final Object nested = create("nested");
    }
}
'''


class InstanceFieldIndexTests(unittest.TestCase):
    def _archive(self, root: Path, *, source: str = SOURCE, name: str = "source.zip") -> Path:
        archive = root / name
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
            zf.writestr(
                "src/version.json",
                json.dumps({"id": "test", "world_version": 1, "protocol_version": 2}),
            )
            zf.writestr("src/net/minecraft/server/test/Holder.java", source)
        return archive

    def test_indexes_only_initialized_instance_fields_and_preserves_anonymous_body(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = self._archive(root)
            db = root / "atlas.sqlite"
            atlas.index_archive(archive, db, None, None)

            report = fields.index_instance_fields(archive, db, check=False)
            self.assertEqual(report["synthetic_field_initializers"], 3)
            self.assertEqual(report["observable_field_initializers"], 1)

            conn = sqlite3.connect(db)
            rows = conn.execute(
                """SELECT t.qualified_name,m.name,m.signature,m.normalized_sha256,m.body_sha256
                   FROM methods m JOIN types t ON t.id=m.type_id
                   WHERE m.modifiers=? ORDER BY t.qualified_name,m.name""",
                (fields.SYNTHETIC_MODIFIERS,),
            ).fetchall()
            self.assertEqual(
                [(row[0], row[1]) for row in rows],
                [
                    ("net.minecraft.server.test.Holder", "<fieldinit:ordinary>"),
                    ("net.minecraft.server.test.Holder", "<fieldinit:synchronizer>"),
                    ("net.minecraft.server.test.Holder$Nested", "<fieldinit:nested>"),
                ],
            )
            self.assertTrue(all(row[2] == f"{row[1]}()" for row in rows))
            self.assertTrue(all(len(row[3]) == 64 and len(row[4]) == 64 for row in rows))
            self.assertFalse(any(row[1] == "<fieldinit:STATIC>" for row in rows))
            self.assertFalse(any(row[1] == "<fieldinit:noInitializer>" for row in rows))

            synchronizer_id = conn.execute(
                """SELECT m.id FROM methods m JOIN types t ON t.id=m.type_id
                   WHERE t.qualified_name=? AND m.signature=?""",
                (
                    "net.minecraft.server.test.Holder",
                    "<fieldinit:synchronizer>()",
                ),
            ).fetchone()[0]
            hazards = {
                row[0]
                for row in conn.execute(
                    "SELECT kind FROM hazards WHERE method_id=?", (synchronizer_id,)
                )
            }
            self.assertIn("CLIENT_OBSERVABLE", hazards)
            self.assertIn("NETWORK_SEND", hazards)
            conn.close()

            atlas_conn = atlas.connect_db(db)
            self.assertEqual(
                len(
                    atlas.resolve_methods(
                        atlas_conn,
                        "net.minecraft.server.test.Holder#<fieldinit:synchronizer>()",
                    )
                ),
                1,
            )
            atlas_conn.close()

    def test_anonymous_initializer_literal_is_fingerprint_significant(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            first_archive = self._archive(root, name="first.zip")
            first_db = root / "first.sqlite"
            atlas.index_archive(first_archive, first_db, None, None)
            fields.index_instance_fields(first_archive, first_db, check=False)
            first = sqlite3.connect(first_db).execute(
                """SELECT m.normalized_sha256 FROM methods m JOIN types t ON t.id=m.type_id
                   WHERE t.qualified_name=? AND m.signature=?""",
                ("net.minecraft.server.test.Holder", "<fieldinit:synchronizer>()"),
            ).fetchone()[0]

            changed = SOURCE.replace('ClientboundExamplePacket("initial")', 'ClientboundExamplePacket("changed")')
            second_archive = self._archive(root, source=changed, name="second.zip")
            second_db = root / "second.sqlite"
            atlas.index_archive(second_archive, second_db, None, None)
            fields.index_instance_fields(second_archive, second_db, check=False)
            second = sqlite3.connect(second_db).execute(
                """SELECT m.normalized_sha256 FROM methods m JOIN types t ON t.id=m.type_id
                   WHERE t.qualified_name=? AND m.signature=?""",
                ("net.minecraft.server.test.Holder", "<fieldinit:synchronizer>()"),
            ).fetchone()[0]
            self.assertNotEqual(first, second)

    def test_index_is_idempotent_and_check_detects_no_drift(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = self._archive(root)
            db = root / "atlas.sqlite"
            atlas.index_archive(archive, db, None, None)
            fields.index_instance_fields(archive, db, check=False)
            first = fields.index_instance_fields(archive, db, check=True)
            fields.index_instance_fields(archive, db, check=False)
            second = fields.index_instance_fields(archive, db, check=True)
            self.assertEqual(
                first["synthetic_field_initializers"], second["synthetic_field_initializers"]
            )

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
                zf.writestr("src/net/minecraft/server/test/Holder.java", SOURCE)
            with self.assertRaises(fields.InstanceFieldIndexError):
                fields.index_instance_fields(other, db, check=False)


if __name__ == "__main__":
    unittest.main()
