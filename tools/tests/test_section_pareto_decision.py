from __future__ import annotations

import copy
import json
import tempfile
import unittest
from pathlib import Path

from tools import section_pareto_decision as pareto
from tools import section_target_combined as combined
from tools import section_target_hardware as population
from tools import section_target_synthetic_evidence as synthetic

GIT_SHA = "c" * 40
GENERATION_SHA = "a" * 64
INPUT_SHA = "b" * 64
BINARY_SHA_PLACEHOLDER = "d" * 64
POPULATION_SHA = "e" * 64
ADMISSION_SHA = "f" * 64
PACK_SHA = "1" * 64


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def target() -> dict[str, object]:
    return {
        "minecraft_version": "26.2",
        "protocol_version": 776,
        "data_version": 4903,
        "state_count": 32_366,
        "state_data_generation_sha256": GENERATION_SHA,
        "state_data_input_sha256": INPUT_SHA,
    }


def aggregate(value: int, count: int = 5) -> dict[str, int]:
    return {
        "count": count,
        "median": value,
        "mad": 0,
        "relative_mad_ppm": 0,
        "min": value,
        "max": value,
    }


def candidate_value(candidate: str) -> int:
    return {
        "direct-reference": 105,
        "direct": 100,
        "adaptive": 90,
        "fast-local": 100,
        "packed-local": 110,
    }[candidate]


def candidate_memory(candidate: str) -> int:
    return {
        "direct-reference": 1_050,
        "direct": 1_000,
        "adaptive": 1_000,
        "fast-local": 800,
        "packed-local": 700,
    }[candidate]


def replacement_map(candidate: str) -> dict[str, int]:
    result: dict[str, int] = {}
    representation = {
        "direct-reference": "direct-reference",
        "direct": "direct-n",
        "adaptive": "local-8",
        "fast-local": "local-8",
        "packed-local": "packed-8",
    }[candidate]
    for pattern, pool in synthetic.QUALIFICATION_CASES:
        for workload in synthetic.REPLACEMENTS:
            key = f"{workload}|{pattern}|{pool}|{pool}|{representation}"
            result[key] = 100
    return result


def promotion_map(candidate: str) -> dict[str, int]:
    representation = {
        "direct-reference": "direct-reference->direct-reference",
        "direct": "direct-n->direct-n",
        "adaptive": "local->local",
        "fast-local": "local->local",
        "packed-local": "packed->packed",
    }[candidate]
    return {
        f"promotion-to-{target_cardinality}|{representation}": 100
        for target_cardinality in synthetic.PROMOTION_TARGETS
    }


class QualificationFixture:
    def __init__(self, base: Path) -> None:
        self.repo = base / "repo"
        self.root = base / "combined"
        self.correctness_dir = base / "correctness"
        self.repo.mkdir()
        self.root.mkdir()
        write_json(
            self.repo / "vanilla/state-data/26.2-state-data-manifest.json",
            {
                "target": {
                    "minecraft_version": "26.2",
                    "protocol_version": 776,
                    "data_version": 4903,
                },
                "state_count": 32_366,
                "generation_digest": GENERATION_SHA,
                "input_digest": INPUT_SHA,
            },
        )
        self.correctness_paths = self._write_correctness()
        self._write_combined_artifact()

    def _write_correctness(self) -> list[Path]:
        paths: list[Path] = []
        for candidate in population.PRODUCTION_CANDIDATES:
            record = {
                "schema": 1,
                "qualification": "section",
                "mode": "full",
                "minecraft_version": "26.2",
                "protocol_version": 776,
                "data_version": 4903,
                "commit_sha": GIT_SHA,
                "state_count": 32_366,
                "state_data_input_sha256": INPUT_SHA,
                "state_data_generation_sha256": GENERATION_SHA,
                "trace_schema": 1,
                "sem_ids": list(pareto.FULL_SEM_IDS),
                "records": [
                    {
                        "id": f"EQUIV-WORLD-SECTION-FULL-{candidate.upper().replace('-', '_')}",
                        "candidate": candidate,
                        "trace_count": pareto.FULL_TRACE_COUNT,
                        "trace_operations": pareto.FULL_TRACE_OPERATIONS,
                        "synthetic_operations": pareto.FULL_SYNTHETIC_OPERATIONS,
                        "trace_fingerprint_fnv1a64": pareto.FULL_TRACE_FINGERPRINT,
                    }
                ],
            }
            path = self.correctness_dir / f"{candidate}.json"
            write_json(path, record)
            paths.append(path)
        return paths

    def _population_child_file(
        self, round_index: int, dimension: str, candidate: str
    ) -> tuple[dict[str, object], Path]:
        stem = f"{dimension.replace(':', '_')}-{candidate}-p0"
        relative = Path("children") / f"round-{round_index:02d}" / f"{stem}.json"
        path = self.root / "population" / relative
        memory = candidate_memory(candidate)
        child = {
            "candidate": candidate,
            "dimension": dimension,
            "commit_sha": GIT_SHA,
            **target(),
            "section_count": 10,
            "memory": {
                "logical_owned_bytes": memory,
                "max_owned_bytes": memory // 10,
                "construction_transitions": 20,
                "logical_backing_allocations": 30,
            },
            "representations": {"uniform": 10},
        }
        write_json(path, child)
        value = candidate_value(candidate)
        summary: dict[str, object] = {
            "round": round_index,
            "dimension": dimension,
            "candidate": candidate,
            "child_evidence_path": relative.as_posix(),
            "child_evidence_sha256": pareto.sha256_file(path),
            "rss_loaded_delta_kib": memory,
            "construction_p99_ns": value,
            "timing_p50_ps_per_op": {
                workload: value for workload in population.WORKLOADS
            },
        }
        return summary, path

    def _write_combined_artifact(self) -> None:
        population_dir = self.root / "population"
        population_dir.mkdir()
        executable = population_dir / "benchmark-executable"
        executable.write_bytes(b"qualification-binary")
        binary_sha = pareto.sha256_file(executable)

        pop_children: list[dict[str, object]] = []
        for round_index in range(5):
            for dimension in population.DIMENSIONS:
                for candidate in population.CANDIDATES:
                    child, _ = self._population_child_file(
                        round_index, dimension, candidate
                    )
                    pop_children.append(child)
        pop_aggregates = population.aggregate_children(pop_children)
        pop_noise = population.classify_noise(
            pop_aggregates, smoke=False, rounds=5
        )
        self.assert_true(pop_noise["population_evidence_eligible"])
        population_record: dict[str, object] = {
            "schema": population.SCHEMA,
            "kind": population.KIND,
            "mode": "qualification",
            "qualification_complete": True,
            "population_evidence_eligible": True,
            "decision_evidence_eligible": False,
            "decision_blockers": [],
            "decision_scope": pareto.DECISION_SCOPE,
            "cross_dimension_score_allowed": False,
            "rounds": 5,
            "cpu": 0,
            "cpu_topology": {"cpu": 0},
            "candidates": list(population.CANDIDATES),
            "production_candidates": list(population.PRODUCTION_CANDIDATES),
            "dimensions": list(population.DIMENSIONS),
            "identities": {
                "repository_commit_sha": GIT_SHA,
                "benchmark_executable_sha256": binary_sha,
                "pack_manifest_sha256": PACK_SHA,
                "representative_policy": population.REPRESENTATIVE_POLICY,
                "population_sha256": POPULATION_SHA,
                "admission_sha256": ADMISSION_SHA,
                "source_artifact_manifest_sha256": "2" * 64,
            },
            "children": pop_children,
            "aggregates": pop_aggregates,
            "noise_qualification": pop_noise,
        }
        population_record["evidence_sha256"] = pareto.canonical_digest(
            population_record
        )
        write_json(population_dir / "orchestration.json", population_record)

        population_manifest_files: list[dict[str, object]] = []
        for path in sorted(population_dir.rglob("*")):
            if not path.is_file() or path.name == "artifact-manifest.json":
                continue
            population_manifest_files.append(
                {
                    "path": path.relative_to(population_dir).as_posix(),
                    "size": path.stat().st_size,
                    "sha256": pareto.sha256_file(path),
                }
            )
        population_manifest: dict[str, object] = {
            "schema": population.ARTIFACT_SCHEMA,
            "kind": population.ARTIFACT_KIND,
            "orchestration_sha256": population_record["evidence_sha256"],
            "files": population_manifest_files,
        }
        population_manifest["manifest_sha256"] = pareto.canonical_digest(
            population_manifest
        )
        write_json(population_dir / "artifact-manifest.json", population_manifest)

        synth_children: list[dict[str, object]] = []
        for round_index in range(5):
            for candidate in synthetic.CANDIDATES:
                synth_children.append(
                    {
                        "round": round_index,
                        "candidate": candidate,
                        "control_p50_ps_per_op": 100,
                        "replacement_p50_ps_per_op": replacement_map(candidate),
                        "promotion_p99_ns": promotion_map(candidate),
                    }
                )
        synth_aggregates = synthetic.aggregate_children(synth_children)
        synth_noise = synthetic.classify_noise(
            synth_aggregates, smoke=False, rounds=5
        )
        self.assert_true(synth_noise["synthetic_evidence_eligible"])

        combined_record: dict[str, object] = {
            "schema": combined.SCHEMA,
            "kind": combined.KIND,
            "mode": "qualification",
            "qualification_complete": True,
            "population_evidence_eligible": True,
            "synthetic_evidence_eligible": True,
            "combined_measurement_evidence_eligible": True,
            "decision_evidence_eligible": False,
            "decision_blockers": [
                "dimension-separated Pareto selection record not assembled"
            ],
            "decision_scope": pareto.DECISION_SCOPE,
            "cross_dimension_score_allowed": False,
            "rounds": 5,
            "cpu": 0,
            "cpu_topology": {"cpu": 0},
            "candidates": list(population.CANDIDATES),
            "production_candidates": list(population.PRODUCTION_CANDIDATES),
            "dimensions": list(population.DIMENSIONS),
            "identities": {
                "repository_commit_sha": GIT_SHA,
                "benchmark_executable_sha256": binary_sha,
                "pack_manifest_sha256": PACK_SHA,
                "representative_policy": population.REPRESENTATIVE_POLICY,
                "population_sha256": POPULATION_SHA,
                "population_admission_sha256": ADMISSION_SHA,
                "population_orchestration_evidence_sha256": population_record[
                    "evidence_sha256"
                ],
                "population_artifact_manifest_sha256": population_manifest[
                    "manifest_sha256"
                ],
            },
            "population": {
                "evidence_path": "population/orchestration.json",
                "artifact_manifest_path": "population/artifact-manifest.json",
                "aggregates": pop_aggregates,
                "noise_qualification": pop_noise,
            },
            "synthetic": {
                "schedule": [],
                "children": synth_children,
                "aggregates": synth_aggregates,
                "noise_qualification": synth_noise,
            },
        }
        combined_record["evidence_sha256"] = pareto.canonical_digest(
            combined_record
        )
        write_json(self.root / "combined-orchestration.json", combined_record)
        self.reseal_root_manifest()

    def reseal_root_manifest(self) -> None:
        combined_record = json.loads(
            (self.root / "combined-orchestration.json").read_text(encoding="utf-8")
        )
        files: list[dict[str, object]] = []
        for path in sorted(self.root.rglob("*")):
            if not path.is_file() or path == self.root / "artifact-manifest.json":
                continue
            files.append(
                {
                    "path": path.relative_to(self.root).as_posix(),
                    "size": path.stat().st_size,
                    "sha256": pareto.sha256_file(path),
                }
            )
        manifest: dict[str, object] = {
            "schema": combined.ARTIFACT_SCHEMA,
            "kind": combined.ARTIFACT_KIND,
            "combined_evidence_sha256": combined_record["evidence_sha256"],
            "files": files,
        }
        manifest["manifest_sha256"] = pareto.canonical_digest(manifest)
        write_json(self.root / "artifact-manifest.json", manifest)

    @staticmethod
    def assert_true(value: object) -> None:
        if value is not True:
            raise AssertionError(f"fixture expected eligibility, got {value!r}")


class ParetoDecisionTests(unittest.TestCase):
    def test_full_qualification_fixture_is_accepted_and_content_addressed(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = QualificationFixture(Path(raw))
            result = pareto.analyze(
                repo_root=fixture.repo,
                combined_artifact=fixture.root,
                correctness_paths=fixture.correctness_paths,
            )
            self.assertTrue(result["analysis_complete"])
            self.assertFalse(result["decision_evidence_eligible"])
            self.assertEqual(result["decision_scope"], pareto.DECISION_SCOPE)
            self.assertFalse(result["cross_dimension_score_allowed"])
            self.assertEqual(
                result["analysis_sha256"],
                pareto.canonical_digest(
                    {key: value for key, value in result.items() if key != "analysis_sha256"}
                ),
            )
            global_record = result["global"]
            self.assertNotIn("direct-reference", global_record["pareto_survivors"])
            self.assertTrue(global_record["common_all_dimension_frontier"])

    def test_smoke_or_ineligible_combined_evidence_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = QualificationFixture(Path(raw))
            path = fixture.root / "combined-orchestration.json"
            record = json.loads(path.read_text(encoding="utf-8"))
            record["mode"] = "smoke"
            record["combined_measurement_evidence_eligible"] = False
            record["evidence_sha256"] = pareto.canonical_digest(
                {key: value for key, value in record.items() if key != "evidence_sha256"}
            )
            write_json(path, record)
            fixture.reseal_root_manifest()
            with self.assertRaises(pareto.ParetoEvidenceError):
                pareto.analyze(
                    repo_root=fixture.repo,
                    combined_artifact=fixture.root,
                    correctness_paths=fixture.correctness_paths,
                )

    def test_correctness_commit_and_trace_identity_are_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = QualificationFixture(Path(raw))
            path = fixture.correctness_paths[0]
            record = json.loads(path.read_text(encoding="utf-8"))
            record["commit_sha"] = "d" * 40
            write_json(path, record)
            with self.assertRaises(pareto.ParetoEvidenceError):
                pareto.analyze(
                    repo_root=fixture.repo,
                    combined_artifact=fixture.root,
                    correctness_paths=fixture.correctness_paths,
                )

            record["commit_sha"] = GIT_SHA
            record["records"][0]["trace_fingerprint_fnv1a64"] = "0" * 16
            write_json(path, record)
            with self.assertRaises(pareto.ParetoEvidenceError):
                pareto.analyze(
                    repo_root=fixture.repo,
                    combined_artifact=fixture.root,
                    correctness_paths=fixture.correctness_paths,
                )

    def test_artifact_corruption_is_rejected_even_if_json_still_parses(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = QualificationFixture(Path(raw))
            executable = fixture.root / "population/benchmark-executable"
            executable.write_bytes(b"changed")
            with self.assertRaises(pareto.ParetoEvidenceError):
                pareto.analyze(
                    repo_root=fixture.repo,
                    combined_artifact=fixture.root,
                    correctness_paths=fixture.correctness_paths,
                )

    def test_git_commit_and_sha256_identities_are_not_interchangeable(self) -> None:
        self.assertEqual(pareto.git_sha(GIT_SHA, "git"), GIT_SHA)
        self.assertEqual(pareto.sha256(GENERATION_SHA, "sha"), GENERATION_SHA)
        with self.assertRaises(pareto.ParetoEvidenceError):
            pareto.sha256(GIT_SHA, "wrong")
        with self.assertRaises(pareto.ParetoEvidenceError):
            pareto.git_sha(GENERATION_SHA, "wrong")

    def test_equal_vectors_are_not_strict_dominance(self) -> None:
        left = {"a": 10, "b": 20}
        self.assertFalse(pareto.strictly_dominates(left, dict(left)))
        self.assertTrue(pareto.strictly_dominates({"a": 9, "b": 20}, left))
        self.assertFalse(pareto.strictly_dominates({"a": 9, "b": 21}, left))

    def test_reference_only_candidate_cannot_eliminate_production_candidate(self) -> None:
        vectors = {
            candidate: {"metric": 100}
            for candidate in population.CANDIDATES
        }
        vectors["direct-reference"]["metric"] = 1
        frontier, dominators = pareto.dimension_frontier(vectors)
        self.assertEqual(set(frontier), set(population.PRODUCTION_CANDIDATES))
        self.assertTrue(all("direct-reference" not in value for value in dominators.values()))

    def test_one_dimension_domination_does_not_become_global_rejection(self) -> None:
        vectors: dict[str, dict[str, dict[str, int]]] = {}
        for dimension in population.DIMENSIONS:
            vectors[dimension] = {
                candidate: {"metric": 100}
                for candidate in population.CANDIDATES
            }
        vectors["minecraft:overworld"]["adaptive"]["metric"] = 90
        vectors["minecraft:overworld"]["fast-local"]["metric"] = 100
        vectors["minecraft:the_nether"]["adaptive"]["metric"] = 110
        vectors["minecraft:the_nether"]["fast-local"]["metric"] = 100
        dominators = pareto.all_dimension_dominators(vectors)
        self.assertNotIn("adaptive", dominators["fast-local"])
        self.assertNotIn("fast-local", dominators["adaptive"])

    def test_all_dimension_domination_requires_same_production_dominator(self) -> None:
        vectors: dict[str, dict[str, dict[str, int]]] = {}
        for dimension in population.DIMENSIONS:
            vectors[dimension] = {
                candidate: {"metric": 100}
                for candidate in population.CANDIDATES
            }
            vectors[dimension]["adaptive"]["metric"] = 90
            vectors[dimension]["fast-local"]["metric"] = 100
        dominators = pareto.all_dimension_dominators(vectors)
        self.assertIn("adaptive", dominators["fast-local"])

    def test_materiality_boundaries_use_exact_integer_ppm(self) -> None:
        self.assertEqual(pareto.improvement_ppm(100, 95), 50_000)
        self.assertEqual(pareto.improvement_ppm(100, 90), 100_000)
        self.assertEqual(pareto.improvement_ppm(100, 96), 40_000)

    def test_deterministic_memory_drift_across_rounds_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = QualificationFixture(Path(raw))
            pop_path = fixture.root / "population/orchestration.json"
            population_record = json.loads(pop_path.read_text(encoding="utf-8"))
            first = population_record["children"][0]
            raw_child_path = fixture.root / "population" / first["child_evidence_path"]
            raw_child = json.loads(raw_child_path.read_text(encoding="utf-8"))
            raw_child["memory"]["logical_owned_bytes"] += 1
            write_json(raw_child_path, raw_child)
            first["child_evidence_sha256"] = pareto.sha256_file(raw_child_path)
            population_record["evidence_sha256"] = pareto.canonical_digest(
                {
                    key: value
                    for key, value in population_record.items()
                    if key != "evidence_sha256"
                }
            )
            write_json(pop_path, population_record)
            # Call the deterministic verifier directly: the drift must fail even when the
            # child SHA in the orchestration has been deliberately resealed.
            with self.assertRaises(pareto.ParetoEvidenceError):
                pareto.population_deterministic_diagnostics(
                    fixture.root, population_record, target()
                )

    def test_normalized_synthetic_surface_ignores_representation_names(self) -> None:
        children: list[dict[str, object]] = []
        for candidate in synthetic.CANDIDATES:
            children.append(
                {
                    "candidate": candidate,
                    "control_p50_ps_per_op": 100,
                    "replacement_p50_ps_per_op": replacement_map(candidate),
                    "promotion_p99_ns": promotion_map(candidate),
                }
            )
        aggregates = synthetic.aggregate_children(children)
        values, replacements, promotions = pareto.synthetic_medians(aggregates)
        self.assertEqual(len(replacements), len(synthetic.QUALIFICATION_CASES) * len(synthetic.REPLACEMENTS))
        self.assertEqual(len(promotions), len(synthetic.PROMOTION_TARGETS))
        self.assertEqual(set(values["direct"]), set(values["packed-local"]))


if __name__ == "__main__":
    unittest.main()
