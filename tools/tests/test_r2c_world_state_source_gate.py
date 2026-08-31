from __future__ import annotations

import copy
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r2c_world_state_admission_materialize as materialize
from tools import r2c_world_state_admission_promote as promote
from tools import r2c_world_state_source_gate as bound_gate
from tools.tests.test_r2c_world_state_admission_promote import PromotionFixture, digest


def generic_report(fixture: PromotionFixture) -> dict[str, object]:
    report = copy.deepcopy(fixture.report_value)
    report.pop("materialization_id")
    report.pop("materialization_manifest_sha256")
    report.pop("source_free_bundle_bound")
    return report


class WorldStateBoundSourceGateTests(unittest.TestCase):
    def test_bound_gate_hashes_complete_materialization_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            with mock.patch.object(
                bound_gate.source_gate,
                "evaluate",
                return_value=generic_report(fixture),
            ) as evaluate:
                result = bound_gate.evaluate_bound(
                    db_path=Path(tmp) / "atlas.sqlite",
                    staging_dir=fixture.staging,
                )

            evaluate.assert_called_once_with(
                db_path=Path(tmp) / "atlas.sqlite",
                gate_path=fixture.staging / "gate.json",
                records_dir=fixture.staging / "records",
            )
            self.assertEqual(result["materialization_id"], materialize.ID)
            self.assertEqual(
                result["materialization_manifest_sha256"], digest(fixture.manifest_raw)
            )
            self.assertTrue(result["source_free_bundle_bound"])
            self.assertTrue(result["admitted"])

    def test_wrong_generic_gate_identity_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            report = generic_report(fixture)
            report["gate_id"] = "GATE-WRONG"
            with mock.patch.object(bound_gate.source_gate, "evaluate", return_value=report):
                with self.assertRaisesRegex(bound_gate.BoundGateError, "wrong gate identity"):
                    bound_gate.evaluate_bound(
                        db_path=Path(tmp) / "atlas.sqlite",
                        staging_dir=fixture.staging,
                    )

    def test_wrong_generic_gate_digest_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            report = generic_report(fixture)
            report["gate_sha256"] = "a" * 64
            with mock.patch.object(bound_gate.source_gate, "evaluate", return_value=report):
                with self.assertRaisesRegex(bound_gate.BoundGateError, "exact staged gate"):
                    bound_gate.evaluate_bound(
                        db_path=Path(tmp) / "atlas.sqlite",
                        staging_dir=fixture.staging,
                    )

    def test_staged_semantic_drift_fails_before_generic_gate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            semantics = fixture.staging / "semantics" / materialize.SEMANTICS_FILE
            semantics.write_bytes(semantics.read_bytes() + b"drift")
            with mock.patch.object(bound_gate.source_gate, "evaluate") as evaluate:
                with self.assertRaisesRegex(promote.PromoteError, "differs from materialization manifest"):
                    bound_gate.evaluate_bound(
                        db_path=Path(tmp) / "atlas.sqlite",
                        staging_dir=fixture.staging,
                    )
            evaluate.assert_not_called()


if __name__ == "__main__":
    unittest.main()
