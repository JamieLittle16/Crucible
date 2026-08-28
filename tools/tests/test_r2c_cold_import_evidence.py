from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_cold_import_evidence as evidence
from tools import r2c_cold_import_target_run as target_run


def hardware(cpu: str = "3") -> dict[str, str]:
    result = {field: f"fixture-{field}" for field in evidence.STABLE_HARDWARE_FIELDS}
    result["commit_sha"] = "a" * 40
    result["rustflags"] = ""
    result["cargo_encoded_rustflags"] = ""
    result["cpus_allowed_list"] = cpu
    result["cpu_current_khz"] = "dynamic"
    result["load_average"] = "dynamic"
    return result


def artifact(scale: int = 1, cpu: str = "3") -> dict[str, object]:
    rounds = 4
    summary = {
        "p50": 10 * scale,
        "p95": 12 * scale,
        "p99": 14 * scale,
        "p999": 15 * scale,
        "max": 16 * scale,
    }
    return {
        "schema": 1,
        "kind": evidence.BENCHMARK_KIND,
        "mode": "full",
        "reference_section_builder": True,
        "production_section_policy_selected": False,
        "performance_admitted": False,
        "warmup_rounds": 2,
        "measured_rounds": rounds,
        "expected_state_id": 1,
        "hardware": hardware(cpu),
        "summary_ns": {
            "import": dict(summary),
            "install": {key: value // 2 or 1 for key, value in summary.items()},
            "combined": {key: value * 2 for key, value in summary.items()},
        },
        "builder_counters": {
            "uniform_sections": rounds * 2,
            "dense_sections": 0,
            "dense_cell_writes": 0,
        },
        "samples": [
            {
                "import_ns": (10 + index) * scale,
                "install_ns": (5 + index) * scale,
                "combined_ns": (20 + index) * scale,
            }
            for index in range(rounds)
        ],
        "target_qualification": {
            "schema": 1,
            "runner": target_run.RUNNER_ID,
            "explicit_operator_action": True,
            "single_cpu_requirement_enforced": True,
            "requested_cpu": cpu,
            "hosted_ci_eligible": False,
        },
    }


def write_artifact(path: Path, data: dict[str, object]) -> None:
    path.write_text(json.dumps(data), encoding="utf-8")


class R2CColdImportEvidenceTests(unittest.TestCase):
    def test_requires_three_unique_target_runs(self) -> None:
        with self.assertRaisesRegex(evidence.EvidenceError, "at least 3"):
            evidence.combine([Path("one.json"), Path("two.json")])

        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "run.json"
            write_artifact(path, artifact())
            with self.assertRaisesRegex(evidence.EvidenceError, "must be unique"):
                evidence.combine([path, path, path])

    def test_combines_consistent_runs_without_admitting_performance(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = []
            for index, scale in enumerate((1, 2, 3), start=1):
                path = root / f"run-{index}.json"
                write_artifact(path, artifact(scale))
                paths.append(path)

            report = evidence.combine(paths)

        self.assertTrue(report["mechanically_consistent"])
        self.assertFalse(report["performance_admitted"])
        self.assertTrue(report["human_baseline_review_required"])
        self.assertFalse(report["timing_threshold_selected"])
        self.assertEqual(report["run_count"], 3)
        self.assertEqual([phase["phase"] for phase in report["phases"]], list(evidence.PHASES))
        combined = next(phase for phase in report["phases"] if phase["phase"] == "combined")
        self.assertEqual(combined["cross_process"]["p50_median_ns"], 40)
        self.assertEqual(combined["cross_process"]["run_count"], 3)
        self.assertGreaterEqual(combined["cross_process"]["p99_over_p50_median_ppm"], 1_000_000)

    def test_dynamic_hardware_observations_do_not_break_consistency(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = []
            for index in range(3):
                data = artifact(index + 1)
                data["hardware"]["cpu_current_khz"] = str(2_000_000 + index)
                data["hardware"]["load_average"] = f"{index}.0"
                path = root / f"run-{index}.json"
                write_artifact(path, data)
                paths.append(path)
            report = evidence.combine(paths)
        self.assertTrue(report["mechanically_consistent"])

    def test_rejects_hardware_target_and_workload_drift(self) -> None:
        def expect_rejected(mutator, message: str) -> None:
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                paths = []
                for index in range(3):
                    data = artifact(index + 1)
                    if index == 2:
                        mutator(data)
                    path = root / f"run-{index}.json"
                    write_artifact(path, data)
                    paths.append(path)
                with self.assertRaisesRegex(evidence.EvidenceError, message):
                    evidence.combine(paths)

        expect_rejected(
            lambda data: data["hardware"].__setitem__("cpu_model", "different"),
            "stable hardware/toolchain",
        )
        expect_rejected(
            lambda data: data["target_qualification"].__setitem__("requested_cpu", "4"),
            "requested CPU",
        )
        expect_rejected(
            lambda data: data.__setitem__("expected_state_id", 2),
            "workload/structural witness",
        )

    def test_rejects_hosted_or_malformed_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            hosted = artifact()
            hosted.pop("target_qualification")
            hosted_path = root / "hosted.json"
            write_artifact(hosted_path, hosted)
            with self.assertRaisesRegex(evidence.EvidenceError, "target_qualification missing"):
                evidence.load_artifact(hosted_path)

            malformed = artifact()
            malformed["summary_ns"]["import"]["p95"] = 1
            malformed_path = root / "bad.json"
            write_artifact(malformed_path, malformed)
            with self.assertRaisesRegex(evidence.EvidenceError, "not monotone"):
                evidence.load_artifact(malformed_path)

            dense = artifact()
            dense["builder_counters"]["dense_sections"] = 1
            dense_path = root / "dense.json"
            write_artifact(dense_path, dense)
            with self.assertRaisesRegex(evidence.EvidenceError, "dense reference construction"):
                evidence.load_artifact(dense_path)

    def test_summary_helpers_are_integer_deterministic(self) -> None:
        self.assertEqual(evidence.median_int([9, 1, 5]), 5)
        self.assertEqual(evidence.mad_int([1, 5, 9]), 4)
        self.assertEqual(evidence.relative_ppm(3, 2), 1_500_000)
        with self.assertRaises(evidence.EvidenceError):
            evidence.median_int([])
        with self.assertRaises(evidence.EvidenceError):
            evidence.relative_ppm(1, 0)


if __name__ == "__main__":
    unittest.main()
