from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "tools" / "section_corpus.py"
SPEC = importlib.util.spec_from_file_location("crucible_section_corpus_dimensions", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
section_corpus = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = section_corpus
SPEC.loader.exec_module(section_corpus)

GENERATION = "a" * 64
INPUT = "b" * 64
SOURCE = "c" * 64


def target(tmp: Path):
    manifest = tmp / "manifest.json"
    generated = tmp / "generated.rs"
    manifest.write_text(
        json.dumps(
            {
                "schema": 1,
                "state_count": 4,
                "generation_digest": GENERATION,
                "input_digest": INPUT,
                "target": {
                    "minecraft_version": "26.2",
                    "protocol_version": 776,
                    "data_version": 4903,
                },
            }
        ),
        encoding="utf-8",
    )
    generated.write_text(
        "pub const BLOCK_STATE_COUNT: usize = 4;\n"
        f'pub const STATE_DATA_INPUT_SHA256: &str = "{INPUT}";\n'
        f'pub const STATE_DATA_GENERATION_SHA256: &str = "{GENERATION}";\n'
        "pub static STATE_MUTATION_FLAGS: [u8; BLOCK_STATE_COUNT] = [0, 1, 3, 15];\n",
        encoding="utf-8",
    )
    return section_corpus.load_target_evidence(manifest, generated)


def section(dimension: str, y: int, states: list[int]) -> str:
    return f"SECTION|{dimension}|0|0|{y}|" + ",".join(map(str, states))


class SectionCorpusDimensionTests(unittest.TestCase):
    def test_per_dimension_statistics_are_independently_derived_from_cells(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            overworld_air = [0] * 4096
            overworld_solid = [1] * 4096
            end_mixed = [0] * 2048 + [2] * 1024 + [3] * 1024
            text = "\n".join(
                [
                    "CRUCIBLE-SECTION-CORPUS|1",
                    "TARGET|minecraft=26.2|protocol=776|data=4903|state_count=4|"
                    f"generation_sha256={GENERATION}",
                    f"SOURCE|kind=vanilla-save|inventory_sha256={SOURCE}|extractor=fixture-v1",
                    section("minecraft:overworld", 0, overworld_air),
                    section("minecraft:overworld", 1, overworld_solid),
                    section("minecraft:the_end", 0, end_mixed),
                ]
            ) + "\n"
            path = tmp / "corpus.txt"
            path.write_text(text, encoding="utf-8", newline="\n")

            parsed = section_corpus.validate_corpus(path, target(tmp))
            self.assertEqual(parsed.dimensions, {"minecraft:overworld": 2, "minecraft:the_end": 1})

            overworld = parsed.per_dimension["minecraft:overworld"]
            self.assertEqual(overworld.section_count, 2)
            self.assertEqual(overworld.total_cells, 8192)
            self.assertEqual(overworld.distinct_state_ids, 2)
            self.assertEqual(overworld.cardinality_histogram, {1: 2})
            self.assertEqual(overworld.cell_facts["non_air"], 4096)
            self.assertEqual(overworld.section_classes["all_air"], 1)

            end = parsed.per_dimension["minecraft:the_end"]
            self.assertEqual(end.section_count, 1)
            self.assertEqual(end.total_cells, 4096)
            self.assertEqual(end.distinct_state_ids, 3)
            self.assertEqual(end.cardinality_histogram, {3: 1})
            self.assertEqual(end.cell_facts["non_air"], 2048)
            self.assertEqual(end.cell_facts["counted_fluid"], 2048)
            self.assertEqual(end.cell_facts["random_block"], 1024)
            self.assertEqual(end.cell_facts["random_fluid"], 1024)
            self.assertEqual(end.section_classes["all_air"], 0)
            self.assertEqual(end.section_classes["contains_fluid"], 1)

            manifest = parsed.manifest()
            self.assertEqual(
                manifest["per_dimension"]["minecraft:overworld"]["cardinality_histogram"],
                {"1": 2},
            )
            self.assertEqual(
                manifest["per_dimension"]["minecraft:the_end"]["cardinality_histogram"],
                {"3": 1},
            )

            recomposed_sections = sum(
                entry["section_count"] for entry in manifest["per_dimension"].values()
            )
            self.assertEqual(recomposed_sections, manifest["section_count"])


if __name__ == "__main__":
    unittest.main()
