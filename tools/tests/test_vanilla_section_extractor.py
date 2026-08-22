from __future__ import annotations

import gzip
import importlib.util
import json
import struct
import sys
import tempfile
import unittest
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TOOLS = ROOT / "tools"
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

import state_data

MODULE_PATH = TOOLS / "vanilla_section_extractor.py"
SPEC = importlib.util.spec_from_file_location("crucible_vanilla_section_extractor", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
extractor = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = extractor
SPEC.loader.exec_module(extractor)

GENERATION = "a" * 64


def nbt_string(value: str) -> bytes:
    encoded = value.encode("utf-8")
    return struct.pack(">H", len(encoded)) + encoded


def named(tag_type: int, name: str, payload: bytes) -> bytes:
    return bytes([tag_type]) + nbt_string(name) + payload


def string_payload(value: str) -> bytes:
    return nbt_string(value)


def int_payload(value: int) -> bytes:
    return struct.pack(">i", value)


def byte_payload(value: int) -> bytes:
    return struct.pack(">b", value)


def long_array_payload(values: list[int]) -> bytes:
    return struct.pack(">i", len(values)) + b"".join(
        struct.pack(">q", value) for value in values
    )


def compound_payload(entries: list[bytes]) -> bytes:
    return b"".join(entries) + b"\x00"


def list_payload(element_type: int, payloads: list[bytes]) -> bytes:
    return bytes([element_type]) + struct.pack(">i", len(payloads)) + b"".join(payloads)


def root_compound(entries: list[bytes]) -> bytes:
    return b"\x0a" + nbt_string("") + compound_payload(entries)


def palette_entry(name: str, properties: dict[str, str] | None = None) -> bytes:
    entries = [named(8, "Name", string_payload(name))]
    if properties is not None:
        property_entries = [
            named(8, key, string_payload(value)) for key, value in properties.items()
        ]
        entries.append(named(10, "Properties", compound_payload(property_entries)))
    return compound_payload(entries)


def pack_indices(indices: list[int], palette_size: int) -> list[int]:
    bits = max(4, (palette_size - 1).bit_length())
    per_long = 64 // bits
    words = [0] * ((len(indices) + per_long - 1) // per_long)
    for index, value in enumerate(indices):
        words[index // per_long] |= value << ((index % per_long) * bits)
    return [word if word < 1 << 63 else word - (1 << 64) for word in words]


def block_states_payload(
    palette: list[tuple[str, dict[str, str] | None]],
    indices: list[int] | None = None,
    override_data: list[int] | None = None,
) -> bytes:
    entries = [
        named(
            9,
            "palette",
            list_payload(
                10,
                [palette_entry(name, properties) for name, properties in palette],
            ),
        )
    ]
    if len(palette) > 1:
        data = (
            override_data
            if override_data is not None
            else pack_indices(indices or [0] * 4096, len(palette))
        )
        entries.append(named(12, "data", long_array_payload(data)))
    return compound_payload(entries)


def section_payload(
    y: int,
    palette: list[tuple[str, dict[str, str] | None]],
    indices: list[int] | None = None,
    override_data: list[int] | None = None,
) -> bytes:
    return compound_payload(
        [
            named(1, "Y", byte_payload(y)),
            named(
                10,
                "block_states",
                block_states_payload(palette, indices, override_data),
            ),
        ]
    )


def chunk_nbt(
    chunk_x: int = 0,
    chunk_z: int = 0,
    data_version: int = 4903,
    sections: list[bytes] | None = None,
) -> bytes:
    if sections is None:
        sections = [section_payload(0, [("minecraft:air", None)])]
    return root_compound(
        [
            named(3, "DataVersion", int_payload(data_version)),
            named(3, "xPos", int_payload(chunk_x)),
            named(3, "zPos", int_payload(chunk_z)),
            named(9, "sections", list_payload(10, sections)),
        ]
    )


def level_nbt(data_version: int = 4903) -> bytes:
    data = compound_payload([named(3, "DataVersion", int_payload(data_version))])
    return root_compound([named(10, "Data", data)])


def write_region(path: Path, chunk: bytes) -> None:
    compressed = zlib.compress(chunk)
    record = len(compressed + b"\x02").to_bytes(4, "big") + b"\x02" + compressed
    sectors = (len(record) + 4095) // 4096
    header = bytearray(8192)
    header[0:4] = ((2 << 8) | sectors).to_bytes(4, "big")
    payload = record + b"\x00" * (sectors * 4096 - len(record))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(bytes(header) + payload)


def qualified_state_data() -> dict[str, object]:
    return {
        "schema": 1,
        "target": {
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "data_version": 4903,
        },
        "air_key": "minecraft:air",
        "states": [
            {
                "key": "minecraft:air",
                "vanilla_id": 0,
                "non_air": False,
                "counted_fluid": False,
                "random_block": False,
                "random_fluid": False,
            },
            {
                "key": "minecraft:stone",
                "vanilla_id": 1,
                "non_air": True,
                "counted_fluid": False,
                "random_block": False,
                "random_fluid": False,
            },
            {
                "key": "minecraft:oak_log[axis=x]",
                "vanilla_id": 2,
                "non_air": True,
                "counted_fluid": False,
                "random_block": False,
                "random_fluid": False,
            },
            {
                "key": "minecraft:water[level=0]",
                "vanilla_id": 3,
                "non_air": True,
                "counted_fluid": True,
                "random_block": False,
                "random_fluid": False,
            },
        ],
    }


def write_state_evidence(tmp: Path) -> tuple[Path, Path, Path]:
    qualified = qualified_state_data()
    input_digest = state_data.digest(qualified)
    qualified_path = tmp / "qualified.json"
    manifest_path = tmp / "manifest.json"
    generated_path = tmp / "generated.rs"
    qualified_path.write_text(json.dumps(qualified), encoding="utf-8")
    manifest = {
        "schema": 1,
        "target": qualified["target"],
        "state_count": 4,
        "assignment_policy": "vanilla-identity",
        "mapping": "identity",
        "input_digest": input_digest,
        "generation_digest": GENERATION,
    }
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    generated_path.write_text(
        "pub const BLOCK_STATE_COUNT: usize = 4;\n"
        f'pub const STATE_DATA_INPUT_SHA256: &str = "{input_digest}";\n'
        f'pub const STATE_DATA_GENERATION_SHA256: &str = "{GENERATION}";\n'
        "pub static STATE_MUTATION_FLAGS: [u8; BLOCK_STATE_COUNT] = [0, 1, 1, 3];\n",
        encoding="utf-8",
    )
    return qualified_path, manifest_path, generated_path


def write_world(tmp: Path, chunk: bytes, level_version: int = 4903) -> Path:
    world = tmp / "world"
    world.mkdir()
    (world / "level.dat").write_bytes(gzip.compress(level_nbt(level_version)))
    write_region(world / "region" / "r.0.0.mca", chunk)
    return world


def typed_palette(values: list[dict[str, object]]) -> extractor.NbtListValue:
    return extractor.NbtListValue(10, tuple(values))


def typed_longs(values: list[int]) -> extractor.NbtLongArray:
    return extractor.NbtLongArray(tuple(values))


class VanillaSectionExtractorTests(unittest.TestCase):
    def test_palette_identity_sorts_properties(self) -> None:
        entry = {
            "Name": "minecraft:test",
            "Properties": {"z": "2", "a": "1"},
        }
        self.assertEqual(
            extractor.canonical_palette_key(entry),
            "minecraft:test[a=1,z=2]",
        )

    def test_decode_block_states_uses_non_spanning_packed_longs(self) -> None:
        palette = [
            {"Name": "minecraft:air"},
            {"Name": "minecraft:stone"},
            {"Name": "minecraft:oak_log", "Properties": {"axis": "x"}},
        ]
        indices = [index % 3 for index in range(4096)]
        data = pack_indices(indices, 3)
        decoded = extractor.decode_block_states(
            {"palette": typed_palette(palette), "data": typed_longs(data)},
            {
                "minecraft:air": 0,
                "minecraft:stone": 1,
                "minecraft:oak_log[axis=x]": 2,
            },
        )
        self.assertEqual(decoded, tuple(indices))

    def test_end_to_end_extracts_and_revalidates_normalized_corpus(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            qualified, manifest, generated = write_state_evidence(tmp)
            indices = [0] * 2048 + [1] * 1024 + [2] * 512 + [3] * 512
            section = section_payload(
                0,
                [
                    ("minecraft:air", None),
                    ("minecraft:stone", None),
                    ("minecraft:oak_log", {"axis": "x"}),
                    ("minecraft:water", {"level": "0"}),
                ],
                indices,
            )
            world = write_world(tmp, chunk_nbt(sections=[section]))
            corpus = tmp / "corpus.txt"
            inventory = tmp / "inventory.json"

            parsed = extractor.extract_world(
                world=world,
                qualified_states=qualified,
                state_manifest_path=manifest,
                generated_rust_path=generated,
                output=corpus,
                inventory_output=inventory,
                dimensions=None,
            )

            self.assertEqual(parsed.section_count, 1)
            self.assertEqual(parsed.distinct_state_ids, 4)
            self.assertEqual(parsed.cardinality_histogram, {4: 1})
            self.assertEqual(parsed.cell_facts["non_air"], 2048)
            self.assertEqual(parsed.cell_facts["counted_fluid"], 512)
            inventory_data = json.loads(inventory.read_text(encoding="utf-8"))
            self.assertEqual(inventory_data["policy"], extractor.EXTRACTOR_ID)
            self.assertEqual(inventory_data["section_count"], 1)
            self.assertEqual(len(inventory_data["files"]), 2)
            self.assertIn(
                "extractor=vanilla-save-region-v1-stored-sections",
                corpus.read_text(),
            )

    def test_level_dat_and_chunk_data_versions_are_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            world = write_world(tmp, chunk_nbt(), level_version=4902)
            with self.assertRaisesRegex(
                extractor.ExtractorError,
                "level.dat DataVersion",
            ):
                extractor.validate_level_dat(world)

            region = tmp / "bad-region" / "r.0.0.mca"
            write_region(region, chunk_nbt(data_version=4902))
            with self.assertRaisesRegex(extractor.ExtractorError, "DataVersion 4902"):
                extractor.extract_region(
                    region,
                    "minecraft:overworld",
                    {"minecraft:air": 0},
                )

    def test_unknown_saved_state_is_rejected(self) -> None:
        section = {
            "palette": typed_palette([{"Name": "minecraft:not_in_target"}]),
        }
        with self.assertRaisesRegex(
            extractor.ExtractorError,
            "absent from qualified target",
        ):
            extractor.decode_block_states(section, {"minecraft:air": 0})

    def test_region_slot_must_match_chunk_coordinates(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            region = tmp / "r.0.0.mca"
            write_region(region, chunk_nbt(chunk_x=1))
            with self.assertRaisesRegex(extractor.ExtractorError, "contains chunk 1,0"):
                extractor.extract_region(
                    region,
                    "minecraft:overworld",
                    {"minecraft:air": 0},
                )

    def test_packed_long_count_and_palette_indices_are_checked(self) -> None:
        palette = typed_palette(
            [{"Name": "minecraft:air"}, {"Name": "minecraft:stone"}]
        )
        state_ids = {"minecraft:air": 0, "minecraft:stone": 1}
        with self.assertRaisesRegex(
            extractor.ExtractorError,
            "length does not match palette",
        ):
            extractor.decode_block_states(
                {"palette": palette, "data": typed_longs([0])},
                state_ids,
            )

        data = pack_indices([0] * 4096, 2)
        data[0] = 2  # First 4-bit palette index is outside a two-entry palette.
        with self.assertRaisesRegex(
            extractor.ExtractorError,
            "palette index 2 outside",
        ):
            extractor.decode_block_states(
                {"palette": palette, "data": typed_longs(data)},
                state_ids,
            )

    def test_list_cannot_masquerade_as_long_array(self) -> None:
        palette = typed_palette(
            [{"Name": "minecraft:air"}, {"Name": "minecraft:stone"}]
        )
        fake_data = extractor.NbtListValue(4, tuple([0] * 256))
        with self.assertRaisesRegex(
            extractor.ExtractorError,
            "must be an NBT long array",
        ):
            extractor.decode_block_states(
                {"palette": palette, "data": fake_data},
                {"minecraft:air": 0, "minecraft:stone": 1},
            )

    def test_palette_and_sections_require_compound_lists(self) -> None:
        wrong_palette_type = extractor.NbtListValue(8, ("minecraft:air",))
        with self.assertRaisesRegex(
            extractor.ExtractorError,
            "element type 10",
        ):
            extractor.decode_block_states(
                {"palette": wrong_palette_type},
                {"minecraft:air": 0},
            )

        parsed = extractor.parse_nbt(
            root_compound([named(9, "sections", list_payload(8, [string_payload("bad")]))])
        )
        with self.assertRaisesRegex(extractor.ExtractorError, "element type 10"):
            extractor._require_list(parsed["sections"], "chunk sections", element_type=10)

    def test_lz4_is_rejected_explicitly_in_v1(self) -> None:
        with self.assertRaisesRegex(extractor.ExtractorError, "LZ4-compressed"):
            extractor._decompress_chunk(b"ignored", 4)

    def test_qualified_state_map_must_match_committed_input_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_name:
            tmp = Path(tmp_name)
            qualified, manifest, _ = write_state_evidence(tmp)
            manifest_data = json.loads(manifest.read_text(encoding="utf-8"))
            manifest_data["input_digest"] = "f" * 64
            manifest.write_text(json.dumps(manifest_data), encoding="utf-8")
            with self.assertRaisesRegex(
                extractor.ExtractorError,
                "does not match committed",
            ):
                extractor.load_state_identity_map(qualified, manifest)


if __name__ == "__main__":
    unittest.main()
