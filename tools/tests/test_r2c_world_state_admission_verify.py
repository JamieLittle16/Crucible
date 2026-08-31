from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_admission_promote as promote
from tools import r2c_world_state_admission_verify as verify
from tools.tests.test_r2c_world_state_admission_promote import PromotionFixture, pretty


class WorldStateAdmissionVerificationTests(unittest.TestCase):
    def test_freshly_promoted_bundle_verifies(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            promoted = promote.promote(fixture.staging, fixture.report, fixture.repo)
            result = verify.verify_repository(fixture.repo)
            self.assertTrue(result["verified"])
            self.assertTrue(result["source_admitted"])
            self.assertFalse(result["runtime_behavior_implemented"])
            self.assertEqual(result["manifest_sha256"], promoted["manifest_sha256"])

    def test_promoted_record_byte_drift_is_detected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            promote.promote(fixture.staging, fixture.report, fixture.repo)
            record = fixture.repo / promote.RECORD_ROOT / f"{fixture.var_id}.json"
            record.write_bytes(record.read_bytes() + b"\n")
            with self.assertRaisesRegex(verify.VerifyError, "promoted file drift"):
                verify.verify_repository(fixture.repo)

    def test_promoted_gate_byte_drift_is_detected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            promote.promote(fixture.staging, fixture.report, fixture.repo)
            gate = fixture.repo / promote.GATE_PATH
            gate.write_bytes(gate.read_bytes() + b"\n")
            with self.assertRaisesRegex(verify.VerifyError, "promoted file drift"):
                verify.verify_repository(fixture.repo)

    def test_manifest_cannot_point_outside_canonical_promotion_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            promote.promote(fixture.staging, fixture.report, fixture.repo)
            manifest_path = fixture.repo / promote.MANIFEST_PATH
            manifest = json.loads(manifest_path.read_text())
            manifest["files"][0]["path"] = "README.md"
            manifest_path.write_bytes(pretty(manifest))
            with self.assertRaisesRegex(verify.VerifyError, "non-canonical promotion path"):
                verify.verify_repository(fixture.repo)

    def test_manifest_report_binding_is_checked(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            promote.promote(fixture.staging, fixture.report, fixture.repo)
            manifest_path = fixture.repo / promote.MANIFEST_PATH
            manifest = json.loads(manifest_path.read_text())
            manifest["source_gate_report_sha256"] = "f" * 64
            manifest_path.write_bytes(pretty(manifest))
            with self.assertRaisesRegex(verify.VerifyError, "source-gate report digest"):
                verify.verify_repository(fixture.repo)

    def test_missing_manifest_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            with self.assertRaisesRegex(verify.VerifyError, "must be a real non-symlink file"):
                verify.verify_repository(repo)


if __name__ == "__main__":
    unittest.main()
