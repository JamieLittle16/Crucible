from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import vanilla_chunk_status as status


class VanillaChunkStatusTests(unittest.TestCase):
    def test_saved_status_must_be_nonempty_string(self) -> None:
        self.assertEqual(
            status._require_status({"Status": "minecraft:full"}, 0, 0),
            "minecraft:full",
        )
        for value in (None, "", 7):
            with self.subTest(value=value), self.assertRaises(status.ChunkStatusError):
                status._require_status({"Status": value}, 0, 0)

    def test_selected_chunks_must_all_be_full(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            region = Path(tmp_name) / "r.0.0.mca"
            region.write_bytes(b"unused by patched scanner")
            regions = {"minecraft:overworld": {region}}
            selection = {"minecraft:overworld": {(0, 0), (1, 0)}}
            records = [
                status.ChunkStatusRecord("minecraft:overworld", 0, 0, "minecraft:full"),
                status.ChunkStatusRecord("minecraft:overworld", 1, 0, "minecraft:full"),
            ]
            with mock.patch.object(status, "extract_region_statuses", return_value=records):
                self.assertEqual(
                    status.qualify_selected_chunks(regions, selection),
                    {"minecraft:full": 2},
                )

    def test_proto_chunk_status_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            region = Path(tmp_name) / "r.0.0.mca"
            region.write_bytes(b"unused by patched scanner")
            regions = {"minecraft:overworld": {region}}
            selection = {"minecraft:overworld": {(0, 0)}}
            records = [
                status.ChunkStatusRecord("minecraft:overworld", 0, 0, "minecraft:features")
            ]
            with mock.patch.object(status, "extract_region_statuses", return_value=records):
                with self.assertRaisesRegex(status.ChunkStatusError, "minecraft:full"):
                    status.qualify_selected_chunks(regions, selection)

    def test_missing_or_duplicate_selected_status_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            first = Path(tmp_name) / "r.0.0.mca"
            second = Path(tmp_name) / "r.1.0.mca"
            first.write_bytes(b"unused")
            second.write_bytes(b"unused")
            selection = {"minecraft:overworld": {(0, 0), (1, 0)}}

            with mock.patch.object(
                status,
                "extract_region_statuses",
                return_value=[
                    status.ChunkStatusRecord("minecraft:overworld", 0, 0, "minecraft:full")
                ],
            ):
                with self.assertRaisesRegex(status.ChunkStatusError, "identity mismatch"):
                    status.qualify_selected_chunks(
                        {"minecraft:overworld": {first}}, selection
                    )

            calls = [
                [status.ChunkStatusRecord("minecraft:overworld", 0, 0, "minecraft:full")],
                [status.ChunkStatusRecord("minecraft:overworld", 0, 0, "minecraft:full")],
            ]
            with mock.patch.object(status, "extract_region_statuses", side_effect=calls):
                with self.assertRaisesRegex(status.ChunkStatusError, "more than once"):
                    status.qualify_selected_chunks(
                        {"minecraft:overworld": {first, second}}, selection
                    )


if __name__ == "__main__":
    unittest.main()
