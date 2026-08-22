from __future__ import annotations

import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "tools" / "section_corpus.py"
SPEC = importlib.util.spec_from_file_location("crucible_section_corpus", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
section_corpus = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = section_corpus
SPEC.loader.exec_module(section_corpus)

GENERATION = "a" * 64
INPUT = "b" * 64
SOURCE = "c" * 64


def state_manifest() -> dict[str, object]:
    return {
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


def generated_rust(flags: list[int] | None = None) -> str:
    values = flags if flags is not None else [0, 1, 3, 15]
    return (
        "pub const BLOCK_STATE_COUNT: usize = 4;\n"
        f'pub const STATE_DATA_INPUT_SHA256: &str = "{INPUT}";\n'
        f'pub const STATE_DATA_GENERATION_SHA256: &str = "{GENERATION}";\n'
        "pub static STATE_MUTATION_FLAGS: [u8; BLOCK_STATE_COUNT] = ["
        + ", ".join(str(value) for value in values)
        + "];\n"
    )


def target(tmp: Path, flags: list[int] | None = None):
    manifest_path = tmp / "manifest.json"
    rust_path = tmp / "generated.rs"
    manifest_path.write_text(json.dumps(state_manifest()), encoding="utf-8")
    rust_path.write_text(generated_rust(flags), encoding="utf-8")
    return section_corpus.load_target_evidence(manifest_path, rust_path)


def section_line(
    dimension: str,
    chunk_x: int,
    chunk_z: int,
    section_y: int,
    states: list[int],
) -> str:
    return (
        f"SECTION|{dimension}|{chunk_x}|{chunk_z}|{section_y}|"
        + ",".join(str(state) for state in states)
    )


def corpus_text(lines: list[str], generation: str = GENERATION, source_kind: str = "vanilla-save") -> str:
    headers = [
        "CRUCIBLE-SECTION-CORPUS|1",
        "TARGET|minecraft=26.2|protocol=776|data=4903|state_count=4|"
        f"generation_sha256={generation}",
        f"SOURCE|kind={source_kind}|inventory_sha256={SOURCE}|extractor=fixture-extractor-v1",
    ]
    return "\n".join(headers + lines) + "\n"


class SectionCorpusTests(unittest.TestCase):
    def test_valid_corpus_recomputes_manifest_from_cells(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            target_evidence = target(tmp)
            first = [0] * 4096
            second = [1] * 2048 + [2] * 1024 + [3] * 1024
            text = corpus_text(
                [
                    section_line("minecraft:overworld", 0, 0, 0, first),
                    section_line("minecraft:overworld", 0, 0, 1, second),
                ]
            )
            path = tmp / "corpus.txt"
            path.write_text(text, encoding="utf-8", newline="\n")

            parsed = section_corpus.validate_corpus(path, target_evidence)
            manifest = parsed.manifest()

            self.assertEqual(parsed.section_count, 2)
            self.assertEqual(parsed.total_cells, 8192)
            self.assertEqual(parsed.distinct_state_ids, 4)
            self.assertEqual(parsed.cardinality_histogram, {1: 1, 3: 1})
            self.assertEqual(parsed.dimensions, {"minecraft:overworld": 2})
            self.assertEqual(
                parsed.cell_facts,
                {
                    "non_air": 4096,
                    "counted_fluid": 2048,
                    "random_block": 1024,
                    "random_fluid": 1024,
                },
            )
            self.assertEqual(
                parsed.section_classes,
                {
                    "all_air": 1,
                    "contains_fluid": 1,
                    "random_block_present": 1,
                    "random_fluid_present": 1,
                },
            )
            self.assertEqual(parsed.corpus_sha256, hashlib.sha256(text.encode()).hexdigest())
            self.assertEqual(manifest["target"]["state_data_input_sha256"], INPUT)

    def test_wrong_target_generation_digest_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            path = tmp / "corpus.txt"
            path.write_text(
                corpus_text(
                    [section_line("minecraft:overworld", 0, 0, 0, [0] * 4096)],
                    generation="d" * 64,
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(section_corpus.CorpusError, "does not match frozen state data"):
                section_corpus.validate_corpus(path, target(tmp))

    def test_out_of_range_state_id_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            states = [0] * 4096
            states[123] = 4
            path = tmp / "corpus.txt"
            path.write_text(
                corpus_text([section_line("minecraft:overworld", 0, 0, 0, states)]),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(section_corpus.CorpusError, "outside 0..3"):
                section_corpus.validate_corpus(path, target(tmp))

    def test_section_must_have_exactly_4096_cells(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            path = tmp / "corpus.txt"
            path.write_text(
                corpus_text([section_line("minecraft:overworld", 0, 0, 0, [0] * 4095)]),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(section_corpus.CorpusError, "4095 cells; expected 4096"):
                section_corpus.validate_corpus(path, target(tmp))

    def test_duplicate_and_out_of_order_coordinates_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            one = section_line("minecraft:overworld", 0, 0, 1, [0] * 4096)
            duplicate = tmp / "duplicate.txt"
            duplicate.write_text(corpus_text([one, one]), encoding="utf-8")
            with self.assertRaisesRegex(section_corpus.CorpusError, "duplicate"):
                section_corpus.validate_corpus(duplicate, target(tmp))

            out_of_order = tmp / "out-of-order.txt"
            zero = section_line("minecraft:overworld", 0, 0, 0, [0] * 4096)
            out_of_order.write_text(corpus_text([one, zero]), encoding="utf-8")
            with self.assertRaisesRegex(section_corpus.CorpusError, "out of order"):
                section_corpus.validate_corpus(out_of_order, target(tmp))

    def test_noncanonical_line_endings_and_missing_final_newline_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            text = corpus_text([section_line("minecraft:overworld", 0, 0, 0, [0] * 4096)])
            crlf = tmp / "crlf.txt"
            crlf.write_bytes(text.replace("\n", "\r\n").encode())
            with self.assertRaisesRegex(section_corpus.CorpusError, "LF line endings"):
                section_corpus.validate_corpus(crlf, target(tmp))

            no_newline = tmp / "no-newline.txt"
            no_newline.write_bytes(text.rstrip("\n").encode())
            with self.assertRaisesRegex(section_corpus.CorpusError, "end with a newline"):
                section_corpus.validate_corpus(no_newline, target(tmp))

    def test_invalid_dimension_and_source_kind_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            bad_dimension = tmp / "dimension.txt"
            bad_dimension.write_text(
                corpus_text([section_line("Over World", 0, 0, 0, [0] * 4096)]),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(section_corpus.CorpusError, "invalid dimension"):
                section_corpus.validate_corpus(bad_dimension, target(tmp))

            bad_source = tmp / "source.txt"
            bad_source.write_text(
                corpus_text(
                    [section_line("minecraft:overworld", 0, 0, 0, [0] * 4096)],
                    source_kind="synthetic",
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(section_corpus.CorpusError, "must be vanilla-save"):
                section_corpus.validate_corpus(bad_source, target(tmp))

    def test_generated_fact_table_must_match_state_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            with self.assertRaisesRegex(section_corpus.CorpusError, "has 3 entries; expected 4"):
                target(tmp, flags=[0, 1, 3])
            with self.assertRaisesRegex(section_corpus.CorpusError, "bits outside"):
                target(tmp, flags=[0, 1, 3, 16])

    def test_noncanonical_coordinate_and_state_numbers_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            coordinate = tmp / "coordinate.txt"
            line = section_line("minecraft:overworld", 0, 0, 0, [0] * 4096).replace(
                "|0|0|0|", "|00|0|0|", 1
            )
            coordinate.write_text(corpus_text([line]), encoding="utf-8")
            with self.assertRaisesRegex(section_corpus.CorpusError, "canonical decimal"):
                section_corpus.validate_corpus(coordinate, target(tmp))

            state = tmp / "state.txt"
            line = section_line("minecraft:overworld", 0, 0, 0, [0] * 4096)
            line = line.rsplit("|", 1)[0] + "|00," + ",".join(["0"] * 4095)
            state.write_text(corpus_text([line]), encoding="utf-8")
            with self.assertRaisesRegex(section_corpus.CorpusError, "noncanonical state ID"):
                section_corpus.validate_corpus(state, target(tmp))


if __name__ == "__main__":
    unittest.main()
