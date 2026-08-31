#!/usr/bin/env python3
"""Compare production Rust Anvil import with the normalized official-save Python corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

REGION_NAME = re.compile(r"r\.(-?[0-9]+)\.(-?[0-9]+)\.mca\Z")
DIMENSION_REGIONS = {
    "minecraft:overworld": Path("dimensions/minecraft/overworld/region"),
    "minecraft:the_nether": Path("dimensions/minecraft/the_nether/region"),
    "minecraft:the_end": Path("dimensions/minecraft/the_end/region"),
}
POLICY = "production-anvil-vs-python-corpus-v1"


class DifferentialError(RuntimeError):
    """Raised when official-save normalized semantics disagree."""


def _region_coordinates(path: Path) -> tuple[int, int]:
    match = REGION_NAME.fullmatch(path.name)
    if match is None:
        raise DifferentialError(f"invalid region filename: {path.name}")
    return int(match.group(1)), int(match.group(2))


def _section_header(line: bytes) -> tuple[str, int, int, int]:
    try:
        tag, dimension, raw_x, raw_z, raw_y, states = line.rstrip(b"\n").split(b"|", 5)
    except ValueError as error:
        raise DifferentialError("malformed normalized SECTION line") from error
    if tag != b"SECTION" or not states:
        raise DifferentialError("malformed normalized SECTION payload")
    try:
        decoded_dimension = dimension.decode("utf-8")
        chunk_x = int(raw_x)
        chunk_z = int(raw_z)
        section_y = int(raw_y)
    except (UnicodeDecodeError, ValueError) as error:
        raise DifferentialError("invalid normalized SECTION coordinate/header") from error
    return decoded_dimension, chunk_x, chunk_z, section_y


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _first_difference(expected: Path, actual: Path) -> str:
    with expected.open("rb") as left, actual.open("rb") as right:
        line_number = 0
        while True:
            lhs = left.readline()
            rhs = right.readline()
            if not lhs and not rhs:
                return "digests differ but no differing line was located"
            line_number += 1
            if lhs == rhs:
                continue
            if not lhs:
                return f"line {line_number}: Rust emitted an unexpected extra section"
            if not rhs:
                return f"line {line_number}: Rust omitted an expected section"
            try:
                ltag, ldim, lx, lz, ly, lstates = lhs.rstrip(b"\n").split(b"|", 5)
                rtag, rdim, rx, rz, ry, rstates = rhs.rstrip(b"\n").split(b"|", 5)
            except ValueError:
                return f"line {line_number}: malformed section fact on one side"
            if (ltag, ldim, lx, lz, ly) != (rtag, rdim, rx, rz, ry):
                return (
                    f"line {line_number}: section identity differs: "
                    f"oracle={(ldim, lx, lz, ly)!r} rust={(rdim, rx, rz, ry)!r}"
                )
            left_states = lstates.split(b",")
            right_states = rstates.split(b",")
            if len(left_states) != len(right_states):
                return (
                    f"line {line_number}: cell count differs for "
                    f"{ldim.decode(errors='replace')}:{lx.decode()},{lz.decode()},{ly.decode()}: "
                    f"oracle={len(left_states)} rust={len(right_states)}"
                )
            for cell, (left_state, right_state) in enumerate(
                zip(left_states, right_states, strict=True)
            ):
                if left_state != right_state:
                    return (
                        f"line {line_number}: state differs at cell={cell} for "
                        f"{ldim.decode(errors='replace')}:{lx.decode()},{lz.decode()},{ly.decode()}: "
                        f"oracle={left_state.decode(errors='replace')} "
                        f"rust={right_state.decode(errors='replace')}"
                    )
            return f"line {line_number}: section bytes differ outside parsed semantic fields"


def _partition_expected(
    corpus: Path,
    expected_dir: Path,
    dimension: str,
    region_paths: dict[tuple[int, int], Path],
) -> tuple[int, str]:
    expected_dir.mkdir(parents=True, exist_ok=True)
    handles: dict[tuple[int, int], object] = {}
    section_count = 0
    semantic_digest = hashlib.sha256()
    try:
        with corpus.open("rb") as source:
            for line in source:
                if not line.startswith(b"SECTION|"):
                    continue
                line_dimension, chunk_x, chunk_z, _ = _section_header(line)
                if line_dimension != dimension:
                    raise DifferentialError(
                        f"corpus contains unexpected dimension {line_dimension!r}; expected {dimension!r}"
                    )
                region = (chunk_x // 32, chunk_z // 32)
                if region not in region_paths:
                    raise DifferentialError(
                        f"corpus section {chunk_x},{chunk_z} maps to absent region {region}"
                    )
                handle = handles.get(region)
                if handle is None:
                    handle = (expected_dir / f"r.{region[0]}.{region[1]}.expected").open("wb")
                    handles[region] = handle
                handle.write(line)
                semantic_digest.update(line)
                section_count += 1
    finally:
        for handle in handles.values():
            handle.close()
    if section_count == 0:
        raise DifferentialError("normalized official-save corpus contains no SECTION records")
    return section_count, semantic_digest.hexdigest()


def run(
    world: Path,
    corpus: Path,
    rust_emitter: Path,
    work_dir: Path,
    dimension: str,
) -> tuple[int, int, str]:
    relative_region_dir = DIMENSION_REGIONS.get(dimension)
    if relative_region_dir is None:
        raise DifferentialError(f"unsupported differential dimension: {dimension}")
    region_dir = world / relative_region_dir
    if not region_dir.is_dir():
        raise DifferentialError(f"official world region directory is absent: {region_dir}")
    if not corpus.is_file():
        raise DifferentialError(f"normalized Python corpus is absent: {corpus}")
    if not rust_emitter.is_file():
        raise DifferentialError(f"Rust importer emitter is absent: {rust_emitter}")

    region_paths: dict[tuple[int, int], Path] = {}
    for path in sorted(region_dir.glob("r.*.*.mca"), key=lambda item: item.name):
        coordinates = _region_coordinates(path)
        if coordinates in region_paths:
            raise DifferentialError(f"duplicate region coordinates: {coordinates}")
        region_paths[coordinates] = path
    if not region_paths:
        raise DifferentialError("official world contains no selected Anvil region files")

    if work_dir.exists():
        import shutil

        shutil.rmtree(work_dir)
    expected_dir = work_dir / "expected"
    actual_dir = work_dir / "actual"
    actual_dir.mkdir(parents=True)

    section_count, semantic_sha256 = _partition_expected(
        corpus, expected_dir, dimension, region_paths
    )

    # Compare every actual region, including a region for which the oracle emitted zero sections.
    # An empty expected file makes unexpected Rust-only sections observable rather than silently
    # skipping an unreferenced region.
    for region in region_paths:
        expected = expected_dir / f"r.{region[0]}.{region[1]}.expected"
        if not expected.exists():
            expected.touch()

    compared_regions = 0
    compared_sections = 0
    for region in sorted(region_paths):
        region_path = region_paths[region]
        expected = expected_dir / f"r.{region[0]}.{region[1]}.expected"
        actual = actual_dir / f"r.{region[0]}.{region[1]}.actual"
        result = subprocess.run(
            [str(rust_emitter), dimension, str(region_path), str(actual)],
            check=False,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            raise DifferentialError(
                f"Rust importer failed for {region_path.name}: "
                f"stdout={result.stdout!r} stderr={result.stderr!r}"
            )
        expected_sha = _sha256(expected)
        actual_sha = _sha256(actual)
        if expected_sha != actual_sha:
            raise DifferentialError(
                f"semantic mismatch for {region_path.name}: {_first_difference(expected, actual)}"
            )
        with expected.open("rb") as handle:
            compared_sections += sum(1 for _ in handle)
        compared_regions += 1

    if compared_sections != section_count:
        raise DifferentialError(
            f"comparison did not cover the complete corpus: expected={section_count} "
            f"compared={compared_sections}"
        )
    return compared_regions, section_count, semantic_sha256


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--world", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--rust-emitter", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--dimension", default="minecraft:overworld")
    parser.add_argument("--evidence", type=Path)
    return parser


def main() -> int:
    args = _parser().parse_args()
    try:
        regions, sections, digest = run(
            args.world,
            args.corpus,
            args.rust_emitter,
            args.work_dir,
            args.dimension,
        )
        corpus_sha256 = _sha256(args.corpus)
        if args.evidence is not None:
            args.evidence.parent.mkdir(parents=True, exist_ok=True)
            args.evidence.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "policy": POLICY,
                        "dimension": args.dimension,
                        "region_count": regions,
                        "section_count": sections,
                        "cell_count": sections * 4096,
                        "semantic_sha256": digest,
                        "corpus_sha256": corpus_sha256,
                    },
                    indent=2,
                    sort_keys=True,
                )
                + "\n",
                encoding="utf-8",
            )
    except (DifferentialError, OSError, ValueError) as error:
        print(f"R2C genuine-save differential: FAIL: {error}", file=sys.stderr)
        return 1
    print(
        "R2C genuine-save differential: "
        f"regions={regions} sections={sections} cells={sections * 4096} "
        f"semantic_sha256={digest} corpus_sha256={corpus_sha256} PASS"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
