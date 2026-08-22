#!/usr/bin/env python3
"""Extract one plan-bound representative corpus member from an official vanilla save."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import defaultdict
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

import section_corpus  # noqa: E402
import section_representative_plan  # noqa: E402
import vanilla_dimensions  # noqa: E402
import vanilla_section_extractor as base  # noqa: E402

EXTRACTOR_ID = "vanilla-save-region-v2-representative-member"


class RepresentativeCorpusError(ValueError):
    """Raised when a source world does not satisfy the frozen representative plan."""


def selected_chunks(plan: dict[str, object]) -> dict[str, set[tuple[int, int]]]:
    dimensions = plan["dimensions"]
    assert isinstance(dimensions, dict)
    result: dict[str, set[tuple[int, int]]] = {}
    for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS:
        entry = dimensions[descriptor.key]
        assert isinstance(entry, dict)
        chunks = entry["chunks"]
        assert isinstance(chunks, list)
        result[descriptor.key] = {(int(chunk[0]), int(chunk[1])) for chunk in chunks}
    return result


def _dimension_dirs(world: Path) -> dict[str, Path]:
    return {
        descriptor.key: world / descriptor.region_path
        for descriptor in vanilla_dimensions.STANDARD_DIMENSIONS
    }


def selected_region_paths(
    world: Path,
    selection: dict[str, set[tuple[int, int]]],
) -> dict[str, set[Path]]:
    directories = _dimension_dirs(world)
    result: dict[str, set[Path]] = {}
    for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS:
        dimension = descriptor.key
        chunks = selection[dimension]
        directory = directories[dimension]
        if not directory.is_dir():
            raise RepresentativeCorpusError(
                f"selected dimension region directory is missing: {dimension} -> {directory}"
            )
        paths = {
            directory / f"r.{chunk_x // 32}.{chunk_z // 32}.mca"
            for chunk_x, chunk_z in chunks
        }
        missing = sorted(str(path) for path in paths if not path.is_file())
        if missing:
            raise RepresentativeCorpusError(
                f"selected chunks are absent from region storage for {dimension}: {missing[:8]}"
            )
        result[dimension] = paths
    return result


def source_inventory(
    world: Path,
    regions: dict[str, set[Path]],
) -> tuple[str, list[dict[str, str]]]:
    paths: set[Path] = {world / "level.dat"}
    selected_directories: set[Path] = set()
    for region_paths in regions.values():
        paths.update(region_paths)
        selected_directories.update(path.parent for path in region_paths)
    # External chunk payloads are rare but legal. Hash every external payload in a selected
    # region directory so the provenance snapshot cannot change underneath extraction.
    for directory in selected_directories:
        paths.update(directory.glob("c.*.*.mcc"))
    if any(not path.is_file() for path in paths):
        raise RepresentativeCorpusError("representative source inventory contains a missing file")

    entries: list[dict[str, str]] = []
    records: list[str] = []
    for path in sorted(paths, key=lambda item: item.relative_to(world).as_posix()):
        relative = path.relative_to(world).as_posix()
        digest = base.sha256_file(path)
        entries.append({"path": relative, "sha256": digest})
        records.append(f"{relative}\t{digest}\n")
    inventory = hashlib.sha256("".join(records).encode("utf-8")).hexdigest()
    return inventory, entries


def extract_selected_sections(
    regions: dict[str, set[Path]],
    selection: dict[str, set[tuple[int, int]]],
    state_ids: dict[str, int],
) -> list[base.ExtractedSection]:
    sections: list[base.ExtractedSection] = []
    for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS:
        dimension = descriptor.key
        selected = selection[dimension]
        for region_path in sorted(regions[dimension], key=lambda path: path.as_posix()):
            for section in base.extract_region(region_path, dimension, state_ids):
                if (section.chunk_x, section.chunk_z) in selected:
                    sections.append(section)
    return sections


def validate_selected_sections(
    sections: list[base.ExtractedSection],
    selection: dict[str, set[tuple[int, int]]],
) -> dict[str, list[int]]:
    by_chunk: dict[tuple[str, int, int], list[int]] = defaultdict(list)
    for section in sections:
        by_chunk[(section.dimension, section.chunk_x, section.chunk_z)].append(section.section_y)

    lattice: dict[str, list[int]] = {}
    for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS:
        dimension = descriptor.key
        expected_chunks = selection[dimension]
        observed_chunks = {
            (chunk_x, chunk_z)
            for (observed_dimension, chunk_x, chunk_z) in by_chunk
            if observed_dimension == dimension
        }
        missing = sorted(expected_chunks - observed_chunks)
        extra = sorted(observed_chunks - expected_chunks)
        if missing or extra:
            raise RepresentativeCorpusError(
                f"selected chunk identity mismatch for {dimension}: missing={missing[:8]} extra={extra[:8]}"
            )

        expected_lattice: list[int] | None = None
        for chunk_x, chunk_z in sorted(expected_chunks):
            ys = sorted(by_chunk[(dimension, chunk_x, chunk_z)])
            if len(ys) != len(set(ys)):
                raise RepresentativeCorpusError(
                    f"duplicate section Y in selected chunk {dimension} {chunk_x},{chunk_z}"
                )
            if not ys:
                raise RepresentativeCorpusError(
                    f"selected chunk has no block sections: {dimension} {chunk_x},{chunk_z}"
                )
            contiguous = list(range(ys[0], ys[-1] + 1))
            if ys != contiguous:
                raise RepresentativeCorpusError(
                    f"selected chunk section lattice is not contiguous: {dimension} {chunk_x},{chunk_z} -> {ys}"
                )
            if expected_lattice is None:
                expected_lattice = ys
            elif ys != expected_lattice:
                raise RepresentativeCorpusError(
                    f"selected chunks disagree on section lattice for {dimension}: "
                    f"expected {expected_lattice}, got {ys} at {chunk_x},{chunk_z}"
                )
        assert expected_lattice is not None
        lattice[dimension] = expected_lattice
    return lattice


def render_corpus(
    sections: list[base.ExtractedSection],
    state_manifest: dict[str, object],
    inventory_sha256: str,
) -> str:
    target = state_manifest.get("target")
    if not isinstance(target, dict):
        raise RepresentativeCorpusError("state-data manifest target is invalid")
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


def extract_member(
    *,
    world: Path,
    plan_path: Path,
    seed_index: int,
    qualified_states: Path,
    state_manifest_path: Path,
    generated_rust_path: Path,
    output: Path,
    evidence_output: Path,
) -> section_corpus.ParsedCorpus:
    plan = section_representative_plan.load_plan(plan_path)
    seeds = plan["seeds"]
    assert isinstance(seeds, list)
    if seed_index < 0 or seed_index >= len(seeds):
        raise RepresentativeCorpusError(
            f"seed-index must be in 0..{len(seeds) - 1}; got {seed_index}"
        )
    seed = int(seeds[seed_index])

    base.validate_level_dat(world)
    state_ids, state_manifest = base.load_state_identity_map(
        qualified_states, state_manifest_path
    )
    selection = selected_chunks(plan)
    regions = selected_region_paths(world, selection)
    inventory_sha256, inventory_entries = source_inventory(world, regions)
    sections = extract_selected_sections(regions, selection, state_ids)
    lattice = validate_selected_sections(sections, selection)

    expected_sections = sum(
        len(selection[descriptor.key]) * len(lattice[descriptor.key])
        for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS
    )
    if len(sections) != expected_sections:
        raise RepresentativeCorpusError(
            f"representative section count mismatch: expected {expected_sections}, got {len(sections)}"
        )

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        render_corpus(sections, state_manifest, inventory_sha256),
        encoding="utf-8",
        newline="\n",
    )
    target = section_corpus.load_target_evidence(state_manifest_path, generated_rust_path)
    parsed = section_corpus.validate_corpus(output, target)

    evidence = {
        "schema": 1,
        "policy": EXTRACTOR_ID,
        "representative_policy": plan["policy"],
        "plan_sha256": plan["plan_sha256"],
        "seed_index": seed_index,
        "seed": seed,
        "world": str(world),
        "inventory_sha256": inventory_sha256,
        "files": inventory_entries,
        "selected_chunks": {
            descriptor.key: [[x, z] for x, z in sorted(selection[descriptor.key])]
            for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS
        },
        "section_lattice": lattice,
        "corpus_sha256": parsed.corpus_sha256,
        "section_count": parsed.section_count,
        "distinct_state_ids": parsed.distinct_state_ids,
    }
    evidence_output.parent.mkdir(parents=True, exist_ok=True)
    evidence_output.write_text(
        json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return parsed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--world", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--seed-index", type=int, required=True)
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
        default=Path("crates/data/crucible-generated/src/lib.rs"),
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()

    try:
        parsed = extract_member(
            world=args.world,
            plan_path=args.plan,
            seed_index=args.seed_index,
            qualified_states=args.qualified_states,
            state_manifest_path=args.state_manifest,
            generated_rust_path=args.generated_rust,
            output=args.output,
            evidence_output=args.evidence,
        )
    except (
        RepresentativeCorpusError,
        section_representative_plan.PlanError,
        base.ExtractorError,
        section_corpus.CorpusError,
        OSError,
        ValueError,
        json.JSONDecodeError,
    ) as error:
        print(f"representative section corpus error: {error}", file=sys.stderr)
        return 1

    print(
        "representative section corpus: "
        f"seed_index={args.seed_index} sections={parsed.section_count} "
        f"states={parsed.distinct_state_ids} corpus_sha256={parsed.corpus_sha256} PASS"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
