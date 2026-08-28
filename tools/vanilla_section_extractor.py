#!/usr/bin/env python3
"""Extract normalized block-section corpora from pinned vanilla Java saves."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import math
import re
import struct
import sys
import zlib
from dataclasses import dataclass
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import section_corpus  # noqa: E402
import state_data  # noqa: E402

EXTRACTOR_ID = "vanilla-save-region-v1-stored-sections"
SECTOR_BYTES = 4096
REGION_HEADER_BYTES = 8192
TARGET_DATA_VERSION = 4903
# Minecraft 26.1 moved all default dimensions below dimensions/<namespace>/<dimension>.
# Crucible targets 26.2 exactly, so legacy root/DIM-1/DIM1 paths are deliberately not accepted here.
STANDARD_DIMENSIONS = (
    ("minecraft:overworld", Path("dimensions/minecraft/overworld/region")),
    ("minecraft:the_nether", Path("dimensions/minecraft/the_nether/region")),
    ("minecraft:the_end", Path("dimensions/minecraft/the_end/region")),
)
REGION_NAME = re.compile(r"r\.(-?[0-9]+)\.(-?[0-9]+)\.mca\Z")


class ExtractorError(ValueError):
    """Raised when a vanilla save cannot be normalized safely."""


@dataclass(frozen=True, order=True)
class ExtractedSection:
    dimension: str
    chunk_x: int
    chunk_z: int
    section_y: int
    states: tuple[int, ...]


@dataclass(frozen=True)
class NbtListValue:
    element_type: int
    values: tuple[object, ...]


@dataclass(frozen=True)
class NbtIntArray:
    values: tuple[int, ...]


@dataclass(frozen=True)
class NbtLongArray:
    values: tuple[int, ...]


class NbtReader:
    """Strict big-endian Java NBT reader for cold qualification tooling."""

    def __init__(self, data: bytes):
        self.data = data
        self.offset = 0

    def _take(self, size: int) -> bytes:
        end = self.offset + size
        if size < 0 or end > len(self.data):
            raise ExtractorError("truncated NBT payload")
        value = self.data[self.offset:end]
        self.offset = end
        return value

    def _unpack(self, fmt: str):
        return struct.unpack(fmt, self._take(struct.calcsize(fmt)))[0]

    def _string(self) -> str:
        raw = self._take(self._unpack(">H"))
        try:
            return raw.decode("utf-8")
        except UnicodeDecodeError as error:
            raise ExtractorError("NBT string is not UTF-8") from error

    def _length(self, label: str) -> int:
        length = self._unpack(">i")
        if length < 0:
            raise ExtractorError(f"negative NBT {label} length")
        return length

    def payload(self, tag_type: int):
        if tag_type == 1:
            return self._unpack(">b")
        if tag_type == 2:
            return self._unpack(">h")
        if tag_type == 3:
            return self._unpack(">i")
        if tag_type == 4:
            return self._unpack(">q")
        if tag_type == 5:
            return self._unpack(">f")
        if tag_type == 6:
            return self._unpack(">d")
        if tag_type == 7:
            return self._take(self._length("byte-array"))
        if tag_type == 8:
            return self._string()
        if tag_type == 9:
            element_type = self._unpack(">B")
            length = self._length("list")
            if element_type == 0 and length != 0:
                raise ExtractorError("non-empty NBT list cannot have TAG_End element type")
            return NbtListValue(
                element_type,
                tuple(self.payload(element_type) for _ in range(length)),
            )
        if tag_type == 10:
            result: dict[str, object] = {}
            while True:
                child_type = self._unpack(">B")
                if child_type == 0:
                    return result
                name = self._string()
                if name in result:
                    raise ExtractorError(f"duplicate NBT compound key: {name}")
                result[name] = self.payload(child_type)
        if tag_type == 11:
            return NbtIntArray(
                tuple(self._unpack(">i") for _ in range(self._length("int-array")))
            )
        if tag_type == 12:
            return NbtLongArray(
                tuple(self._unpack(">q") for _ in range(self._length("long-array")))
            )
        raise ExtractorError(f"unsupported NBT tag type: {tag_type}")

    def root(self) -> dict[str, object]:
        if self._unpack(">B") != 10:
            raise ExtractorError("Java NBT root must be a compound")
        self._string()
        value = self.payload(10)
        if self.offset != len(self.data):
            raise ExtractorError("trailing bytes after NBT root")
        assert isinstance(value, dict)
        return value


def parse_nbt(data: bytes) -> dict[str, object]:
    return NbtReader(data).root()


def _require_compound(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise ExtractorError(f"{label} must be an NBT compound")
    return value


def _require_list(
    value: object,
    label: str,
    *,
    element_type: int | None = None,
) -> tuple[object, ...]:
    if not isinstance(value, NbtListValue):
        raise ExtractorError(f"{label} must be an NBT list")
    if element_type is not None and value.element_type != element_type:
        raise ExtractorError(
            f"{label} must have NBT element type {element_type}; got {value.element_type}"
        )
    return value.values


def _require_int(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ExtractorError(f"{label} must be an NBT integer")
    return value


def _require_str(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ExtractorError(f"{label} must be a non-empty NBT string")
    return value


def canonical_palette_key(entry: object) -> str:
    compound = _require_compound(entry, "block-state palette entry")
    name = _require_str(compound.get("Name"), "block-state palette Name")
    raw_properties = compound.get("Properties")
    if raw_properties is None:
        return name
    properties = _require_compound(raw_properties, "block-state palette Properties")
    parts: list[str] = []
    for key, value in properties.items():
        if not key or not isinstance(value, str) or not value:
            raise ExtractorError(
                "block-state palette Properties must map non-empty strings to strings"
            )
        parts.append(f"{key}={value}")
    return name if not parts else name + "[" + ",".join(sorted(parts)) + "]"


def load_state_identity_map(
    qualified_path: Path,
    state_manifest_path: Path,
) -> tuple[dict[str, int], dict[str, object]]:
    qualified = state_data.load(qualified_path)
    manifest = json.loads(state_manifest_path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict):
        raise ExtractorError("state-data manifest root must be an object")
    if (
        manifest.get("assignment_policy") != "vanilla-identity"
        or manifest.get("mapping") != "identity"
    ):
        raise ExtractorError(
            "section save extraction currently requires frozen vanilla-identity state IDs"
        )
    if state_data.digest(qualified) != manifest.get("input_digest"):
        raise ExtractorError(
            "qualified state dataset does not match committed state-data input digest"
        )
    if qualified.get("target") != manifest.get("target"):
        raise ExtractorError("qualified state dataset target disagrees with state-data manifest")
    states = qualified.get("states")
    assert isinstance(states, list)
    ordered = sorted(states, key=lambda state: int(state["vanilla_id"]))
    if [int(state["vanilla_id"]) for state in ordered] != list(range(len(ordered))):
        raise ExtractorError("qualified state dataset is not dense vanilla identity")
    mapping = {str(state["key"]): int(state["vanilla_id"]) for state in ordered}
    if len(mapping) != len(ordered):
        raise ExtractorError("qualified state dataset has duplicate canonical state identities")
    return mapping, manifest


def decode_block_states(
    block_states: object,
    state_ids: dict[str, int],
) -> tuple[int, ...]:
    compound = _require_compound(block_states, "section block_states")
    raw_palette = _require_list(
        compound.get("palette"),
        "section block_states.palette",
        element_type=10,
    )
    if not raw_palette:
        raise ExtractorError("section block-state palette must not be empty")
    palette: list[int] = []
    for raw_entry in raw_palette:
        key = canonical_palette_key(raw_entry)
        state = state_ids.get(key)
        if state is None:
            raise ExtractorError(
                f"saved block state is absent from qualified target identity map: {key}"
            )
        palette.append(state)

    raw_data = compound.get("data")
    if len(palette) == 1:
        if raw_data is not None and (
            not isinstance(raw_data, NbtLongArray) or raw_data.values
        ):
            raise ExtractorError(
                "single-state section must omit block_states.data or store an empty long array"
            )
        return (palette[0],) * section_corpus.SECTION_CELLS
    if not isinstance(raw_data, NbtLongArray):
        raise ExtractorError("section block_states.data must be an NBT long array")

    bits = max(4, (len(palette) - 1).bit_length())
    values_per_long = 64 // bits
    expected_longs = math.ceil(section_corpus.SECTION_CELLS / values_per_long)
    if len(raw_data.values) != expected_longs:
        raise ExtractorError(
            "section block_states.data length does not match palette cardinality: "
            f"palette={len(palette)} bits={bits} longs={len(raw_data.values)} "
            f"expected={expected_longs}"
        )
    mask = (1 << bits) - 1
    states: list[int] = []
    for cell in range(section_corpus.SECTION_CELLS):
        word = raw_data.values[cell // values_per_long] & 0xFFFF_FFFF_FFFF_FFFF
        palette_index = (word >> ((cell % values_per_long) * bits)) & mask
        if palette_index >= len(palette):
            raise ExtractorError(
                f"section block-state palette index {palette_index} outside 0..{len(palette) - 1}"
            )
        states.append(palette[palette_index])
    return tuple(states)


def _decompress_chunk(payload: bytes, compression: int) -> bytes:
    try:
        if compression == 1:
            return gzip.decompress(payload)
        if compression == 2:
            return zlib.decompress(payload)
        if compression == 3:
            return payload
    except (OSError, zlib.error) as error:
        raise ExtractorError(
            f"chunk decompression failed for compression {compression}"
        ) from error
    if compression == 4:
        raise ExtractorError(
            "LZ4-compressed region chunks are not supported by extractor v1; refusing to guess"
        )
    raise ExtractorError(f"unsupported region chunk compression type: {compression}")


def _chunk_payload(
    region_path: Path,
    region_bytes: bytes,
    absolute_x: int,
    absolute_z: int,
    offset_sectors: int,
    sector_count: int,
) -> bytes:
    if offset_sectors < 2 or sector_count <= 0:
        raise ExtractorError(f"invalid region location for chunk {absolute_x},{absolute_z}")
    start = offset_sectors * SECTOR_BYTES
    allocation_end = start + sector_count * SECTOR_BYTES
    if allocation_end > len(region_bytes) or start + 5 > len(region_bytes):
        raise ExtractorError(
            f"region location exceeds file for chunk {absolute_x},{absolute_z}"
        )
    length = int.from_bytes(region_bytes[start : start + 4], "big")
    if length < 1 or length > sector_count * SECTOR_BYTES - 4:
        raise ExtractorError(
            f"invalid region chunk length for {absolute_x},{absolute_z}: {length}"
        )
    compression_byte = region_bytes[start + 4]
    external = bool(compression_byte & 0x80)
    compression = compression_byte & 0x7F
    if external:
        external_path = region_path.parent / f"c.{absolute_x}.{absolute_z}.mcc"
        try:
            payload = external_path.read_bytes()
        except OSError as error:
            raise ExtractorError(f"missing external chunk payload: {external_path}") from error
    else:
        payload = region_bytes[start + 5 : start + 4 + length]
    return _decompress_chunk(payload, compression)


def _region_coordinates(path: Path) -> tuple[int, int]:
    match = REGION_NAME.fullmatch(path.name)
    if match is None:
        raise ExtractorError(f"invalid region filename: {path.name}")
    return int(match.group(1)), int(match.group(2))


def _region_locations(raw: bytes, path: Path) -> list[tuple[int, int]]:
    locations: list[tuple[int, int]] = []
    occupied = {0, 1}
    sector_total = len(raw) // SECTOR_BYTES
    for slot in range(1024):
        location = int.from_bytes(raw[slot * 4 : slot * 4 + 4], "big")
        if location == 0:
            locations.append((0, 0))
            continue
        offset = location >> 8
        count = location & 0xFF
        if offset < 2 or count <= 0 or offset + count > sector_total:
            raise ExtractorError(f"invalid region location entry {slot} in {path}")
        allocated = set(range(offset, offset + count))
        overlap = occupied.intersection(allocated)
        if overlap:
            raise ExtractorError(
                f"overlapping region sector allocation in {path}: slot={slot} sectors={sorted(overlap)}"
            )
        occupied.update(allocated)
        locations.append((offset, count))
    return locations


def extract_region(
    region_path: Path,
    dimension: str,
    state_ids: dict[str, int],
) -> list[ExtractedSection]:
    region_x, region_z = _region_coordinates(region_path)
    raw = region_path.read_bytes()
    if len(raw) < REGION_HEADER_BYTES or len(raw) % SECTOR_BYTES != 0:
        raise ExtractorError(f"region file is not sector-aligned: {region_path}")
    locations = _region_locations(raw, region_path)

    sections: list[ExtractedSection] = []
    for slot, (offset, count) in enumerate(locations):
        if offset == 0:
            continue
        chunk_x = region_x * 32 + slot % 32
        chunk_z = region_z * 32 + slot // 32
        root = parse_nbt(
            _chunk_payload(region_path, raw, chunk_x, chunk_z, offset, count)
        )
        data_version = _require_int(root.get("DataVersion"), "chunk DataVersion")
        if data_version != TARGET_DATA_VERSION:
            raise ExtractorError(
                f"chunk {chunk_x},{chunk_z} DataVersion {data_version} != {TARGET_DATA_VERSION}"
            )
        stored_x = _require_int(root.get("xPos"), "chunk xPos")
        stored_z = _require_int(root.get("zPos"), "chunk zPos")
        if (stored_x, stored_z) != (chunk_x, chunk_z):
            raise ExtractorError(
                f"region slot {chunk_x},{chunk_z} contains chunk {stored_x},{stored_z}"
            )
        raw_sections = _require_list(
            root.get("sections"), "chunk sections", element_type=10
        )
        seen_y: set[int] = set()
        for raw_section in raw_sections:
            section = _require_compound(raw_section, "chunk section")
            section_y = _require_int(section.get("Y"), "chunk section Y")
            if section_y in seen_y:
                raise ExtractorError(
                    f"chunk {chunk_x},{chunk_z} has duplicate section Y={section_y}"
                )
            seen_y.add(section_y)
            if "block_states" not in section:
                continue
            sections.append(
                ExtractedSection(
                    dimension,
                    chunk_x,
                    chunk_z,
                    section_y,
                    decode_block_states(section["block_states"], state_ids),
                )
            )
    return sections


def validate_level_dat(world: Path) -> None:
    path = world / "level.dat"
    try:
        raw = gzip.decompress(path.read_bytes())
    except (OSError, EOFError) as error:
        raise ExtractorError(f"could not read gzip-compressed level.dat: {path}") from error
    data = _require_compound(parse_nbt(raw).get("Data"), "level.dat Data")
    data_version = _require_int(data.get("DataVersion"), "level.dat DataVersion")
    if data_version != TARGET_DATA_VERSION:
        raise ExtractorError(
            f"level.dat DataVersion {data_version} != pinned target {TARGET_DATA_VERSION}"
        )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def source_inventory(
    world: Path,
    selected_region_dirs: list[Path],
) -> tuple[str, list[dict[str, str]]]:
    paths: set[Path] = {world / "level.dat"}
    for directory in selected_region_dirs:
        paths.update(path for path in directory.glob("r.*.*.mca") if REGION_NAME.fullmatch(path.name))
        paths.update(directory.glob("c.*.*.mcc"))
    if any(not path.is_file() for path in paths):
        raise ExtractorError("source inventory contains a missing/non-file path")
    entries: list[dict[str, str]] = []
    records: list[str] = []
    for path in sorted(paths, key=lambda item: item.relative_to(world).as_posix()):
        relative = path.relative_to(world).as_posix()
        digest = sha256_file(path)
        entries.append({"path": relative, "sha256": digest})
        records.append(f"{relative}\t{digest}\n")
    return hashlib.sha256("".join(records).encode("utf-8")).hexdigest(), entries


def discover_dimensions(
    world: Path,
    requested: set[str] | None,
) -> list[tuple[str, Path]]:
    known = {name for name, _ in STANDARD_DIMENSIONS}
    if requested is not None:
        unknown = sorted(requested - known)
        if unknown:
            raise ExtractorError(
                f"extractor v1 does not support requested dimensions: {unknown}"
            )
    selected = [
        (name, world / relative)
        for name, relative in STANDARD_DIMENSIONS
        if (requested is None or name in requested) and (world / relative).is_dir()
    ]
    if not selected:
        raise ExtractorError("no selected Minecraft 26.2 dimension region directories exist")
    return selected


def render_corpus(
    sections: list[ExtractedSection],
    state_manifest: dict[str, object],
    inventory_sha256: str,
) -> str:
    if not sections:
        raise ExtractorError("stored-section extraction produced no block sections")
    target = state_manifest.get("target")
    if not isinstance(target, dict):
        raise ExtractorError("state-data manifest target is invalid")
    lines = [
        section_corpus.MAGIC,
        "TARGET|"
        f"minecraft={target['minecraft_version']}|protocol={target['protocol_version']}|"
        f"data={target['data_version']}|state_count={state_manifest['state_count']}|"
        f"generation_sha256={state_manifest['generation_digest']}",
        "SOURCE|"
        f"kind=vanilla-save|inventory_sha256={inventory_sha256}|extractor={EXTRACTOR_ID}",
    ]
    for section in sorted(sections):
        lines.append(
            "SECTION|"
            f"{section.dimension}|{section.chunk_x}|{section.chunk_z}|{section.section_y}|"
            + ",".join(str(state) for state in section.states)
        )
    return "\n".join(lines) + "\n"


def extract_world(
    world: Path,
    qualified_states: Path,
    state_manifest_path: Path,
    generated_rust_path: Path,
    output: Path,
    inventory_output: Path | None,
    dimensions: set[str] | None,
) -> section_corpus.ParsedCorpus:
    validate_level_dat(world)
    state_ids, state_manifest = load_state_identity_map(
        qualified_states, state_manifest_path
    )
    selected = discover_dimensions(world, dimensions)
    inventory_sha256, inventory_entries = source_inventory(
        world, [directory for _, directory in selected]
    )
    sections: list[ExtractedSection] = []
    for dimension, directory in selected:
        for region_path in sorted(directory.glob("r.*.*.mca"), key=lambda path: path.name):
            if REGION_NAME.fullmatch(region_path.name):
                sections.extend(extract_region(region_path, dimension, state_ids))
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        render_corpus(sections, state_manifest, inventory_sha256),
        encoding="utf-8",
        newline="\n",
    )
    target = section_corpus.load_target_evidence(
        state_manifest_path, generated_rust_path
    )
    parsed = section_corpus.validate_corpus(output, target)
    if inventory_output is not None:
        inventory_output.parent.mkdir(parents=True, exist_ok=True)
        inventory_output.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "policy": EXTRACTOR_ID,
                    "world": str(world),
                    "inventory_sha256": inventory_sha256,
                    "files": inventory_entries,
                    "corpus_sha256": parsed.corpus_sha256,
                    "section_count": parsed.section_count,
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
    return parsed


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--world", type=Path, required=True)
    parser.add_argument(
        "--qualified-states",
        type=Path,
        default=Path(".crucible/vanilla/26.2-block-states.qualified.json"),
    )
    parser.add_argument(
        "--state-manifest",
        type=Path,
        default=Path("vanilla/state-data/26.2-state-data-manifest.json"),
    )
    parser.add_argument(
        "--generated-rust",
        type=Path,
        default=Path("crates/data/helve-generated/src/lib.rs"),
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--inventory", type=Path)
    parser.add_argument(
        "--dimension",
        action="append",
        choices=[name for name, _ in STANDARD_DIMENSIONS],
        help="limit extraction to one or more standard dimensions",
    )
    return parser


def main() -> int:
    args = _parser().parse_args()
    if not args.world.is_dir():
        print(f"section extraction error: world directory does not exist: {args.world}", file=sys.stderr)
        return 1
    try:
        parsed = extract_world(
            args.world,
            args.qualified_states,
            args.state_manifest,
            args.generated_rust,
            args.output,
            args.inventory,
            set(args.dimension) if args.dimension else None,
        )
    except (
        ExtractorError,
        section_corpus.CorpusError,
        ValueError,
        OSError,
        json.JSONDecodeError,
    ) as error:
        print(f"section extraction error: {error}", file=sys.stderr)
        return 1
    print(
        "section extraction: "
        f"policy={EXTRACTOR_ID} sections={parsed.section_count} "
        f"states={parsed.distinct_state_ids} corpus_sha256={parsed.corpus_sha256} PASS"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
