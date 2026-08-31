#!/usr/bin/env python3
"""Differentially compare Helve's Rust stored-world importer with the Python oracle."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import math
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from dataclasses import dataclass
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import vanilla_section_extractor as oracle  # noqa: E402

SECTOR_BYTES = 4096
REGION_HEADER_BYTES = 2 * SECTOR_BYTES
DIMENSION = "minecraft:overworld"
# This synthetic gate isolates region/NBT/compression/palette interpretation. State-name -> dense-ID
# authority is separately source/runtime qualified by the stored-state lookup gate. These two stable
# identities are also asserted by the cross-language result itself.
FIXTURE_STATE_IDS = {
    "minecraft:air": 0,
    "minecraft:stone": 1,
}


class DifferentialError(RuntimeError):
    """Raised when fixture construction or cross-language semantics disagree."""


@dataclass(frozen=True)
class SectionSpec:
    section_y: int
    palette: tuple[str, ...] | None
    indices: tuple[int, ...] | None = None


@dataclass(frozen=True)
class ChunkSpec:
    chunk_x: int
    chunk_z: int
    compression: int
    external: bool
    sections: tuple[SectionSpec, ...]


def _name(value: str) -> bytes:
    raw = value.encode("utf-8")
    if len(raw) > 0xFFFF:
        raise DifferentialError("fixture NBT name exceeds u16")
    return struct.pack(">H", len(raw)) + raw


def _named_header(tag_type: int, name: str) -> bytes:
    return bytes((tag_type,)) + _name(name)


def _int_field(name: str, value: int) -> bytes:
    return _named_header(3, name) + struct.pack(">i", value)


def _byte_field(name: str, value: int) -> bytes:
    if not -128 <= value <= 127:
        raise DifferentialError(f"fixture byte outside i8: {value}")
    return _named_header(1, name) + struct.pack(">b", value)


def _string_field(name: str, value: str) -> bytes:
    return _named_header(8, name) + _name(value)


def _packed_words(palette_len: int, indices: tuple[int, ...]) -> tuple[int, ...]:
    if len(indices) != 4096:
        raise DifferentialError("packed fixture section must contain exactly 4096 indices")
    if palette_len <= 1:
        raise DifferentialError("packed words require a multi-entry palette")
    bits = max(4, (palette_len - 1).bit_length())
    per_word = 64 // bits
    words = [0] * math.ceil(4096 / per_word)
    mask = (1 << bits) - 1
    for cell, index in enumerate(indices):
        if not 0 <= index < palette_len:
            raise DifferentialError(f"fixture palette index outside palette: {index}")
        words[cell // per_word] |= (index & mask) << ((cell % per_word) * bits)
    return tuple(words)


def _long_array_field(name: str, words: tuple[int, ...]) -> bytes:
    payload = bytearray(_named_header(12, name))
    payload.extend(struct.pack(">i", len(words)))
    for word in words:
        signed = word if word < 1 << 63 else word - (1 << 64)
        payload.extend(struct.pack(">q", signed))
    return bytes(payload)


def _section_payload(spec: SectionSpec) -> bytes:
    payload = bytearray(_byte_field("Y", spec.section_y))
    if spec.palette is None:
        if spec.indices is not None:
            raise DifferentialError("section without block_states cannot carry indices")
        payload.append(0)
        return bytes(payload)

    if not spec.palette:
        raise DifferentialError("fixture block-state palette cannot be empty")
    payload.extend(_named_header(10, "block_states"))
    payload.extend(_named_header(9, "palette"))
    payload.append(10)
    payload.extend(struct.pack(">i", len(spec.palette)))
    for state_name in spec.palette:
        payload.extend(_string_field("Name", state_name))
        payload.append(0)
    if len(spec.palette) == 1:
        if spec.indices is not None:
            raise DifferentialError("uniform fixture section must omit packed indices")
    else:
        if spec.indices is None:
            raise DifferentialError("multi-state fixture section requires packed indices")
        payload.extend(_long_array_field("data", _packed_words(len(spec.palette), spec.indices)))
    payload.append(0)  # block_states compound
    payload.append(0)  # section compound
    return bytes(payload)


def _chunk_nbt(spec: ChunkSpec) -> bytes:
    payload = bytearray((10, 0, 0))  # unnamed root compound
    payload.extend(_int_field("DataVersion", 4903))
    payload.extend(_int_field("xPos", spec.chunk_x))
    payload.extend(_int_field("zPos", spec.chunk_z))
    payload.extend(_named_header(9, "sections"))
    payload.append(10)
    payload.extend(struct.pack(">i", len(spec.sections)))
    for section in spec.sections:
        payload.extend(_section_payload(section))
    payload.append(0)
    return bytes(payload)


def _compress(payload: bytes, compression: int) -> bytes:
    if compression == 1:
        return gzip.compress(payload, compresslevel=6, mtime=0)
    if compression == 2:
        return zlib.compress(payload, level=6)
    if compression == 3:
        return payload
    raise DifferentialError(f"unsupported fixture compression: {compression}")


def _region_slot(region_x: int, region_z: int, chunk_x: int, chunk_z: int) -> tuple[int, int, int]:
    local_x = chunk_x - region_x * 32
    local_z = chunk_z - region_z * 32
    if not 0 <= local_x < 32 or not 0 <= local_z < 32:
        raise DifferentialError(
            f"chunk {chunk_x},{chunk_z} is outside region {region_x},{region_z}"
        )
    return local_x, local_z, local_z * 32 + local_x


def _write_region(path: Path, region_x: int, region_z: int, chunks: tuple[ChunkSpec, ...]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    region = bytearray(REGION_HEADER_BYTES)
    next_sector = 2
    seen_slots: set[int] = set()

    for chunk in chunks:
        _, _, slot = _region_slot(region_x, region_z, chunk.chunk_x, chunk.chunk_z)
        if slot in seen_slots:
            raise DifferentialError(f"duplicate fixture region slot: {slot}")
        seen_slots.add(slot)
        compressed = _compress(_chunk_nbt(chunk), chunk.compression)
        if chunk.external:
            external_path = path.parent / f"c.{chunk.chunk_x}.{chunk.chunk_z}.mcc"
            external_path.write_bytes(compressed)
            record = struct.pack(">I", 1) + bytes((chunk.compression | 0x80,))
        else:
            record = struct.pack(">I", len(compressed) + 1) + bytes((chunk.compression,)) + compressed
        sector_count = math.ceil(len(record) / SECTOR_BYTES)
        if not 1 <= sector_count <= 0xFF:
            raise DifferentialError("fixture region record exceeds Anvil sector-count byte")
        location = (next_sector << 8) | sector_count
        region[slot * 4 : slot * 4 + 4] = location.to_bytes(4, "big")
        timestamp = 0x6500_0000 + slot
        timestamp_offset = SECTOR_BYTES + slot * 4
        region[timestamp_offset : timestamp_offset + 4] = timestamp.to_bytes(4, "big")
        region.extend(record)
        region.extend(b"\x00" * (sector_count * SECTOR_BYTES - len(record)))
        next_sector += sector_count

    path.write_bytes(region)


def _binary_pattern() -> tuple[int, ...]:
    return tuple(((cell * 5) + (cell // 16) + (cell // 257)) & 1 for cell in range(4096))


def _wide_palette_pattern() -> tuple[int, ...]:
    # Seventeen entries force five-bit non-spanning storage. Duplicate semantic identities are
    # intentional here: the test targets packed index/cell interpretation, not writer palette dedupe.
    return tuple(((cell * 7) + (cell // 13) + (cell // 129)) % 17 for cell in range(4096))


def _fixture_regions(root: Path) -> tuple[Path, ...]:
    region_dir = root / "region"
    palette17 = tuple(
        "minecraft:air" if index % 2 == 0 else "minecraft:stone" for index in range(17)
    )
    _write_region(
        region_dir / "r.0.0.mca",
        0,
        0,
        (
            ChunkSpec(
                0,
                0,
                2,
                False,
                (
                    SectionSpec(0, ("minecraft:air",)),
                    SectionSpec(1, None),
                ),
            ),
            ChunkSpec(
                1,
                0,
                1,
                False,
                (SectionSpec(-4, ("minecraft:air", "minecraft:stone"), _binary_pattern()),),
            ),
            ChunkSpec(
                2,
                0,
                3,
                False,
                (
                    SectionSpec(5, ("minecraft:stone",)),
                    SectionSpec(-1, ("minecraft:air",)),
                ),
            ),
            ChunkSpec(
                3,
                0,
                2,
                True,
                (SectionSpec(2, palette17, _wide_palette_pattern()),),
            ),
        ),
    )
    _write_region(
        region_dir / "r.-1.-1.mca",
        -1,
        -1,
        (
            ChunkSpec(
                -32,
                -32,
                3,
                False,
                (SectionSpec(-4, ("minecraft:stone",)),),
            ),
            ChunkSpec(
                -1,
                -1,
                1,
                False,
                (
                    SectionSpec(
                        3,
                        ("minecraft:stone", "minecraft:air"),
                        _binary_pattern(),
                    ),
                ),
            ),
        ),
    )
    return tuple(sorted(region_dir.glob("r.*.*.mca"), key=lambda item: item.name))


def _oracle_lines(region_path: Path) -> list[str]:
    sections = oracle.extract_region(region_path, DIMENSION, FIXTURE_STATE_IDS)
    lines: list[str] = []
    for section in sorted(sections):
        lines.append(
            "SECTION|"
            f"{section.dimension}|{section.chunk_x}|{section.chunk_z}|{section.section_y}|"
            + ",".join(str(state) for state in section.states)
        )
    return lines


def _rust_lines(emitter: Path, region_path: Path, output: Path) -> list[str]:
    result = subprocess.run(
        [str(emitter), DIMENSION, str(region_path), str(output)],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise DifferentialError(
            "Rust importer fact emitter failed for "
            f"{region_path.name}: stdout={result.stdout!r} stderr={result.stderr!r}"
        )
    return [line for line in output.read_text(encoding="utf-8").splitlines() if line]


def _first_difference(expected: list[str], actual: list[str]) -> str:
    for index, (left, right) in enumerate(zip(expected, actual, strict=False)):
        if left != right:
            return f"line {index}: oracle={left[:240]!r} rust={right[:240]!r}"
    return f"line-count mismatch: oracle={len(expected)} rust={len(actual)}"


def run(emitter: Path, work_dir: Path) -> str:
    if not emitter.is_file():
        raise DifferentialError(f"Rust importer emitter does not exist: {emitter}")
    if work_dir.exists():
        shutil.rmtree(work_dir)
    work_dir.mkdir(parents=True)

    combined: list[str] = []
    for region_path in _fixture_regions(work_dir):
        expected = _oracle_lines(region_path)
        output = work_dir / f"{region_path.name}.rust-sections.txt"
        actual = _rust_lines(emitter, region_path, output)
        if expected != actual:
            raise DifferentialError(
                f"semantic mismatch for {region_path.name}: {_first_difference(expected, actual)}"
            )
        combined.extend(expected)

    if len(combined) != 7:
        raise DifferentialError(f"unexpected fixture block-section count: {len(combined)}")
    digest = hashlib.sha256(("\n".join(combined) + "\n").encode("utf-8")).hexdigest()
    return digest


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rust-emitter", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path)
    return parser


def main() -> int:
    args = _parser().parse_args()
    temporary: tempfile.TemporaryDirectory[str] | None = None
    try:
        if args.work_dir is None:
            temporary = tempfile.TemporaryDirectory(prefix="helve-r2c-import-diff-")
            work_dir = Path(temporary.name)
        else:
            work_dir = args.work_dir
        digest = run(args.rust_emitter, work_dir)
    except (DifferentialError, oracle.ExtractorError, OSError, ValueError) as error:
        print(f"R2C importer differential: FAIL: {error}", file=sys.stderr)
        return 1
    finally:
        if temporary is not None:
            temporary.cleanup()
    print(f"R2C importer differential: sections=7 sha256={digest} PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
