from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_import_resident_target_run as runner


def summary(count: int) -> dict[str, int]:
    return {"count": count, "p50": 10, "p95": 20, "p99": 30, "max": 40}


def raw_artifact(cpu: str = "3", measured_rounds: int = 2) -> dict[str, object]:
    chunks = 2
    region_files = 1
    dense_sections = 1
    uniform_sections = 3
    return {
        "schema": 1,
        "kind": runner.BENCHMARK_KIND,
        "mode": runner.RAW_MODE,
        "production_decision_eligible": False,
        "profile": dict(runner.EXPECTED_PROFILE),
        "hardware": {"cpus_allowed_list": cpu},
        "config": {
            "warmup_rounds": 1,
            "measured_rounds": measured_rounds,
            "filesystem_io_timed": False,
            "dimension_setup_timed_separately": True,
            "round_excludes_dimension_setup": True,
        },
        "world": {
            "region_files": region_files,
            "region_file_bytes": 7,
            "chunks": chunks,
        },
        "builder": {
            "uniform_sections": uniform_sections,
            "dense_sections": dense_sections,
            "dense_cells_copied": dense_sections * 4096,
            "retained_cells_written": (uniform_sections + dense_sections) * 4096,
        },
        "scratch": {
            "grew_during_measurement": False,
            "decoder_grew_during_measurement": False,
        },
        "samples_ns": {
            "dimension_setup": summary(measured_rounds),
            "region_open": summary(region_files * measured_rounds),
            "import": summary(chunks * measured_rounds),
            "install": summary(chunks * measured_rounds),
            "whole_chunk": summary(chunks * measured_rounds),
            "round": summary(measured_rounds),
        },
    }


def world_identity() -> dict[str, object]:
    return {
        "policy": runner.WORLD_ID_POLICY,
        "sha256": "a" * 64,
        "region_files": 1,
        "external_payload_files": 0,
        "region_file_bytes": 7,
        "external_payload_bytes": 0,
        "total_bytes": 7,
    }


class R2cImportResidentTargetRunTests(unittest.TestCase):
    def test_cpu_and_positive_values_are_fail_closed(self) -> None:
        self.assertEqual(runner.parse_cpu("17"), "17")
        with self.assertRaises(runner.TargetRunError):
            runner.parse_cpu("1-2")
        with self.assertRaises(runner.TargetRunError):
            runner.parse_cpu("-1")
        self.assertEqual(runner.parse_positive(3, "rounds"), 3)
        for value in (0, -1, True):
            with self.assertRaises(runner.TargetRunError):
                runner.parse_positive(value, "rounds")

    def test_benchmark_command_pins_cpu_and_complete_real_path(self) -> None:
        command = runner.benchmark_command("4", Path("/world"), Path("/tmp/raw.json"), 3, 12)
        self.assertEqual(command[:4], ["taskset", "-c", "4", "cargo"])
        self.assertIn("helve-world-load-qualification", command)
        self.assertIn("r2c_import_resident_bench", command)
        self.assertIn("--require-single-cpu", command)
        world_index = command.index("--world")
        self.assertEqual(command[world_index + 1], "/world")
        warmup_index = command.index("--warmup-rounds")
        measured_index = command.index("--measured-rounds")
        self.assertEqual(command[warmup_index + 1], "3")
        self.assertEqual(command[measured_index + 1], "12")

    def test_dimension_world_identity_is_deterministic_and_byte_sensitive(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            region = root / "region"
            region.mkdir()
            (region / "r.0.0.mca").write_bytes(b"region0")
            (region / "c.1.2.mcc").write_bytes(b"external")

            first = runner.dimension_world_identity(root)
            second = runner.dimension_world_identity(root)
            self.assertEqual(first, second)
            self.assertEqual(first["region_files"], 1)
            self.assertEqual(first["external_payload_files"], 1)
            self.assertEqual(first["region_file_bytes"], 7)
            self.assertEqual(first["external_payload_bytes"], 8)
            self.assertEqual(first["total_bytes"], 15)

            (region / "r.0.0.mca").write_bytes(b"region1")
            changed = runner.dimension_world_identity(root)
            self.assertNotEqual(first["sha256"], changed["sha256"])

    def test_dimension_world_identity_requires_real_region_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(runner.TargetRunError, "does not exist"):
                runner.dimension_world_identity(root)
            (root / "region").mkdir()
            with self.assertRaisesRegex(runner.TargetRunError, "no Anvil region files"):
                runner.dimension_world_identity(root)

    def test_annotation_preserves_raw_nondecision_and_adds_target_witness(self) -> None:
        data = raw_artifact()
        annotated = runner.annotate_artifact(data, "3", world_identity(), 1, 2)
        self.assertFalse(annotated["production_decision_eligible"])
        self.assertEqual(annotated["input_world"], world_identity())
        target = annotated["target_qualification"]
        self.assertEqual(target["runner"], runner.RUNNER_ID)
        self.assertEqual(target["requested_cpu"], "3")
        self.assertTrue(target["explicit_operator_action"])
        self.assertFalse(target["raw_benchmark_self_promoted"])

    def test_annotation_rejects_affinity_profile_and_scratch_drift(self) -> None:
        with self.assertRaisesRegex(runner.TargetRunError, "pinned"):
            runner.annotate_artifact(raw_artifact("2"), "3", world_identity(), 1, 2)

        profile = raw_artifact()
        profile["profile"] = {**runner.EXPECTED_PROFILE, "height": 256}
        with self.assertRaisesRegex(runner.TargetRunError, "profile"):
            runner.annotate_artifact(profile, "3", world_identity(), 1, 2)

        scratch = raw_artifact()
        scratch["scratch"]["grew_during_measurement"] = True
        with self.assertRaisesRegex(runner.TargetRunError, "scratch grew"):
            runner.annotate_artifact(scratch, "3", world_identity(), 1, 2)

    def test_annotation_rejects_builder_and_sample_accounting_drift(self) -> None:
        builder = raw_artifact()
        builder["builder"]["dense_cells_copied"] -= 1
        with self.assertRaisesRegex(runner.TargetRunError, "dense section copy"):
            runner.annotate_artifact(builder, "3", world_identity(), 1, 2)

        samples = raw_artifact()
        samples["samples_ns"]["whole_chunk"]["count"] -= 1
        with self.assertRaisesRegex(runner.TargetRunError, "whole_chunk.count"):
            runner.annotate_artifact(samples, "3", world_identity(), 1, 2)

    def test_annotation_rejects_world_and_config_drift_or_double_stamp(self) -> None:
        wrong_world = raw_artifact()
        wrong_world["world"]["region_file_bytes"] = 9
        with self.assertRaisesRegex(runner.TargetRunError, "region-byte"):
            runner.annotate_artifact(wrong_world, "3", world_identity(), 1, 2)

        wrong_config = raw_artifact()
        wrong_config["config"]["measured_rounds"] = 3
        with self.assertRaisesRegex(runner.TargetRunError, "measured_rounds"):
            runner.annotate_artifact(wrong_config, "3", world_identity(), 1, 2)

        stamped = raw_artifact()
        stamped["target_qualification"] = {}
        with self.assertRaisesRegex(runner.TargetRunError, "already annotated"):
            runner.annotate_artifact(stamped, "3", world_identity(), 1, 2)

    def test_target_process_refuses_overwrite_before_invoking_command(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "evidence.json"
            output.write_text("existing", encoding="utf-8")
            calls: list[list[str]] = []

            def command(command: list[str], check: bool) -> None:
                calls.append(command)
                self.assertTrue(check)

            with self.assertRaisesRegex(runner.TargetRunError, "overwrite"):
                runner.run_target_process("1", root, output, 1, 2, command)
            self.assertEqual(calls, [])

    def test_target_process_rejects_world_mutation_during_measurement(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dimension = root / "dimension"
            region = dimension / "region"
            region.mkdir(parents=True)
            region_file = region / "r.0.0.mca"
            region_file.write_bytes(b"region0")
            output = root / "evidence.json"

            def command(command: list[str], check: bool) -> None:
                self.assertTrue(check)
                raw = Path(command[command.index("--output") + 1])
                raw.write_text(json.dumps(raw_artifact("1")), encoding="utf-8")
                region_file.write_bytes(b"region1")

            with self.assertRaisesRegex(runner.TargetRunError, "changed during target measurement"):
                runner.run_target_process("1", dimension, output, 1, 2, command)
            self.assertFalse(output.exists())

    def test_target_process_writes_only_valid_annotated_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dimension = root / "dimension"
            region = dimension / "region"
            region.mkdir(parents=True)
            (region / "r.0.0.mca").write_bytes(b"region0")
            output = root / "evidence.json"

            def command(command: list[str], check: bool) -> None:
                self.assertTrue(check)
                raw = Path(command[command.index("--output") + 1])
                raw.write_text(json.dumps(raw_artifact("1")), encoding="utf-8")

            runner.run_target_process("1", dimension, output, 1, 2, command)
            data = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(data["target_qualification"]["requested_cpu"], "1")
            self.assertEqual(data["input_world"]["region_files"], 1)
            self.assertEqual(data["input_world"]["region_file_bytes"], 7)


if __name__ == "__main__":
    unittest.main()
