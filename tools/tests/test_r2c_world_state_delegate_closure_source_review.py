from __future__ import annotations

import json
import sqlite3
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

from tools import r2c_world_state_delegate_closure_source_review as closure


class R2cWorldStateDelegateClosureTests(unittest.TestCase):
    def test_committed_plan_is_narrow_and_canonical(self) -> None:
        plan = closure._load_plan()
        self.assertEqual(
            [(group.group_id, group.parent_group_id) for group in plan.groups],
            list(closure.EXPECTED_GROUPS),
        )
        biome_types = {
            selector.type_name
            for selector in plan.groups[0].selectors
            if selector.type_name is not None
        }
        self.assertIn("net.minecraft.world.level.chunk.PalettedContainer$Data", biome_types)
        self.assertIn("net.minecraft.world.level.chunk.SingleValuePalette", biome_types)
        self.assertIn("net.minecraft.world.level.chunk.GlobalPalette", biome_types)

        light_types = {
            selector.type_name
            for selector in plan.groups[1].selectors
            if selector.type_name is not None
        }
        self.assertIn("net.minecraft.world.level.chunk.DataLayer", light_types)
        self.assertIn(
            "net.minecraft.world.level.lighting.LayerLightEventListener",
            light_types,
        )

    def test_source_rich_output_inside_repository_is_rejected(self) -> None:
        path = closure.REPO_ROOT / "delegate-closure-should-not-exist.tar.gz"
        with self.assertRaisesRegex(closure.ClosureError, "outside the repository"):
            closure._external_output(path)

    def test_required_type_names_selector_fails_closed(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        conn.executescript(
            """
            CREATE TABLE source_files(id INTEGER PRIMARY KEY, path TEXT NOT NULL);
            CREATE TABLE types(id INTEGER PRIMARY KEY, qualified_name TEXT NOT NULL, file_id INTEGER NOT NULL);
            CREATE TABLE methods(
                id INTEGER PRIMARY KEY,
                type_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                signature TEXT NOT NULL,
                param_count INTEGER NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL
            );
            INSERT INTO source_files VALUES(1, 'src/net/minecraft/Test.java');
            INSERT INTO types VALUES(1, 'net.minecraft.Test', 1);
            INSERT INTO methods VALUES(1, 1, 'present', 'present()', 0, 3, 5);
            """
        )
        selector = closure.Selector(
            mode="type_names",
            required=True,
            type_name="net.minecraft.Test",
            names=("present", "missing"),
        )
        with self.assertRaisesRegex(closure.ClosureError, "required names missing"):
            closure._query_selector(conn, selector)
        conn.close()

    def test_source_excerpt_is_exact_and_bounded(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            archive_path = Path(temporary) / "source.zip"
            with zipfile.ZipFile(archive_path, mode="w") as archive:
                archive.writestr(
                    "src/net/minecraft/Test.java",
                    "one\n"
                    "two\n"
                    "three\n"
                    "four\n",
                )
            with zipfile.ZipFile(archive_path) as archive:
                row = {
                    "qualified_name": "net.minecraft.Test",
                    "signature": "twoThree()",
                    "path": "src/net/minecraft/Test.java",
                    "start_line": 2,
                    "end_line": 3,
                }
                excerpt = closure._source_excerpt(
                    archive,
                    row,
                    max_lines=2,
                    cache={},
                )
                self.assertEqual(excerpt, b"two\nthree\n")
                with self.assertRaisesRegex(closure.ClosureError, "review bound"):
                    closure._source_excerpt(
                        archive,
                        {**row, "start_line": 1, "end_line": 3},
                        max_lines=2,
                        cache={},
                    )

    def test_source_text_is_confined_to_review_pack(self) -> None:
        plan = closure.Plan(
            max_candidate_methods=4,
            max_candidate_lines=32,
            max_source_bytes=4096,
            groups=(
                closure.Group(
                    "R2C-BIOME-PALETTE-WIRE",
                    "R2C-BIOMES",
                    "biome focus",
                    (),
                ),
                closure.Group(
                    "R2C-LIGHT-DATA-LAYER",
                    "R2C-LIGHT",
                    "light focus",
                    (),
                ),
            ),
        )
        records = [
            {
                "candidate_id": "DISC-NET-R2C-WORLD-DELEGATE-0001",
                "source_identity": "net.minecraft.Biome#write()",
                "source": {
                    "type": "net.minecraft.Biome",
                    "signature": "write()",
                    "fingerprint_algorithm": "java-token-v2-literal-sensitive",
                    "normalized_sha256": "a" * 64,
                    "body_sha256": "b" * 64,
                },
                "source_location": {
                    "path": "src/net/minecraft/Biome.java",
                    "start_line": 1,
                    "end_line": 1,
                },
                "atlas_observed_hazards": ["CODEC"],
                "atlas_classifications": [],
                "calls": {
                    "call_sites": 0,
                    "resolved_targets": [],
                    "unresolved_call_sites": 0,
                    "top_unresolved_callees": [],
                },
                "group_ids": ["R2C-BIOME-PALETTE-WIRE"],
                "review_focus": ["biome focus"],
                "source_excerpt": "SECRET_SOURCE_LINE\n",
                "source_excerpt_sha256": "c" * 64,
            }
        ]
        payloads = closure._payloads(
            plan=plan,
            plan_sha256="1" * 64,
            parent_plan_sha256="2" * 64,
            frontier_sha256="3" * 64,
            source_sha256=closure.EXPECTED_SOURCE_SHA256,
            records=records,
        )
        self.assertIn(b"SECRET_SOURCE_LINE", payloads["review-pack.json"])
        self.assertNotIn(b"SECRET_SOURCE_LINE", payloads["worksheet.json"])
        self.assertNotIn(b"SECRET_SOURCE_LINE", payloads["manifest.json"])

        worksheet = json.loads(payloads["worksheet.json"])
        self.assertFalse(worksheet["contains_official_source_text"])
        self.assertFalse(worksheet["production_admitted"])
        self.assertEqual(
            worksheet["groups"][0]["candidates"][0]["source_identity"],
            "net.minecraft.Biome#write()",
        )

    def test_archive_has_exact_regular_file_surface(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "closure.tar.gz"
            payloads = {
                "review-pack.json": b'{"source":"rich"}\n',
                "worksheet.json": b'{"source":"free"}\n',
                "manifest.json": b'{"manifest":1}\n',
            }
            closure._write_archive(output, payloads)
            with tarfile.open(output, mode="r:gz") as archive:
                self.assertEqual(
                    {member.name for member in archive.getmembers()},
                    set(payloads),
                )
                self.assertTrue(all(member.isfile() for member in archive.getmembers()))


if __name__ == "__main__":
    unittest.main()
