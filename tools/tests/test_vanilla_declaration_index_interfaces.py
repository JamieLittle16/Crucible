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
package net.minecraft.network.codec;

public interface Codecs {
    Codec<String> PLAYER_NAME = stringUtf8(16);
    Codec<Profile> GAME_PROFILE = composite(PLAYER_NAME, Profile::name, Profile::new);

    static Codec<String> stringUtf8(int maxLength) {
        return codec(maxLength);
    }

    default String description() {
        return "not class initialization";
    }
}
'''


class InterfaceDeclarationIndexTests(unittest.TestCase):
    def _archive(self, root: Path, *, source: str = SOURCE, name: str = "source.zip") -> Path:
        archive = root / name
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
            zf.writestr(
                "src/version.json",
                json.dumps({"id": "test", "world_version": 1, "protocol_version": 2}),
            )
            zf.writestr("src/net/minecraft/network/codec/Codecs.java", source)
        return archive

    def _fingerprint(self, archive: Path, db: Path) -> tuple[str, str]:
        atlas.index_archive(archive, db, None, None)
        report = declarations.index_declarations(archive, db, check=False)
        self.assertEqual(report["synthetic_clinit"], 1)
        conn = sqlite3.connect(db)
        row = conn.execute(
            """SELECT m.normalized_sha256,m.body_sha256
               FROM methods m JOIN types t ON t.id=m.type_id
               WHERE t.qualified_name=? AND m.signature='<clinit>()'""",
            ("net.minecraft.network.codec.Codecs",),
        ).fetchone()
        conn.close()
        self.assertIsNotNone(row)
        assert row is not None
        return str(row[0]), str(row[1])

    def test_interface_fields_are_implicitly_static_class_initialization(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            archive = self._archive(root)
            db = root / "atlas.sqlite"
            normalized, body = self._fingerprint(archive, db)
            self.assertEqual(len(normalized), 64)
            self.assertEqual(len(body), 64)

            conn = atlas.connect_db(db)
            matches = atlas.resolve_methods(
                conn, "net.minecraft.network.codec.Codecs#<clinit>()"
            )
            conn.close()
            self.assertEqual(len(matches), 1)

    def test_implicit_interface_field_literals_are_fingerprint_significant(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            first = self._archive(root, name="first.zip")
            first_fp = self._fingerprint(first, root / "first.sqlite")[0]

            changed = self._archive(
                root,
                source=SOURCE.replace("stringUtf8(16)", "stringUtf8(32)"),
                name="changed.zip",
            )
            changed_fp = self._fingerprint(changed, root / "changed.sqlite")[0]
            self.assertNotEqual(first_fp, changed_fp)


if __name__ == "__main__":
    unittest.main()
