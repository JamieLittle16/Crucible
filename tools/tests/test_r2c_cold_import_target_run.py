from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools import r2c_cold_import_target_run as target_run


def raw_artifact(cpu: str = "3", rounds: int = 4) -> dict[str, object]:
    summary = {"p50": 10, "p95": 12, "p99": 13, "p999": 14, "max": 15}
    return {
        "schema": 1,
        "kind": target_run.KIND,
        "mode": "full",
        "reference_section_builder": True,
        "production_section_policy_selected": False,
        "performance_admitted": False,
        "warmup_rounds": 2,
        "measured_rounds": rounds,
        "expected_state_id": 1,
        "hardware": {"cpus_allowed_list": cpu},
        "summary_ns": {
            "import": dict(summary),
            "install": dict(summary),
            "combined": dict(summary),
        },
        "builder_counters": {
            "uniform_sections": rounds * 2,
            "dense_sections": 0,
            "dense_cell_writes": 0,
        },
        "samples": [
            {"import_ns": 10 + index, "install_ns": 5 + index, "combined_ns": 20 + index}
            for index in range(rounds)
        ],
    }


class R2CColdImportTargetRunTests(unittest.TestCase):
    def test_cpu_must_be_one_decimal_logical_cpu(self) -> None:
        self.assertEqual(target_run.parse_cpu("0"), "0")
        self.assertEqual(target_run.parse_cpu("17"), "17")
        for invalid in ("", "2-3", "1,2", "-1", "cpu3"):
            with self.subTest(invalid=invalid):
                with self.assertRaises(target_run.TargetRunError):
                    target_run.parse_cpu(invalid)

    def test_command_is_full_release_locked_and_affinity_checked(self) -> None:
        command = target_run.benchmark_command("7", Path("/tmp/raw.json"))
        self.assertEqual(command[:4], ["taskset", "-c", "7", "cargo"])
        self.assertIn("--release", command)
        self.assertIn("--locked", command)
        self.assertIn("r2c_cold_path_bench", command)
        self.assertIn("--full", command)
        self.assertIn("--require-single-cpu", command)
        self.assertEqual(command[-2:], ["--output", "/tmp/raw.json"])

    def test_annotation_stamps_explicit_non_hosted_target_witness(self) -> None:
        source = raw_artifact("3")
        annotated = target_run.annotate_artifact(source, "3")
        self.assertNotIn("target_qualification", source)
        witness = annotated["target_qualification"]
        self.assertEqual(witness["runner"], target_run.RUNNER_ID)
        self.assertTrue(witness["explicit_operator_action"])
        self.assertTrue(witness["single_cpu_requirement_enforced"])
        self.assertEqual(witness["requested_cpu"], "3")
        self.assertFalse(witness["hosted_ci_eligible"])

    def test_annotation_rejects_affinity_policy_and_shape_drift(self) -> None:
        with self.assertRaisesRegex(target_run.TargetRunError, "did not remain pinned"):
            target_run.annotate_artifact(raw_artifact("4"), "3")

        wrong_mode = raw_artifact()
        wrong_mode["mode"] = "smoke"
        with self.assertRaisesRegex(target_run.TargetRunError, "mode"):
            target_run.annotate_artifact(wrong_mode, "3")

        wrong_policy = raw_artifact()
        wrong_policy["production_section_policy_selected"] = True
        with self.assertRaisesRegex(target_run.TargetRunError, "production_section_policy_selected"):
            target_run.annotate_artifact(wrong_policy, "3")

        wrong_count = raw_artifact()
        wrong_count["builder_counters"]["uniform_sections"] = 1
        with self.assertRaisesRegex(target_run.TargetRunError, "uniform section count"):
            target_run.annotate_artifact(wrong_count, "3")

        wrong_samples = raw_artifact()
        wrong_samples["samples"] = []
        with self.assertRaisesRegex(target_run.TargetRunError, "samples length"):
            target_run.annotate_artifact(wrong_samples, "3")

    def test_annotation_rejects_bad_summary_and_preannotation(self) -> None:
        malformed = raw_artifact()
        malformed["summary_ns"]["combined"] = {
            "p50": 10,
            "p95": 9,
            "p99": 13,
            "p999": 14,
            "max": 15,
        }
        with self.assertRaisesRegex(target_run.TargetRunError, "not monotone"):
            target_run.annotate_artifact(malformed, "3")

        source = raw_artifact()
        source["target_qualification"] = {"forged": True}
        with self.assertRaisesRegex(target_run.TargetRunError, "already contains"):
            target_run.annotate_artifact(source, "3")

    def test_target_process_writes_only_annotated_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "target-run.json"

            def fake_runner(command: list[str], *, check: bool) -> subprocess.CompletedProcess[str]:
                self.assertTrue(check)
                self.assertIn("--require-single-cpu", command)
                raw = Path(command[-1])
                raw.write_text(json.dumps(raw_artifact("3")), encoding="utf-8")
                return subprocess.CompletedProcess(command, 0)

            target_run.run_target_process("3", output, command_runner=fake_runner)
            written = json.loads(output.read_text(encoding="utf-8"))

        self.assertEqual(written["target_qualification"]["requested_cpu"], "3")
        self.assertFalse(written["target_qualification"]["hosted_ci_eligible"])

    def test_target_process_refuses_overwrite_failure_and_missing_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "target-run.json"
            output.write_text("preserve me", encoding="utf-8")
            with self.assertRaisesRegex(target_run.TargetRunError, "refusing to overwrite"):
                target_run.run_target_process("3", output, command_runner=lambda *_args, **_kwargs: None)
            self.assertEqual(output.read_text(encoding="utf-8"), "preserve me")

        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "target-run.json"

            def failed_runner(command: list[str], *, check: bool) -> None:
                raise subprocess.CalledProcessError(1, command)

            with self.assertRaisesRegex(target_run.TargetRunError, "benchmark command failed"):
                target_run.run_target_process("3", output, command_runner=failed_runner)
            self.assertFalse(output.exists())

        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "target-run.json"

            def missing_runner(command: list[str], *, check: bool) -> subprocess.CompletedProcess[str]:
                return subprocess.CompletedProcess(command, 0)

            with self.assertRaisesRegex(target_run.TargetRunError, "valid JSON"):
                target_run.run_target_process("3", output, command_runner=missing_runner)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
