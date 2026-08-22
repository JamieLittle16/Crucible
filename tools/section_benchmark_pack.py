#!/usr/bin/env python3
"""Build content-addressed target-hardware section benchmark packs."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Any, BinaryIO, Iterator

import section_corpus

PACK_SCHEMA = 1
PACK_MAGIC = b"CRUCIBLE-SECTION-BENCH-PACK|1\n"
PACK_KIND = "section-target-benchmark-pack-set"
ARTIFACT_KIND = "section-representative-set-workflow-artifact"
ADMISSION_KIND = "section-representative-set-admission"
REPRESENTATIVE_EXTRACTOR = "vanilla-save-region-v2-representative-member"
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
RESOURCE_LOCATION = re.compile(r"[a-z0-9_.-]+:[a-z0-9_./-]+\Z")
SECTION_CELLS = 4096


class PackError(ValueError):
    """Raised when an admitted population cannot safely become a benchmark pack."""


@dataclass(frozen=True)
class Member:
    seed_index: int
    corpus_path: Path
    corpus_sha256: str


@dataclass(frozen=True)
class Population:
    population_sha256: str
    admission_sha256: str
    artifact_manifest_sha256: str
    policy: str
    dimensions: dict[str, int]
    members: tuple[Member, ...]


def canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise PackError(f"{label} must be an object")
    return value


def _load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    return _object(value, str(path))


def _sha(value: object, label: str) -> str:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise PackError(f"{label} must be canonical lowercase SHA-256")
    return value


def _integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise PackError(f"{label} must be an integer")
    return value


def _safe_relative_path(raw: object) -> Path:
    if not isinstance(raw, str) or not raw:
        raise PackError("artifact file path must be a non-empty string")
    path = Path(raw)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise PackError(f"unsafe artifact file path: {raw!r}")
    return path


def _verify_digest_field(record: dict[str, Any], field: str, label: str) -> str:
    expected = _sha(record.get(field), f"{label}.{field}")
    payload = dict(record)
    payload.pop(field)
    actual = canonical_digest(payload)
    if actual != expected:
        raise PackError(f"{label} digest mismatch: expected {expected}, got {actual}")
    return expected


def _verify_artifact_files(root: Path, manifest: dict[str, Any]) -> dict[str, str]:
    entries = manifest.get("files")
    if not isinstance(entries, list) or not entries:
        raise PackError("artifact manifest must list retained evidence files")
    hashes: dict[str, str] = {}
    for index, raw_entry in enumerate(entries):
        entry = _object(raw_entry, f"artifact file[{index}]")
        relative = _safe_relative_path(entry.get("path"))
        key = relative.as_posix()
        if key in hashes:
            raise PackError(f"duplicate artifact file entry: {key}")
        path = root / relative
        if not path.is_file():
            raise PackError(f"artifact file is missing: {key}")
        expected_size = _integer(entry.get("size"), f"artifact file {key}.size")
        if expected_size < 0 or path.stat().st_size != expected_size:
            raise PackError(f"artifact file size mismatch: {key}")
        expected_sha = _sha(entry.get("sha256"), f"artifact file {key}.sha256")
        actual_sha = sha256_file(path)
        if actual_sha != expected_sha:
            raise PackError(
                f"artifact file SHA-256 mismatch for {key}: expected {expected_sha}, got {actual_sha}"
            )
        hashes[key] = actual_sha
    return hashes


def admit_population(root: Path) -> Population:
    artifact_path = root / "artifact-manifest.json"
    admission_path = root / "population-admission.json"
    artifact = _load_json(artifact_path)
    admission = _load_json(admission_path)

    if artifact.get("schema") != 2 or artifact.get("kind") != ARTIFACT_KIND:
        raise PackError("representative artifact manifest schema/kind mismatch")
    if artifact.get("qualification_complete") is not True:
        raise PackError("representative artifact is not qualification-complete")
    if artifact.get("decision_eligible") is not True:
        raise PackError("representative artifact is not decision-eligible")
    if artifact.get("benchmark_handoff_eligible") is not True:
        raise PackError("representative artifact is not benchmark-handoff eligible")
    artifact_manifest_sha = _verify_digest_field(
        artifact, "manifest_sha256", "artifact manifest"
    )
    file_hashes = _verify_artifact_files(root, artifact)

    if admission.get("schema") != 1 or admission.get("kind") != ADMISSION_KIND:
        raise PackError("population admission schema/kind mismatch")
    if admission.get("decision_eligible") is not True:
        raise PackError("population admission is not decision-eligible")
    if admission.get("benchmark_handoff_eligible") is not True:
        raise PackError("population admission does not authorize benchmark handoff")
    if admission.get("decision_scope") != "dimension-separated-only":
        raise PackError("population admission decision scope drifted")
    if admission.get("cross_dimension_score_allowed") is not False:
        raise PackError("population admission allows forbidden cross-dimension scoring")
    admission_sha = _verify_digest_field(
        admission, "admission_sha256", "population admission"
    )
    population_sha = _sha(admission.get("population_sha256"), "population_sha256")

    identities = _object(artifact.get("identities"), "artifact identities")
    if identities.get("population_sha256") != population_sha:
        raise PackError("artifact/admission population identity mismatch")
    if identities.get("admission_sha256") != admission_sha:
        raise PackError("artifact/admission admission identity mismatch")
    if file_hashes.get("population-admission.json") != sha256_file(admission_path):
        raise PackError("population-admission.json is not bound by the artifact manifest")

    raw_dimensions = _object(admission.get("per_dimension"), "admission per_dimension")
    dimensions: dict[str, int] = {}
    for dimension, raw_summary in sorted(raw_dimensions.items()):
        if RESOURCE_LOCATION.fullmatch(dimension) is None:
            raise PackError(f"invalid admitted dimension: {dimension!r}")
        summary = _object(raw_summary, f"admission {dimension}")
        sections = _integer(summary.get("section_count"), f"{dimension}.section_count")
        cells = _integer(summary.get("total_cells"), f"{dimension}.total_cells")
        if sections <= 0 or cells != sections * SECTION_CELLS:
            raise PackError(f"invalid admitted section/cell count for {dimension}")
        dimensions[dimension] = sections
    if not dimensions:
        raise PackError("population admission has no dimensions")

    raw_members = admission.get("members")
    if not isinstance(raw_members, list) or len(raw_members) != 4:
        raise PackError("population admission must contain exactly four members")
    members: list[Member] = []
    seen_seed_indices: set[int] = set()
    seen_corpora: set[str] = set()
    for raw_member in raw_members:
        member = _object(raw_member, "population member")
        seed_index = _integer(member.get("seed_index"), "member.seed_index")
        corpus_sha = _sha(member.get("corpus_sha256"), "member.corpus_sha256")
        if seed_index in seen_seed_indices:
            raise PackError(f"duplicate population seed index: {seed_index}")
        if corpus_sha in seen_corpora:
            raise PackError(f"duplicate population corpus identity: {corpus_sha}")
        seen_seed_indices.add(seed_index)
        seen_corpora.add(corpus_sha)
        relative = Path(f"seed-{seed_index}") / "member.corpus"
        key = relative.as_posix()
        if file_hashes.get(key) != corpus_sha:
            raise PackError(f"artifact/member corpus identity mismatch for seed {seed_index}")
        members.append(Member(seed_index, root / relative, corpus_sha))
    if seen_seed_indices != {0, 1, 2, 3}:
        raise PackError(f"population seed indices must be exactly 0..3; got {seen_seed_indices}")
    members.sort(key=lambda member: member.seed_index)

    policy = admission.get("policy")
    if not isinstance(policy, str) or not policy:
        raise PackError("population admission policy must be a non-empty string")
    return Population(
        population_sha256=population_sha,
        admission_sha256=admission_sha,
        artifact_manifest_sha256=artifact_manifest_sha,
        policy=policy,
        dimensions=dimensions,
        members=tuple(members),
    )


def _canonical_line(handle: BinaryIO, digest: "hashlib._Hash", label: str) -> bytes:
    raw = handle.readline()
    if not raw:
        raise PackError(f"{label}: unexpected end of corpus")
    digest.update(raw)
    if b"\r" in raw or not raw.endswith(b"\n"):
        raise PackError(f"{label}: corpus line endings are not canonical LF")
    return raw[:-1]


def _iter_member_sections(
    member: Member,
    target: section_corpus.TargetEvidence,
) -> Iterator[tuple[str, tuple[int, ...]]]:
    # Independent canonical validation is intentionally run before the streaming conversion.
    parsed = section_corpus.validate_corpus(member.corpus_path, target)
    if parsed.corpus_sha256 != member.corpus_sha256:
        raise PackError(
            f"seed {member.seed_index}: validated corpus SHA does not match admission"
        )
    if parsed.source.extractor != REPRESENTATIVE_EXTRACTOR:
        raise PackError(
            f"seed {member.seed_index}: unexpected representative extractor {parsed.source.extractor}"
        )

    digest = hashlib.sha256()
    with member.corpus_path.open("rb") as handle:
        magic = _canonical_line(handle, digest, f"seed {member.seed_index} magic")
        target_line = _canonical_line(handle, digest, f"seed {member.seed_index} target")
        source_line = _canonical_line(handle, digest, f"seed {member.seed_index} source")
        if magic != section_corpus.MAGIC.encode("ascii"):
            raise PackError(f"seed {member.seed_index}: corpus magic drifted during packing")
        if not target_line.startswith(b"TARGET|") or not source_line.startswith(b"SOURCE|"):
            raise PackError(f"seed {member.seed_index}: corpus headers drifted during packing")

        line_number = 4
        while True:
            raw = handle.readline()
            if not raw:
                break
            digest.update(raw)
            if b"\r" in raw or not raw.endswith(b"\n"):
                raise PackError(
                    f"seed {member.seed_index} line {line_number}: noncanonical line ending"
                )
            try:
                line = raw[:-1].decode("ascii")
            except UnicodeDecodeError as error:
                raise PackError(
                    f"seed {member.seed_index} line {line_number}: non-ASCII corpus record"
                ) from error
            parts = line.split("|", maxsplit=5)
            if len(parts) != 6 or parts[0] != "SECTION":
                raise PackError(
                    f"seed {member.seed_index} line {line_number}: malformed SECTION record"
                )
            dimension = parts[1]
            if RESOURCE_LOCATION.fullmatch(dimension) is None:
                raise PackError(
                    f"seed {member.seed_index} line {line_number}: invalid dimension"
                )
            tokens = parts[5].split(",")
            if len(tokens) != SECTION_CELLS:
                raise PackError(
                    f"seed {member.seed_index} line {line_number}: wrong cell count"
                )
            states: list[int] = []
            for cell, token in enumerate(tokens):
                if not token.isascii() or not token.isdigit() or (
                    token.startswith("0") and token != "0"
                ):
                    raise PackError(
                        f"seed {member.seed_index} line {line_number} cell {cell}: noncanonical state"
                    )
                state = int(token)
                if state < 0 or state >= target.state_count or state > 0xFFFF:
                    raise PackError(
                        f"seed {member.seed_index} line {line_number} cell {cell}: state out of range"
                    )
                states.append(state)
            yield dimension, tuple(states)
            line_number += 1

    actual_sha = digest.hexdigest()
    if actual_sha != member.corpus_sha256:
        raise PackError(
            f"seed {member.seed_index}: corpus changed while packing: expected {member.corpus_sha256}, got {actual_sha}"
        )


def _pack_filename(dimension: str) -> str:
    return dimension.replace(":", "_").replace("/", "_") + ".section-pack"


def _write_pack_header(
    handle: BinaryIO,
    *,
    target: section_corpus.TargetEvidence,
    population: Population,
    dimension: str,
    section_count: int,
) -> None:
    lines = [
        PACK_MAGIC,
        (
            f"TARGET|minecraft={target.minecraft_version}|protocol={target.protocol_version}|"
            f"data={target.data_version}|state_count={target.state_count}|"
            f"generation_sha256={target.generation_sha256}\n"
        ).encode("ascii"),
        (
            f"POPULATION|population_sha256={population.population_sha256}|"
            f"admission_sha256={population.admission_sha256}\n"
        ).encode("ascii"),
        f"DIMENSION|name={dimension}|section_count={section_count}\n".encode("ascii"),
        b"DATA\n",
    ]
    for line in lines:
        handle.write(line)


def build_packs(
    *,
    root: Path,
    output_dir: Path,
    state_manifest: Path,
    generated_rust: Path,
) -> dict[str, object]:
    population = admit_population(root)
    target = section_corpus.load_target_evidence(state_manifest, generated_rust)
    output_dir.mkdir(parents=True, exist_ok=True)

    handles: dict[str, BinaryIO] = {}
    paths: dict[str, Path] = {}
    counts = {dimension: 0 for dimension in population.dimensions}
    member_counts: dict[int, dict[str, int]] = {}
    try:
        for dimension, section_count in population.dimensions.items():
            path = output_dir / _pack_filename(dimension)
            paths[dimension] = path
            handle = path.open("wb")
            handles[dimension] = handle
            _write_pack_header(
                handle,
                target=target,
                population=population,
                dimension=dimension,
                section_count=section_count,
            )

        for member in population.members:
            per_member = {dimension: 0 for dimension in population.dimensions}
            for dimension, states in _iter_member_sections(member, target):
                if dimension not in handles:
                    raise PackError(
                        f"seed {member.seed_index}: corpus contains unadmitted dimension {dimension}"
                    )
                handles[dimension].write(struct.pack(f"<{SECTION_CELLS}H", *states))
                counts[dimension] += 1
                per_member[dimension] += 1
            member_counts[member.seed_index] = per_member
    finally:
        for handle in handles.values():
            handle.close()

    for dimension, expected in population.dimensions.items():
        if counts[dimension] != expected:
            raise PackError(
                f"packed section count mismatch for {dimension}: expected {expected}, got {counts[dimension]}"
            )

    packs: dict[str, object] = {}
    for dimension, path in sorted(paths.items()):
        packs[dimension] = {
            "path": path.name,
            "section_count": counts[dimension],
            "total_cells": counts[dimension] * SECTION_CELLS,
            "size": path.stat().st_size,
            "sha256": sha256_file(path),
        }

    result: dict[str, object] = {
        "schema": PACK_SCHEMA,
        "kind": PACK_KIND,
        "policy": population.policy,
        "population_sha256": population.population_sha256,
        "admission_sha256": population.admission_sha256,
        "source_artifact_manifest_sha256": population.artifact_manifest_sha256,
        "target": {
            "minecraft_version": target.minecraft_version,
            "protocol_version": target.protocol_version,
            "data_version": target.data_version,
            "state_count": target.state_count,
            "state_data_generation_sha256": target.generation_sha256,
            "state_data_input_sha256": target.input_sha256,
        },
        "members": [
            {
                "seed_index": member.seed_index,
                "corpus_sha256": member.corpus_sha256,
                "per_dimension_sections": member_counts[member.seed_index],
            }
            for member in population.members
        ],
        "decision_scope": "dimension-separated-only",
        "cross_dimension_score_allowed": False,
        "packs": packs,
    }
    result["manifest_sha256"] = canonical_digest(result)
    manifest_path = output_dir / "pack-manifest.json"
    manifest_path.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--representative-root", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
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
    args = parser.parse_args()
    try:
        result = build_packs(
            root=args.representative_root,
            output_dir=args.output_dir,
            state_manifest=args.state_manifest,
            generated_rust=args.generated_rust,
        )
    except (OSError, json.JSONDecodeError, PackError, section_corpus.CorpusError) as error:
        print(f"section benchmark pack error: {error}")
        return 1

    print(
        "section benchmark packs: "
        f"dimensions={len(result['packs'])} population={result['population_sha256']} "
        f"manifest={result['manifest_sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
