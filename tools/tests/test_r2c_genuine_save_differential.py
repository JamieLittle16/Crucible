from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools import r2c_genuine_save_differential as differential


class GenuineSaveDifferentialTests(unittest.TestCase):
    def test_negative_chunk_coordinates_partition_by_floor_region(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            corpus = root / "corpus.txt"
            states = ",".join("0" for _ in range(4096))
            corpus.write_text(
                "HELVE-SECTION-CORPUS|1\n"
                "TARGET|dummy\n"
                "SOURCE|dummy\n"
                f"SECTION|minecraft:overworld|-1|-1|-4|{states}\n",
                encoding="utf-8",
            )
            region_path = root / "r.-1.-1.mca"
            region_path.write_bytes(b"")
            count, digest = differential._partition_expected(
                corpus,
                root / "expected",
                "minecraft:overworld",
                {(-1, -1): region_path},
            )
            self.assertEqual(count, 1)
            self.assertEqual(len(digest), 64)
            self.assertTrue((root / "expected" / "r.-1.-1.expected").is_file())

    def test_first_difference_reports_exact_cell(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            expected = root / "expected"
            actual = root / "actual"
            left = ["0"] * 4096
            right = left.copy()
            right[3073] = "1"
            prefix = "SECTION|minecraft:overworld|3|-2|7|"
            expected.write_text(prefix + ",".join(left) + "\n", encoding="utf-8")
            actual.write_text(prefix + ",".join(right) + "\n", encoding="utf-8")
            message = differential._first_difference(expected, actual)
            self.assertIn("cell=3073", message)
            self.assertIn("oracle=0", message)
            self.assertIn("rust=1", message)


if __name__ == "__main__":
    unittest.main()
