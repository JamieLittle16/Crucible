#!/usr/bin/env python3
"""Read pinned vanilla saved-chunk status evidence for qualification corpora."""

from __future__ import annotations

from collections import Counter
from dataclasses import dataclass
from pathlib import Path

import vanilla_section_extractor as base

FULL_CHUNK_STATUS = "minecraft:full"


class ChunkStatusError(ValueError):
    """Raised when selected vanilla chunks cannot be status-qualified safely."""


@dataclass(frozen=True, order=True)
class ChunkStatusRecord:
    dimension: str
    chunk_x: int
    chunk_z: int
    status: str


def _require_status(root: dict[str, object], chunk_x: int, chunk_z: int) -> str:
    value = root.get("Status")
    if not isinstance(value, str) or not value:
        raise ChunkStatusError(
            f"chunk {chunk_x},{chunk_z} must contain a non-empty saved Status string"
        )
    return value


def extract_region_statuses(region_path: Path, dimension: str) -> list[ChunkStatusRecord]:
    """Return strict status records for every stored chunk in one region file."""

    region_x, region_z = base._region_coordinates(region_path)
    raw = region_path.read_bytes()
    if len(raw) < base.REGION_HEADER_BYTES or len(raw) % base.SECTOR_BYTES != 0:
        raise ChunkStatusError(f"region file is not sector-aligned: {region_path}")
    locations = base._region_locations(raw, region_path)

    records: list[ChunkStatusRecord] = []
    for slot, (offset, count) in enumerate(locations):
        if offset == 0:
            continue
        chunk_x = region_x * 32 + slot % 32
        chunk_z = region_z * 32 + slot // 32
        try:
            root = base.parse_nbt(
                base._chunk_payload(region_path, raw, chunk_x, chunk_z, offset, count)
            )
            data_version = base._require_int(root.get("DataVersion"), "chunk DataVersion")
            stored_x = base._require_int(root.get("xPos"), "chunk xPos")
            stored_z = base._require_int(root.get("zPos"), "chunk zPos")
        except base.ExtractorError as error:
            raise ChunkStatusError(str(error)) from error
        if data_version != base.TARGET_DATA_VERSION:
            raise ChunkStatusError(
                f"chunk {chunk_x},{chunk_z} DataVersion {data_version} != {base.TARGET_DATA_VERSION}"
            )
        if (stored_x, stored_z) != (chunk_x, chunk_z):
            raise ChunkStatusError(
                f"region slot {chunk_x},{chunk_z} contains chunk {stored_x},{stored_z}"
            )
        records.append(
            ChunkStatusRecord(
                dimension=dimension,
                chunk_x=chunk_x,
                chunk_z=chunk_z,
                status=_require_status(root, chunk_x, chunk_z),
            )
        )
    return records


def qualify_selected_chunks(
    regions: dict[str, set[Path]],
    selection: dict[str, set[tuple[int, int]]],
    *,
    required_status: str = FULL_CHUNK_STATUS,
) -> dict[str, int]:
    """Require every selected chunk exactly once and at the required saved status."""

    selected_records: dict[tuple[str, int, int], ChunkStatusRecord] = {}
    for dimension, region_paths in regions.items():
        if dimension not in selection:
            raise ChunkStatusError(f"status scan has unplanned dimension {dimension}")
        selected = selection[dimension]
        for region_path in sorted(region_paths, key=lambda path: path.as_posix()):
            for record in extract_region_statuses(region_path, dimension):
                coordinates = (record.chunk_x, record.chunk_z)
                if coordinates not in selected:
                    continue
                key = (dimension, record.chunk_x, record.chunk_z)
                if key in selected_records:
                    raise ChunkStatusError(f"selected chunk appears more than once in status scan: {key}")
                selected_records[key] = record

    expected = {
        (dimension, chunk_x, chunk_z)
        for dimension, chunks in selection.items()
        for chunk_x, chunk_z in chunks
    }
    observed = set(selected_records)
    missing = sorted(expected - observed)
    extra = sorted(observed - expected)
    if missing or extra:
        raise ChunkStatusError(
            f"selected chunk status identity mismatch: missing={missing[:8]} extra={extra[:8]}"
        )

    invalid = [
        record
        for record in selected_records.values()
        if record.status != required_status
    ]
    if invalid:
        preview = [
            (record.dimension, record.chunk_x, record.chunk_z, record.status)
            for record in sorted(invalid)[:8]
        ]
        raise ChunkStatusError(
            f"representative chunks must be saved at Status={required_status}; got {preview}"
        )

    histogram = Counter(record.status for record in selected_records.values())
    return dict(sorted(histogram.items()))
