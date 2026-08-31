from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools import r2c_import_differential as differential
from tools import vanilla_section_extractor as oracle


class ImportDifferentialFixtureTests(unittest.TestCase):
    def test_fixture_regions_are_valid_for_independent_oracle(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            regions = differential._fixture_regions(Path(raw))
            self.assertEqual([path.name for path in regions], ["r.-1.-1.mca", "r.0.0.mca"])
            sections = []
            for region in regions:
                sections.extend(
                    oracle.extract_region(
                        region,
                        differential.DIMENSION,
                        differential.FIXTURE_STATE_IDS,
                    )
                )
            self.assertEqual(len(sections), 7)
            self.assertEqual(
                {(section.chunk_x, section.chunk_z) for section in sections},
                {(-32, -32), (-1, -1), (0, 0), (1, 0), (2, 0), (3, 0)},
            )
            self.assertTrue(
                any(
                    len(set(section.states)) == 2
                    for section in sections
                )
            )

    def test_five_bit_pattern_crosses_non_spanning_word_boundaries(self) -> None:
        pattern = differential._wide_palette_pattern()
        words = differential._packed_words(17, pattern)
        self.assertEqual(len(words), 342)
        self.assertEqual(len(pattern), 4096)
        self.assertEqual(set(pattern), set(range(17)))


if __name__ == "__main__":
    unittest.main()
