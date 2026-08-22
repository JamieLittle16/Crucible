from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import representative_section_corpus as representative
import section_representative_plan as plan
import vanilla_section_extractor as base


def section(dimension: str, chunk_x: int, chunk_z: int, section_y: int) -> base.ExtractedSection:
    return base.ExtractedSection(
        dimension,
        chunk_x,
        chunk_z,
        section_y,
        (0,) * 4096,
    )


class RepresentativeCorpusSelectionTests(unittest.TestCase):
    def test_selected_chunks_exactly_match_frozen_plan(self) -> None:
        built = plan.build_plan()
        selection = representative.selected_chunks(built)
        self.assertEqual(set(selection), set(plan.DIMENSIONS))
        for dimension in plan.DIMENSIONS:
            self.assertEqual(len(selection[dimension]), plan.CHUNKS_PER_DIMENSION)
            self.assertEqual(
                selection[dimension],
                {tuple(chunk) for chunk in built["dimensions"][dimension]["chunks"]},
            )

    def test_selected_region_paths_use_floor_division_for_negative_chunks(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            world = Path(temporary)
            selection = {
                "minecraft:overworld": {(-1, -1), (-32, -32), (-33, -33), (31, 31), (32, 32)},
                "minecraft:the_nether": {(0, 0)},
                "minecraft:the_end": {(0, 0)},
            }
            directories = {
                name: world / relative for name, relative in base.STANDARD_DIMENSIONS
            }
            expected = {
                "minecraft:overworld": {
                    "r.-1.-1.mca",
                    "r.-2.-2.mca",
                    "r.0.0.mca",
                    "r.1.1.mca",
                },
                "minecraft:the_nether": {"r.0.0.mca"},
                "minecraft:the_end": {"r.0.0.mca"},
            }
            for dimension, names in expected.items():
                directories[dimension].mkdir(parents=True, exist_ok=True)
                for name in names:
                    (directories[dimension] / name).write_bytes(b"region")

            observed = representative.selected_region_paths(world, selection)
            for dimension, paths in observed.items():
                self.assertEqual({path.name for path in paths}, expected[dimension])

    def test_missing_selected_region_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            world = Path(temporary)
            for _, relative in base.STANDARD_DIMENSIONS:
                (world / relative).mkdir(parents=True, exist_ok=True)
            selection = {
                "minecraft:overworld": {(0, 0)},
                "minecraft:the_nether": {(0, 0)},
                "minecraft:the_end": {(0, 0)},
            }
            with self.assertRaises(representative.RepresentativeCorpusError):
                representative.selected_region_paths(world, selection)

    def test_uniform_contiguous_lattice_is_required_per_dimension(self) -> None:
        selection = {
            "minecraft:overworld": {(0, 0), (1, 0)},
            "minecraft:the_nether": {(0, 0)},
            "minecraft:the_end": {(0, 0)},
        }
        sections = []
        for chunk_x in (0, 1):
            for section_y in (-4, -3, -2):
                sections.append(section("minecraft:overworld", chunk_x, 0, section_y))
        for dimension in ("minecraft:the_nether", "minecraft:the_end"):
            for section_y in (0, 1):
                sections.append(section(dimension, 0, 0, section_y))

        lattice = representative.validate_selected_sections(sections, selection)
        self.assertEqual(lattice["minecraft:overworld"], [-4, -3, -2])
        self.assertEqual(lattice["minecraft:the_nether"], [0, 1])
        self.assertEqual(lattice["minecraft:the_end"], [0, 1])

    def test_missing_selected_chunk_fails_closed(self) -> None:
        selection = {
            "minecraft:overworld": {(0, 0), (1, 0)},
            "minecraft:the_nether": {(0, 0)},
            "minecraft:the_end": {(0, 0)},
        }
        sections = [
            section("minecraft:overworld", 0, 0, 0),
            section("minecraft:the_nether", 0, 0, 0),
            section("minecraft:the_end", 0, 0, 0),
        ]
        with self.assertRaises(representative.RepresentativeCorpusError):
            representative.validate_selected_sections(sections, selection)

    def test_gap_in_section_lattice_fails_closed(self) -> None:
        selection = {
            "minecraft:overworld": {(0, 0)},
            "minecraft:the_nether": {(0, 0)},
            "minecraft:the_end": {(0, 0)},
        }
        sections = [
            section("minecraft:overworld", 0, 0, -4),
            section("minecraft:overworld", 0, 0, -2),
            section("minecraft:the_nether", 0, 0, 0),
            section("minecraft:the_end", 0, 0, 0),
        ]
        with self.assertRaises(representative.RepresentativeCorpusError):
            representative.validate_selected_sections(sections, selection)

    def test_chunks_in_same_dimension_must_share_lattice(self) -> None:
        selection = {
            "minecraft:overworld": {(0, 0), (1, 0)},
            "minecraft:the_nether": {(0, 0)},
            "minecraft:the_end": {(0, 0)},
        }
        sections = [
            section("minecraft:overworld", 0, 0, -4),
            section("minecraft:overworld", 0, 0, -3),
            section("minecraft:overworld", 1, 0, -4),
            section("minecraft:the_nether", 0, 0, 0),
            section("minecraft:the_end", 0, 0, 0),
        ]
        with self.assertRaises(representative.RepresentativeCorpusError):
            representative.validate_selected_sections(sections, selection)

    def test_unplanned_chunk_is_rejected_by_lattice_validator(self) -> None:
        selection = {
            "minecraft:overworld": {(0, 0)},
            "minecraft:the_nether": {(0, 0)},
            "minecraft:the_end": {(0, 0)},
        }
        sections = [
            section("minecraft:overworld", 0, 0, 0),
            section("minecraft:overworld", 5, 5, 0),
            section("minecraft:the_nether", 0, 0, 0),
            section("minecraft:the_end", 0, 0, 0),
        ]
        with self.assertRaises(representative.RepresentativeCorpusError):
            representative.validate_selected_sections(sections, selection)


if __name__ == "__main__":
    unittest.main()
