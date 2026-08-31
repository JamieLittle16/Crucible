from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_admission_doctor as doctor
from tools import r2c_world_state_admission_prepare as prepare
from tools.tests.test_r2c_world_state_admission_materialize import (
    author_complete_worksheet,
    review_result,
)
from tools.tests.test_r2c_world_state_admission_promote import PromotionFixture
from tools.tests.test_r2c_world_state_source_review_finalize import fixture_values, pretty


class WorldStateAdmissionDoctorTests(unittest.TestCase):
    def write_review_fixture(self, root: Path) -> tuple[Path, Path]:
        pack, worksheet = fixture_values()
        pack_path = root / "review-pack.json"
        worksheet_path = root / "review-worksheet.json"
        pack_path.write_bytes(pretty(pack))
        worksheet_path.write_bytes(pretty(worksheet))
        return pack_path, worksheet_path

    def write_admission_fixture(self, root: Path) -> tuple[Path, Path]:
        review_path = root / "review-result.json"
        review_path.write_text(
            json.dumps(review_result(), indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        worksheet = root / "admission.json"
        prepare.prepare(review_path, worksheet)
        author_complete_worksheet(worksheet)
        return review_path, worksheet

    def test_complete_source_review_is_finalize_ready(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pack, worksheet = self.write_review_fixture(Path(tmp))
            result = doctor.diagnose_review(pack, worksheet)
            self.assertEqual(result["phase"], "source-review")
            self.assertTrue(result["ready_for_next_step"])
            self.assertEqual(result["blockers"], [])
            self.assertFalse(result["contains_official_source_text"])
            self.assertFalse(result["semantic_inference_performed"])

    def test_review_doctor_reports_multiple_human_obligations_at_once(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pack, worksheet_path = self.write_review_fixture(Path(tmp))
            worksheet = json.loads(worksheet_path.read_text())
            group = worksheet["groups"][2]
            group["source_inspected"] = False
            group["review_complete"] = False
            group["followup_dependencies"] = ["inspect delegate"]
            group["semantic_observations"] = []
            group["hazards_reviewed"] = []
            group["selected_source_identities"] = []
            group["rejected_source_identities"] = []
            worksheet_path.write_bytes(pretty(worksheet))

            result = doctor.diagnose_review(pack, worksheet_path)
            codes = {blocker["code"] for blocker in result["blockers"]}
            self.assertFalse(result["ready_for_next_step"])
            self.assertTrue(
                {
                    "source-not-inspected",
                    "review-not-complete",
                    "followup-dependencies-open",
                    "semantic-observation-required",
                    "selected-source-required",
                    "candidate-partition-incomplete",
                }.issubset(codes)
            )

    def test_complete_semantic_admission_is_materialization_ready(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            review_path, worksheet = self.write_admission_fixture(Path(tmp))
            result = doctor.diagnose_admission(review_path, worksheet)
            self.assertEqual(result["phase"], "semantic-admission")
            self.assertTrue(result["ready_for_next_step"])
            self.assertEqual(result["blockers"], [])

    def test_admission_doctor_reports_missing_rules_and_completion(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            review_path, worksheet_path = self.write_admission_fixture(Path(tmp))
            worksheet = json.loads(worksheet_path.read_text())
            worksheet["all_groups_admission_complete"] = False
            # Remove the biome group's rule: unlike the heightmap-only shared source,
            # this leaves a biome-only selected source globally unsupported, matching
            # the materializer's actual every-selected-source-must-support-a-rule law.
            worksheet["groups"][0]["admission_complete"] = False
            worksheet["groups"][0]["semantic_rules"] = []
            worksheet_path.write_text(json.dumps(worksheet), encoding="utf-8")

            result = doctor.diagnose_admission(review_path, worksheet_path)
            codes = {blocker["code"] for blocker in result["blockers"]}
            self.assertFalse(result["ready_for_next_step"])
            self.assertIn("all-groups-admission-incomplete", codes)
            self.assertIn("admission-not-complete", codes)
            self.assertIn("semantic-rule-required", codes)
            self.assertIn("selected-source-unused-by-semantic-rules", codes)

    def test_staging_without_bound_report_points_to_r2c_gate_wrapper(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            result = doctor.diagnose_staging(fixture.staging, None)
            self.assertEqual(result["phase"], "source-gate")
            self.assertFalse(result["ready_for_next_step"])
            self.assertEqual(result["blockers"][0]["code"], "manifest-bound-source-gate-required")
            self.assertIn("r2c_world_state_source_gate.py", result["next_step"])

    def test_manifest_bound_admitted_staging_is_promotion_ready(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            result = doctor.diagnose_staging(fixture.staging, fixture.report)
            self.assertEqual(result["phase"], "promotion-ready")
            self.assertTrue(result["ready_for_next_step"])
            self.assertEqual(result["blockers"], [])
            self.assertEqual(result["var_records"], 1)
            self.assertEqual(result["semantic_rules"], 1)

    def test_generic_unbound_gate_report_is_not_promotion_ready(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            report = json.loads(fixture.report.read_text())
            report.pop("materialization_id")
            report.pop("materialization_manifest_sha256")
            report.pop("source_free_bundle_bound")
            fixture.report.write_text(json.dumps(report), encoding="utf-8")

            result = doctor.diagnose_staging(fixture.staging, fixture.report)
            self.assertFalse(result["ready_for_next_step"])
            self.assertEqual(result["blockers"][0]["code"], "source-gate-not-promotion-ready")


if __name__ == "__main__":
    unittest.main()
