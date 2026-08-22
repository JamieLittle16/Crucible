from __future__ import annotations

import hashlib
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

import section_benchmark_pack
import section_corpus

DIMENSIONS = (
    "minecraft:overworld",
    "minecraft:the_end",
    "minecraft:the_nether",
)


def _target() -> section_corpus.TargetEvidence:
    return section_corpus.load_target_evidence(
        Path("vanilla/state-data/26.2-state-data-manifest.json"),
        Path("crates/data/crucible-generated/src/lib.rs"),
    )


def _corpus_bytes(seed_index: int) -> bytes:
    target = _target()
    lines = [
        section_corpus.MAGIC,
        (
            f"TARGET|minecraft={target.minecraft_version}|protocol={target.protocol_version}|"
            f"data={target.data_version}|state_count={target.state_count}|"
            f"generation_sha256={target.generation_sha256}"
        ),
        (
            "SOURCE|kind=vanilla-save|inventory_sha256="
            + hashlib.sha256(f"inventory-{seed_index}".encode()).hexdigest()
            + "|extractor=vanilla-save-region-v2-representative-member"
        ),
    ]
    for dimension_index, dimension in enumerate(DIMENSIONS):
        state = seed_index * 3 + dimension_index
        payload = ",".join([str(state)] * 4096)
        lines.append(f"SECTION|{dimension}|{seed_index}|0|0|{payload}")
    return ("\n".join(lines) + "\n").encode("ascii")


def _canonical_digest(value: object) -> str:
    return section_benchmark_pack.canonical_digest(value)


def _file_entry(root: Path, relative: str) -> dict[str, object]:
    path = root / relative
    return {
        "path": relative,
        "size": path.stat().st_size,
        "sha256": section_benchmark_pack.sha256_file(path),
    }


def _write_artifact_manifest(root: Path, admission: dict[str, object]) -> None:
    files = [_file_entry(root, "population-admission.json")]
    for seed_index in range(4):
        files.append(_file_entry(root, f"seed-{seed_index}/member.corpus"))
    artifact: dict[str, object] = {
        "schema": 2,
        "kind": section_benchmark_pack.ARTIFACT_KIND,
        "qualification_complete": True,
        "decision_eligible": True,
        "benchmark_handoff_eligible": True,
        "provenance": {"repository_commit_sha": "f" * 40},
        "identities": {
            "population_sha256": admission["population_sha256"],
            "set_evidence_sha256": admission["set_evidence_sha256"],
            "set_file_sha256": admission["set_file_sha256"],
            "admission_sha256": admission["admission_sha256"],
        },
        "files": files,
    }
    artifact["manifest_sha256"] = _canonical_digest(artifact)
    (root / "artifact-manifest.json").write_text(
        json.dumps(artifact, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def _fixture(root: Path) -> dict[str, object]:
    members = []
    for seed_index in range(4):
        directory = root / f"seed-{seed_index}"
        directory.mkdir(parents=True)
        corpus = directory / "member.corpus"
        corpus.write_bytes(_corpus_bytes(seed_index))
        members.append(
            {
                "seed_index": seed_index,
                "seed": 1000 + seed_index,
                "corpus_sha256": section_benchmark_pack.sha256_file(corpus),
                "server_properties_sha256": hashlib.sha256(
                    f"properties-{seed_index}".encode()
                ).hexdigest(),
                "cell_facts": {
                    "non_air": 0,
                    "counted_fluid": 0,
                    "random_block": 0,
                    "random_fluid": 0,
                },
                "section_classes": {
                    "all_air": 3,
                    "contains_fluid": 0,
                    "random_block_present": 0,
                    "random_fluid_present": 0,
                },
            }
        )

    per_dimension = {
        dimension: {
            "section_count": 4,
            "total_cells": 4 * 4096,
            "cell_facts": {
                "non_air": 0,
                "counted_fluid": 0,
                "random_block": 0,
                "random_fluid": 0,
            },
            "section_classes": {
                "all_air": 4,
                "contains_fluid": 0,
                "random_block_present": 0,
                "random_fluid_present": 0,
            },
        }
        for dimension in DIMENSIONS
    }
    admission: dict[str, object] = {
        "schema": 1,
        "kind": section_benchmark_pack.ADMISSION_KIND,
        "policy": "vanilla-section-representative-v1",
        "plan_sha256": "1" * 64,
        "population_sha256": "2" * 64,
        "set_evidence_sha256": "3" * 64,
        "set_file_sha256": "4" * 64,
        "member_count": 4,
        "decision_eligible": True,
        "benchmark_handoff_eligible": True,
        "decision_scope": "dimension-separated-only",
        "cross_dimension_score_allowed": False,
        "members": members,
        "per_dimension": per_dimension,
    }
    admission["admission_sha256"] = _canonical_digest(admission)
    (root / "population-admission.json").write_text(
        json.dumps(admission, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    _write_artifact_manifest(root, admission)
    return admission


class SectionBenchmarkPackTests(unittest.TestCase):
    def test_builds_dimension_separated_content_addressed_packs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "representative"
            output = Path(directory) / "packs"
            root.mkdir()
            admission = _fixture(root)
            result = section_benchmark_pack.build_packs(
                root=root,
                output_dir=output,
                state_manifest=Path("vanilla/state-data/26.2-state-data-manifest.json"),
                generated_rust=Path("crates/data/crucible-generated/src/lib.rs"),
            )
            self.assertEqual(result["population_sha256"], admission["population_sha256"])
            self.assertEqual(result["admission_sha256"], admission["admission_sha256"])
            self.assertEqual(set(result["packs"]), set(DIMENSIONS))
            self.assertFalse(result["cross_dimension_score_allowed"])
            self.assertEqual(result["decision_scope"], "dimension-separated-only")

            target = _target()
            for dimension_index, dimension in enumerate(DIMENSIONS):
                record = result["packs"][dimension]
                self.assertEqual(record["section_count"], 4)
                pack = output / record["path"]
                raw = pack.read_bytes()
                marker = raw.index(b"DATA\n") + len(b"DATA\n")
                payload = raw[marker:]
                self.assertEqual(len(payload), 4 * 4096 * 2)
                first_states = struct.unpack("<4H", payload[:8])
                self.assertEqual(first_states, (dimension_index,) * 4)
                second_member_offset = 4096 * 2
                second = struct.unpack_from("<H", payload, second_member_offset)[0]
                self.assertEqual(second, 3 + dimension_index)
                self.assertIn(
                    f"state_count={target.state_count}".encode(), raw[:marker]
                )
                self.assertEqual(
                    section_benchmark_pack.sha256_file(pack), record["sha256"]
                )

            expected_manifest = dict(result)
            digest = expected_manifest.pop("manifest_sha256")
            self.assertEqual(_canonical_digest(expected_manifest), digest)

    def test_pack_output_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "representative"
            root.mkdir()
            _fixture(root)
            first = section_benchmark_pack.build_packs(
                root=root,
                output_dir=Path(directory) / "first",
                state_manifest=Path("vanilla/state-data/26.2-state-data-manifest.json"),
                generated_rust=Path("crates/data/crucible-generated/src/lib.rs"),
            )
            second = section_benchmark_pack.build_packs(
                root=root,
                output_dir=Path(directory) / "second",
                state_manifest=Path("vanilla/state-data/26.2-state-data-manifest.json"),
                generated_rust=Path("crates/data/crucible-generated/src/lib.rs"),
            )
            self.assertEqual(first, second)
            for dimension in DIMENSIONS:
                first_path = Path(directory) / "first" / first["packs"][dimension]["path"]
                second_path = Path(directory) / "second" / second["packs"][dimension]["path"]
                self.assertEqual(first_path.read_bytes(), second_path.read_bytes())

    def test_member_corruption_after_artifact_manifest_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _fixture(root)
            with (root / "seed-2/member.corpus").open("ab") as handle:
                handle.write(b"corruption")
            with self.assertRaises(section_benchmark_pack.PackError):
                section_benchmark_pack.admit_population(root)

    def test_weakened_handoff_flag_is_rejected_even_with_fresh_digests(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            admission = _fixture(root)
            admission["benchmark_handoff_eligible"] = False
            admission.pop("admission_sha256")
            admission["admission_sha256"] = _canonical_digest(admission)
            (root / "population-admission.json").write_text(
                json.dumps(admission, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
            _write_artifact_manifest(root, admission)
            with self.assertRaises(section_benchmark_pack.PackError):
                section_benchmark_pack.admit_population(root)

    def test_cross_dimension_scoring_permission_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            admission = _fixture(root)
            admission["cross_dimension_score_allowed"] = True
            admission.pop("admission_sha256")
            admission["admission_sha256"] = _canonical_digest(admission)
            (root / "population-admission.json").write_text(
                json.dumps(admission, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
            _write_artifact_manifest(root, admission)
            with self.assertRaises(section_benchmark_pack.PackError):
                section_benchmark_pack.admit_population(root)

    def test_unsafe_artifact_paths_are_rejected(self) -> None:
        with self.assertRaises(section_benchmark_pack.PackError):
            section_benchmark_pack._safe_relative_path("../escape")
        with self.assertRaises(section_benchmark_pack.PackError):
            section_benchmark_pack._safe_relative_path("/absolute")


if __name__ == "__main__":
    unittest.main()
