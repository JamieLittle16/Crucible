from __future__ import annotations

import copy
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_resident_world_evidence as evidence


def hardware(cpu: str = "3") -> dict[str, str]:
    return {
        "commit_sha": "a" * 40,
        "rustc_verbose": "rustc 1.97.1\nhost: x86_64-unknown-linux-gnu",
        "target_triple": "x86_64-unknown-linux-gnu",
        "cpu_model": "Test CPU",
        "cpu_vendor": "GenuineIntel",
        "cpu_family": "6",
        "cpu_model_id": "1",
        "cpu_stepping": "2",
        "cpu_microcode": "0x1",
        "kernel": "Linux test",
        "cpu_governor": "performance",
        "cpu_current_khz": "4000000",
        "cpu_min_khz": "4000000",
        "cpu_max_khz": "4000000",
        "cpus_allowed_list": cpu,
        "mems_allowed_list": "0",
        "online_cpus": "0-7",
        "smt_active": "1",
        "cache_topology": "L1:Data:size=32K",
        "perf_event_paranoid": "2",
        "transparent_hugepage": "always [madvise] never",
        "memory_total_kib": "32768000 kB",
        "load_average": "0.01 0.02 0.03 1/100 123",
        "no_turbo": "0",
        "rustflags": "",
        "cargo_encoded_rustflags": "",
    }


def target_qualification(cpu: str = "3") -> dict[str, object]:
    return {
        "schema": 1,
        "runner": evidence.TARGET_RUNNER_ID,
        "explicit_operator_action": True,
        "single_cpu_requirement_enforced": True,
        "requested_cpu": cpu,
        "hosted_ci_eligible": False,
    }


def summary(p50: int, p99: int) -> dict[str, int]:
    return {"p50": p50, "p95": max(p50, p99 - 1), "p99": p99, "max": p99 + 5}


def case(name: str, side: int, seed: int) -> dict[str, object]:
    chunks = side * side
    return {
        "name": name,
        "side": side,
        "chunk_count": chunks,
        "semantic_equivalent": True,
        "stale_rejections_per_lifecycle": chunks,
        "lifecycle_checksum": 1000 + seed,
        "hot_checksum": 2000 + seed,
        "lifecycle_summary_ns": summary(100 + seed, 140 + seed),
        "repeated_resolve_summary_ns": summary(600 + seed, 720 + seed),
        "resolve_once_summary_ns": summary(100 + seed, 130 + seed),
        "lifecycle_samples_ns": [{"round": 0, "elapsed_ns": 100 + seed}],
        "hot_pairs": [
            {
                "round": 0,
                "repeated_first": True,
                "repeated_resolve_ns": 600 + seed,
                "resolve_once_ns": 100 + seed,
            }
        ],
    }


def artifact(run_shift: int = 0) -> dict[str, object]:
    data = {
        "schema": 1,
        "benchmark": "r2c-resident-world-lifecycle",
        "mode": "full",
        "hosted_ci_is_diagnostic_only": True,
        "timing_threshold_selected": False,
        "production_path_unchanged": True,
        "warmup_rounds": 8,
        "measured_rounds": 64,
        "hot_reads": 262_144,
        "structural": {
            "resident_handle_bytes": 24,
            "dimension_profile_bytes": 24,
            "profile_section_count": 24,
            "repeated_resolutions_per_hot_sample": 262_144,
            "resolve_once_resolutions_per_hot_sample": 1,
        },
        "hardware": hardware(),
        "target_qualification": target_qualification(),
        "cases": [
            case("resident-1x1-positive", 1, 1),
            case("resident-3x3-signed", 3, 2),
            case("resident-5x5-mixed", 5, 3),
            case("resident-9x9-negative", 9, 4),
        ],
    }
    for row in data["cases"]:
        row["lifecycle_summary_ns"] = summary(
            row["lifecycle_summary_ns"]["p50"] + run_shift,
            row["lifecycle_summary_ns"]["p99"] + run_shift,
        )
        row["repeated_resolve_summary_ns"] = summary(
            row["repeated_resolve_summary_ns"]["p50"] + run_shift,
            row["repeated_resolve_summary_ns"]["p99"] + run_shift,
        )
        row["resolve_once_summary_ns"] = summary(
            row["resolve_once_summary_ns"]["p50"] + run_shift,
            row["resolve_once_summary_ns"]["p99"] + run_shift,
        )
    return data


def write_artifacts(root: Path, values: list[dict[str, object]]) -> list[Path]:
    paths = []
    for index, value in enumerate(values, start=1):
        path = root / f"run-{index}.json"
        path.write_text(json.dumps(value), encoding="utf-8")
        paths.append(path)
    return paths


class R2CResidentWorldEvidenceTests(unittest.TestCase):
    def test_three_consistent_runs_produce_source_free_cross_process_summary(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            paths = write_artifacts(
                Path(temporary),
                [artifact(0), artifact(5), artifact(10)],
            )
            report = evidence.combine(paths)

        self.assertTrue(report["mechanically_consistent"])
        self.assertFalse(report["performance_admitted"])
        self.assertTrue(report["human_baseline_review_required"])
        self.assertFalse(report["timing_threshold_selected"])
        self.assertEqual(report["target_qualification"]["runner"], evidence.TARGET_RUNNER_ID)
        self.assertEqual(report["run_count"], 3)
        self.assertEqual(len(report["cases"]), 4)
        first = report["cases"][0]["cross_process"]
        self.assertEqual(first["run_count"], 3)
        self.assertEqual(first["resolve_once_faster_runs"], 3)
        self.assertGreater(first["hot_ratio_median_ppm"], 1_000_000)
        self.assertGreaterEqual(first["lifecycle_p50_relative_mad_ppm"], 0)

    def test_fewer_than_three_runs_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            paths = write_artifacts(Path(temporary), [artifact(), artifact(1)])
            with self.assertRaisesRegex(evidence.EvidenceError, "at least 3"):
                evidence.combine(paths)

    def test_ordinary_full_artifact_without_target_runner_witness_is_rejected(self) -> None:
        changed = artifact()
        del changed["target_qualification"]
        with tempfile.TemporaryDirectory() as temporary:
            path = write_artifacts(Path(temporary), [changed])[0]
            with self.assertRaisesRegex(evidence.EvidenceError, "target_qualification missing"):
                evidence.load_artifact(path)

    def test_mismatched_commit_or_machine_is_rejected(self) -> None:
        changed = artifact(10)
        changed["hardware"]["commit_sha"] = "b" * 40
        with tempfile.TemporaryDirectory() as temporary:
            paths = write_artifacts(
                Path(temporary),
                [artifact(), artifact(5), changed],
            )
            with self.assertRaisesRegex(evidence.EvidenceError, "hardware/toolchain"):
                evidence.combine(paths)

    def test_dynamic_frequency_and_load_are_allowed_to_vary(self) -> None:
        changed = artifact(10)
        changed["hardware"]["cpu_current_khz"] = "3900000"
        changed["hardware"]["load_average"] = "0.50 0.20 0.10 2/100 999"
        with tempfile.TemporaryDirectory() as temporary:
            paths = write_artifacts(
                Path(temporary),
                [artifact(), artifact(5), changed],
            )
            report = evidence.combine(paths)
        self.assertTrue(report["mechanically_consistent"])

    def test_unpinned_cpu_affinity_is_rejected(self) -> None:
        changed = artifact(10)
        changed["hardware"] = hardware("2-3")
        changed["target_qualification"] = target_qualification("2-3")
        with tempfile.TemporaryDirectory() as temporary:
            path = write_artifacts(Path(temporary), [changed])[0]
            with self.assertRaisesRegex(evidence.EvidenceError, "requested_cpu must be one logical CPU"):
                evidence.load_artifact(path)

    def test_target_requested_cpu_must_match_observed_affinity(self) -> None:
        changed = artifact(10)
        changed["target_qualification"] = target_qualification("4")
        with tempfile.TemporaryDirectory() as temporary:
            path = write_artifacts(Path(temporary), [changed])[0]
            with self.assertRaisesRegex(evidence.EvidenceError, "requested CPU 4"):
                evidence.load_artifact(path)

    def test_target_witness_mismatch_is_rejected_across_runs(self) -> None:
        changed = artifact(10)
        changed["hardware"] = hardware("4")
        changed["target_qualification"] = target_qualification("4")
        with tempfile.TemporaryDirectory() as temporary:
            paths = write_artifacts(
                Path(temporary),
                [artifact(), artifact(5), changed],
            )
            with self.assertRaisesRegex(evidence.EvidenceError, "hardware/toolchain|target-qualification"):
                evidence.combine(paths)

    def test_semantic_witness_mismatch_is_rejected_even_when_timing_looks_valid(self) -> None:
        changed = artifact(10)
        changed["cases"][2]["hot_checksum"] += 1
        with tempfile.TemporaryDirectory() as temporary:
            paths = write_artifacts(
                Path(temporary),
                [artifact(), artifact(5), changed],
            )
            with self.assertRaisesRegex(evidence.EvidenceError, "semantic witness"):
                evidence.combine(paths)

    def test_workload_shape_mismatch_is_rejected(self) -> None:
        changed = artifact(10)
        changed["hot_reads"] = 131_072
        changed["structural"]["repeated_resolutions_per_hot_sample"] = 131_072
        with tempfile.TemporaryDirectory() as temporary:
            paths = write_artifacts(
                Path(temporary),
                [artifact(), artifact(5), changed],
            )
            with self.assertRaisesRegex(evidence.EvidenceError, "workload or semantic witness"):
                evidence.combine(paths)

    def test_non_monotone_percentiles_fail_closed(self) -> None:
        changed = artifact(10)
        changed["cases"][0]["lifecycle_summary_ns"] = {
            "p50": 100,
            "p95": 90,
            "p99": 120,
            "max": 130,
        }
        with tempfile.TemporaryDirectory() as temporary:
            path = write_artifacts(Path(temporary), [changed])[0]
            with self.assertRaisesRegex(evidence.EvidenceError, "non-monotone"):
                evidence.load_artifact(path)

    def test_duplicate_paths_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = write_artifacts(Path(temporary), [artifact()])[0]
            with self.assertRaisesRegex(evidence.EvidenceError, "unique"):
                evidence.combine([path, path, path])

    def test_input_artifacts_are_not_mutated_by_combination(self) -> None:
        values = [artifact(), artifact(5), artifact(10)]
        before = copy.deepcopy(values)
        with tempfile.TemporaryDirectory() as temporary:
            paths = write_artifacts(Path(temporary), values)
            evidence.combine(paths)
        self.assertEqual(values, before)


if __name__ == "__main__":
    unittest.main()
