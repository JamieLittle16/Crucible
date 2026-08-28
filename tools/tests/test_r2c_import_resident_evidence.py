from __future__ import annotations

import copy
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_import_resident_evidence as evidence


def hardware(cpu: str = "3") -> dict[str, str]:
    result = {field: f"fixture-{field}" for field in evidence.STABLE_HARDWARE_FIELDS}
    result["commit_sha"] = "a" * 40
    result["cpus_allowed_list"] = cpu
    result["rustflags"] = ""
    result["cargo_encoded_rustflags"] = ""
    result["cpu_current_khz"] = "123"
    result["load_average"] = "0.1 0.2 0.3"
    return result


def summary(count: int, offset: int = 0) -> dict[str, int]:
    return {
        "count": count,
        "p50": 100 + offset,
        "p95": 150 + offset,
        "p99": 180 + offset,
        "max": 220 + offset,
    }


def artifact(offset: int = 0) -> dict[str, object]:
    measured = 2
    chunks = 3
    regions = 1
    return {
        "schema": 1,
        "kind": evidence.BENCHMARK_KIND,
        "mode": evidence.RAW_MODE,
        "production_decision_eligible": False,
        "hardware": hardware(),
        "target_qualification": {
            "schema": 1,
            "runner": evidence.TARGET_RUNNER_ID,
            "explicit_operator_action": True,
            "single_cpu_requirement_enforced": True,
            "requested_cpu": "3",
            "raw_benchmark_self_promoted": False,
            "hosted_ci_eligible": False,
        },
        "input_world": {
            "policy": evidence.WORLD_ID_POLICY,
            "sha256": "b" * 64,
            "region_files": regions,
            "external_payload_files": 1,
            "region_file_bytes": 8192,
            "external_payload_bytes": 1024,
            "total_bytes": 9216,
        },
        "world": {
            "region_files": regions,
            "region_file_bytes": 8192,
            "chunks": chunks,
            "compressed_payload_bytes": 2048,
        },
        "profile": {
            "dimension": "minecraft:overworld",
            "min_block_y": -64,
            "height": 384,
            "section_count": 24,
        },
        "config": {
            "warmup_rounds": 3,
            "measured_rounds": measured,
            "filesystem_io_timed": False,
            "dimension_setup_timed_separately": True,
            "round_excludes_dimension_setup": True,
        },
        "mechanism": {"section": "qualification-direct-4096", "decoder": "deflate"},
        "state_data": {"input_sha256": "c" * 64, "generation_sha256": "d" * 64},
        "builder": {
            "uniform_sections": 4,
            "dense_sections": 6,
            "dense_cells_copied": 6 * 4096,
            "retained_cells_written": 10 * 4096,
        },
        "scratch": {
            "before_measurement": {"palette": 8, "packed_words": 256, "states": 4096},
            "after_measurement": {"palette": 8, "packed_words": 256, "states": 4096},
            "decoder_before_measurement": 16 * 1024 * 1024,
            "decoder_after_measurement": 16 * 1024 * 1024,
            "grew_during_measurement": False,
            "decoder_grew_during_measurement": False,
        },
        "empty_sections_synthesized": 42,
        "semantic_checksum": 123456,
        "samples_ns": {
            "dimension_setup": summary(measured, offset),
            "region_open": summary(regions * measured, offset),
            "import": summary(chunks * measured, offset),
            "install": summary(chunks * measured, offset),
            "whole_chunk": summary(chunks * measured, offset),
            "round": summary(measured, offset),
        },
    }


def write(path: Path, data: dict[str, object]) -> None:
    path.write_text(json.dumps(data), encoding="utf-8")


class R2cImportResidentEvidenceTests(unittest.TestCase):
    def test_three_consistent_target_runs_combine_without_admission(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = []
            for index, offset in enumerate((0, 10, 20), start=1):
                path = root / f"run-{index}.json"
                write(path, artifact(offset))
                paths.append(path)

            report = evidence.combine(paths)
            self.assertEqual(report["kind"], evidence.OUTPUT_KIND)
            self.assertEqual(report["run_count"], 3)
            self.assertTrue(report["mechanically_consistent"])
            self.assertFalse(report["performance_admitted"])
            self.assertTrue(report["human_baseline_review_required"])
            self.assertFalse(report["timing_threshold_selected"])
            self.assertEqual(report["workload_witness"]["input_world"]["sha256"], "b" * 64)
            stages = {stage["name"]: stage for stage in report["stages"]}
            self.assertEqual(stages["whole_chunk"]["cross_process"]["p50_median_ns"], 110)
            self.assertEqual(stages["whole_chunk"]["cross_process"]["p50_mad_ns"], 10)
            self.assertEqual(stages["round"]["cross_process"]["run_count"], 3)

    def test_combiner_requires_three_unique_paths(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            first = root / "first.json"
            second = root / "second.json"
            write(first, artifact())
            write(second, artifact())
            with self.assertRaisesRegex(evidence.EvidenceError, "at least 3"):
                evidence.combine([first, second])
            with self.assertRaisesRegex(evidence.EvidenceError, "unique"):
                evidence.combine([first, first, second])

    def test_unstamped_raw_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "run.json"
            data = artifact()
            del data["target_qualification"]
            write(path, data)
            with self.assertRaisesRegex(evidence.EvidenceError, "target_qualification missing"):
                evidence.load_artifact(path)

    def test_world_byte_identity_and_hardware_drift_fail_combination(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rows = [artifact(), artifact(10), artifact(20)]
            rows[1]["input_world"]["sha256"] = "e" * 64
            paths = []
            for index, row in enumerate(rows):
                path = root / f"world-{index}.json"
                write(path, row)
                paths.append(path)
            with self.assertRaisesRegex(evidence.EvidenceError, "input world/workload"):
                evidence.combine(paths)

            rows = [artifact(), artifact(10), artifact(20)]
            rows[2]["hardware"]["cpu_model"] = "different"
            paths = []
            for index, row in enumerate(rows):
                path = root / f"hardware-{index}.json"
                write(path, row)
                paths.append(path)
            with self.assertRaisesRegex(evidence.EvidenceError, "stable hardware"):
                evidence.combine(paths)

    def test_dynamic_frequency_and_load_are_not_stable_hardware_keys(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rows = [artifact(), artifact(10), artifact(20)]
            rows[1]["hardware"]["cpu_current_khz"] = "456"
            rows[2]["hardware"]["load_average"] = "9.0 8.0 7.0"
            paths = []
            for index, row in enumerate(rows):
                path = root / f"dynamic-{index}.json"
                write(path, row)
                paths.append(path)
            report = evidence.combine(paths)
            self.assertTrue(report["mechanically_consistent"])

    def test_builder_scratch_and_sample_accounting_are_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)

            bad_builder = artifact()
            bad_builder["builder"]["dense_cells_copied"] -= 1
            path = root / "builder.json"
            write(path, bad_builder)
            with self.assertRaisesRegex(evidence.EvidenceError, "dense section copy"):
                evidence.load_artifact(path)

            bad_scratch = artifact()
            bad_scratch["scratch"]["decoder_grew_during_measurement"] = True
            path = root / "scratch.json"
            write(path, bad_scratch)
            with self.assertRaisesRegex(evidence.EvidenceError, "decoder scratch grew"):
                evidence.load_artifact(path)

            bad_samples = artifact()
            bad_samples["samples_ns"]["whole_chunk"]["count"] = 5
            path = root / "samples.json"
            write(path, bad_samples)
            with self.assertRaisesRegex(evidence.EvidenceError, "whole_chunk sample count"):
                evidence.load_artifact(path)

    def test_nonmonotone_summary_and_world_accounting_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)

            nonmonotone = artifact()
            nonmonotone["samples_ns"]["install"]["p95"] = 1
            path = root / "summary.json"
            write(path, nonmonotone)
            with self.assertRaisesRegex(evidence.EvidenceError, "non-monotone"):
                evidence.load_artifact(path)

            bad_world = artifact()
            bad_world["input_world"]["total_bytes"] += 1
            path = root / "world.json"
            write(path, bad_world)
            with self.assertRaisesRegex(evidence.EvidenceError, "byte accounting"):
                evidence.load_artifact(path)

    def test_target_cpu_witness_must_match_observed_affinity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "cpu.json"
            data = artifact()
            data["target_qualification"]["requested_cpu"] = "4"
            write(path, data)
            with self.assertRaisesRegex(evidence.EvidenceError, "requested CPU"):
                evidence.load_artifact(path)

    def test_workload_drift_in_measured_rounds_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rows = [artifact(), artifact(10), artifact(20)]
            drifted = copy.deepcopy(rows[2])
            drifted["config"]["measured_rounds"] = 3
            # Keep its local sample counts internally valid so the cross-run witness is what rejects it.
            drifted["samples_ns"]["dimension_setup"]["count"] = 3
            drifted["samples_ns"]["round"]["count"] = 3
            drifted["samples_ns"]["region_open"]["count"] = 3
            for name in ("import", "install", "whole_chunk"):
                drifted["samples_ns"][name]["count"] = 9
            rows[2] = drifted
            paths = []
            for index, row in enumerate(rows):
                path = root / f"rounds-{index}.json"
                write(path, row)
                paths.append(path)
            with self.assertRaisesRegex(evidence.EvidenceError, "input world/workload"):
                evidence.combine(paths)


if __name__ == "__main__":
    unittest.main()
