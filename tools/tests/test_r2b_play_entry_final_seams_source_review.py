from __future__ import annotations

import importlib.util
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]


def _load(name: str, path: Path):  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


seams = _load(
    "r2b_play_entry_final_seams_source_review",
    TOOLS / "r2b_play_entry_final_seams_source_review.py",
)


def _db() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.executescript(
        """
        CREATE TABLE source_files(id INTEGER PRIMARY KEY, path TEXT NOT NULL);
        CREATE TABLE types(id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL, qualified_name TEXT NOT NULL);
        CREATE TABLE methods(
            id INTEGER PRIMARY KEY,
            type_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            signature TEXT NOT NULL,
            param_count INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL
        );
        """
    )
    return conn


def _type(conn: sqlite3.Connection, type_id: int, qname: str) -> None:
    conn.execute("INSERT INTO source_files(id,path) VALUES(?,?)", (type_id, f"src/{type_id}.java"))
    conn.execute("INSERT INTO types(id,file_id,qualified_name) VALUES(?,?,?)", (type_id, type_id, qname))


def _method(conn: sqlite3.Connection, method_id: int, type_id: int, name: str, signature: str) -> None:
    conn.execute(
        "INSERT INTO methods(id,type_id,name,signature,param_count,start_line,end_line) VALUES(?,?,?,?,0,?,?)",
        (method_id, type_id, name, signature, method_id, method_id + 1),
    )


class R2BPlayEntryFinalSeamsTests(unittest.TestCase):
    def test_committed_plan_is_only_two_reusable_dynamic_groups(self) -> None:
        scope, groups = seams.load_plan()
        self.assertEqual(
            [group.group_id for group in groups],
            ["GENERIC_REGISTRY_WIRE", "GLOBAL_POS_WIRE"],
        )
        self.assertIn("Commands", scope)
        self.assertIn("forbidden", scope)
        allowed_types = {
            selector.type_name
            for group in groups
            for selector in group.selectors
        }
        self.assertEqual(
            allowed_types,
            {
                "net.minecraft.network.codec.ByteBufCodecs",
                "net.minecraft.world.Difficulty",
                "net.minecraft.core.GlobalPos",
                "net.minecraft.resources.ResourceKey",
                "net.minecraft.core.BlockPos",
                "net.minecraft.network.FriendlyByteBuf",
            },
        )

        difficulty = [
            selector
            for group in groups
            for selector in group.selectors
            if selector.type_name == "net.minecraft.world.Difficulty"
        ]
        self.assertEqual(len(difficulty), 1)
        self.assertEqual(difficulty[0].mode, "type_name_regex")
        self.assertEqual(difficulty[0].name_regex, "^(<clinit>|getId)$")

        block_pos_writer = [
            selector
            for group in groups
            for selector in group.selectors
            if selector.type_name == "net.minecraft.network.FriendlyByteBuf"
        ]
        self.assertEqual(len(block_pos_writer), 1)
        self.assertEqual(block_pos_writer[0].mode, "type_names")
        self.assertEqual(block_pos_writer[0].names, ("writeBlockPos",))

    def test_regex_selector_is_bounded_to_method_name(self) -> None:
        conn = _db()
        _type(conn, 1, "test.Codecs")
        _method(conn, 10, 1, "map", "map()")
        _method(conn, 11, 1, "holderRegistry", "holderRegistry()")
        _method(conn, 12, 1, "unrelated", "unrelated()")
        selector = seams.Selector(
            mode="type_name_regex",
            required=True,
            type_name="test.Codecs",
            name_regex="^(map|holderRegistry)$",
        )
        rows = seams._query_selector(conn, selector)
        self.assertEqual([row["name"] for row in rows], ["map", "holderRegistry"])
        conn.close()

    def test_required_type_names_fail_closed_per_name(self) -> None:
        conn = _db()
        _type(conn, 1, "test.ResourceKey")
        _method(conn, 10, 1, "other", "other()")
        selector = seams.Selector(
            mode="type_names",
            required=True,
            type_name="test.ResourceKey",
            names=("streamCodec",),
        )
        with self.assertRaisesRegex(seams.FinalSeamsError, "streamCodec"):
            seams._query_selector(conn, selector)
        conn.close()

    def test_resolution_excludes_117_review_and_deduplicates(self) -> None:
        conn = _db()
        _type(conn, 1, "test.Codecs")
        _method(conn, 10, 1, "map", "map()")
        _method(conn, 11, 1, "holderRegistry", "holderRegistry()")
        group = seams.Group(
            group_id="WIRE",
            review_focus="wire",
            selectors=(
                seams.Selector(
                    mode="type_all",
                    required=True,
                    type_name="test.Codecs",
                ),
                seams.Selector(
                    mode="type_name_regex",
                    required=True,
                    type_name="test.Codecs",
                    name_regex="map",
                ),
            ),
        )
        resolved = seams.resolve_groups(conn, (group,), {"test.Codecs#holderRegistry()"})
        self.assertEqual(len(resolved), 1)
        row, group_ids, focus = resolved[0]
        self.assertEqual(row["signature"], "map()")
        self.assertEqual(group_ids, ("WIRE",))
        self.assertEqual(focus, ("wire",))
        conn.close()

    def test_prior_dossier_requires_exact_sha_commitment(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "review-dossier.json"
            path.write_text("{}\n", encoding="utf-8")
            with self.assertRaisesRegex(seams.FinalSeamsError, "SHA mismatch"):
                seams.validate_prior_review(path)

    def test_plan_sha_is_the_uploaded_117_body_commitment(self) -> None:
        self.assertEqual(
            seams.EXPECTED_PRIOR_SHA256,
            "93999fca0a4c69eda607e729af61c74e7ce40c96bf4201516904fabf79bc2e3a",
        )
        self.assertEqual(seams.EXPECTED_PRIOR_COUNT, 117)


if __name__ == "__main__":
    unittest.main()
