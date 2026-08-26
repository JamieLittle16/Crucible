from __future__ import annotations

import importlib.util
import json
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
    "r2b_play_entry_runtime_seams_source_review",
    TOOLS / "r2b_play_entry_runtime_seams_source_review.py",
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


class R2BPlayEntryRuntimeSeamsTests(unittest.TestCase):
    def test_committed_plan_is_narrow_and_base_gate_pinned(self) -> None:
        scope, reused, groups = seams._load_plan(seams.DEFAULT_PLAN)
        self.assertEqual(
            [group.group_id for group in groups],
            ["TELEPORT_TERMINALS", "GAME_TYPE_IDS", "SELECTED_PROFILE_BYTEBUF_HELPERS"],
        )
        self.assertIn("non-empty ItemStack", scope)
        self.assertIn("forbidden", scope)
        self.assertEqual(seams.BASE_REQUIRED_METHODS, 206)
        self.assertEqual(
            seams.BASE_GATE_SHA256,
            "a304f8f35a411b2d14300c5cf1bbe8097afe9eadd1059ae180e4129ef8d781cb",
        )
        self.assertEqual(len(reused), 4)

        by_type = {
            selector.type_name: selector.names
            for group in groups
            for selector in group.selectors
        }
        self.assertEqual(by_type["net.minecraft.world.phys.Vec3"], ("<clinit>",))
        self.assertEqual(by_type["net.minecraft.world.entity.Relative"], ("pack",))
        self.assertEqual(
            by_type["net.minecraft.world.level.GameType"],
            ("<clinit>", "GameType", "getId", "getNullableId"),
        )
        self.assertEqual(by_type["net.minecraft.network.codec.ByteBufCodecs"], ("<clinit>",))
        self.assertEqual(
            by_type["net.minecraft.network.FriendlyByteBuf"],
            (
                "writeEnumSet",
                "writeFixedBitSet",
                "writeNullable",
                "writeUUID",
                "writeContainerId",
            ),
        )

    def test_base_report_requires_exact_admitted_gate(self) -> None:
        report = {
            "gate_id": seams.BASE_GATE_ID,
            "gate_sha256": seams.BASE_GATE_SHA256,
            "admitted": True,
            "failures": [],
            "required_methods": [
                {"source": f"test.Type#m{i}()"} for i in range(seams.BASE_REQUIRED_METHODS)
            ],
            "source": {"archive_sha256": seams.EXPECTED_SOURCE_SHA256},
        }
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "report.json"
            path.write_text(json.dumps(report), encoding="utf-8")
            identities, base = seams._validate_base_report(path)
            self.assertEqual(len(identities), seams.BASE_REQUIRED_METHODS)
            self.assertEqual(base["sha256"], seams.BASE_GATE_SHA256)

            report["gate_sha256"] = "0" * 64
            path.write_text(json.dumps(report), encoding="utf-8")
            with self.assertRaisesRegex(seams.RuntimeSeamsError, "identity/hash mismatch"):
                seams._validate_base_report(path)

    def test_resolution_requires_every_named_terminal(self) -> None:
        conn = _db()
        _type(conn, 1, "test.GameType")
        _method(conn, 10, 1, "<clinit>", "<clinit>()")
        _method(conn, 11, 1, "getId", "getId()")
        group = seams.Group(
            group_id="GAME_TYPE_IDS",
            review_focus="ids",
            selectors=(
                seams.Selector(
                    mode="type_names",
                    required=True,
                    type_name="test.GameType",
                    names=("<clinit>", "GameType", "getId"),
                ),
            ),
        )
        with self.assertRaisesRegex(seams.RuntimeSeamsError, "GameType"):
            seams._resolve_groups(conn, (group,), set())
        conn.close()

    def test_resolution_rejects_identity_already_in_base_gate(self) -> None:
        conn = _db()
        _type(conn, 1, "test.Relative")
        _method(conn, 10, 1, "pack", "pack(Set)")
        group = seams.Group(
            group_id="TELEPORT_TERMINALS",
            review_focus="relative",
            selectors=(
                seams.Selector(
                    mode="type_names",
                    required=True,
                    type_name="test.Relative",
                    names=("pack",),
                ),
            ),
        )
        with self.assertRaisesRegex(seams.RuntimeSeamsError, "redundantly resolves base-gate identity"):
            seams._resolve_groups(conn, (group,), {"test.Relative#pack(Set)"})
        conn.close()


if __name__ == "__main__":
    unittest.main()
