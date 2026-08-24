from __future__ import annotations

import hashlib
import importlib.util
import json
import sys
import tempfile
import tomllib
import unittest
import zipfile
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
MODULE_PATH = TOOLS / "vanilla_source_excerpt.py"
spec = importlib.util.spec_from_file_location("vanilla_source_excerpt", MODULE_PATH)
assert spec and spec.loader
excerpt = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = excerpt
spec.loader.exec_module(excerpt)


class VanillaSourceExcerptTests(unittest.TestCase):
    def _fixture(self, root: Path) -> tuple[Path, Path, dict[str, bytes]]:
        files = {
            "src/net/minecraft/test/A.java": b"package net.minecraft.test;\nclass A {\n}\n",
            "src/net/minecraft/test/B.java": b"package net.minecraft.test;\nclass B {}\n",
        }
        source = root / "mc-src.zip"
        with zipfile.ZipFile(source, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.writestr(
                "src/version.json",
                json.dumps({"id": "test", "protocol_version": 7, "world_version": 11}),
            )
            for path, raw in files.items():
                archive.writestr(path, raw)

        archive_sha256 = hashlib.sha256(source.read_bytes()).hexdigest()
        lock = root / "vanilla.lock.toml"
        lock.write_text(
            "\n".join(
                [
                    "schema = 1",
                    'minecraft = "test"',
                    "protocol = 7",
                    "data_version = 11",
                    "",
                    "[source]",
                    'kind = "local-official-source-corpus"',
                    f'archive_sha256 = "{archive_sha256}"',
                    "java_files = 2",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        return source, lock, files

    def test_exact_excerpt_is_identity_bound_deterministic_and_line_numbered(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            source, lock, files = self._fixture(Path(td))
            rendered = excerpt.render_excerpt(
                source,
                lock,
                ["src/net/minecraft/test/B.java", "src/net/minecraft/test/A.java"],
            )
            self.assertIn("kind: vanilla-source-excerpt-v1", rendered)
            self.assertIn("minecraft: test", rendered)
            self.assertIn("protocol: 7", rendered)
            self.assertIn("data_version: 11", rendered)
            self.assertLess(
                rendered.index("src/net/minecraft/test/A.java"),
                rendered.index("src/net/minecraft/test/B.java"),
            )
            self.assertIn("    1: package net.minecraft.test;", rendered)
            self.assertIn("    2: class A {", rendered)
            for path, raw in files.items():
                self.assertIn(path, rendered)
                self.assertIn(f"sha256: {hashlib.sha256(raw).hexdigest()}", rendered)
            self.assertEqual(
                rendered,
                excerpt.render_excerpt(
                    source,
                    lock,
                    ["src/net/minecraft/test/B.java", "src/net/minecraft/test/A.java"],
                ),
            )

    def test_archive_identity_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            source, lock, _files = self._fixture(root)
            parsed = tomllib.loads(lock.read_text(encoding="utf-8"))
            lock.write_text(
                "\n".join(
                    [
                        "schema = 1",
                        'minecraft = "test"',
                        "protocol = 7",
                        "data_version = 11",
                        "",
                        "[source]",
                        'kind = "local-official-source-corpus"',
                        f'archive_sha256 = "{"0" * 64}"',
                        f"java_files = {parsed['source']['java_files']}",
                        "",
                    ]
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(excerpt.ExcerptError, "source identity mismatch"):
                excerpt.render_excerpt(source, lock, ["src/net/minecraft/test/A.java"])

    def test_missing_duplicate_and_non_src_paths_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            source, lock, _files = self._fixture(Path(td))
            with self.assertRaisesRegex(excerpt.ExcerptError, "missing requested paths"):
                excerpt.render_excerpt(source, lock, ["src/net/minecraft/test/Missing.java"])
            with self.assertRaisesRegex(excerpt.ExcerptError, "duplicate source paths"):
                excerpt.render_excerpt(
                    source,
                    lock,
                    ["src/net/minecraft/test/A.java", "src/net/minecraft/test/A.java"],
                )
            with self.assertRaisesRegex(excerpt.ExcerptError, "exact src/"):
                excerpt.render_excerpt(source, lock, ["net/minecraft/test/A.java"])


if __name__ == "__main__":
    unittest.main()
