import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r0_status_admission as admission
from tools.protocol_capture_admission import EvidenceConvergenceError
from tools.protocol_codegen import CodegenError


class R0StatusAdmissionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.db = self.root / "atlas.sqlite"
        self.db.write_bytes(b"synthetic-atlas-placeholder")
        self.gate = self.root / "gate.json"
        self.gate.write_text("{}", encoding="utf-8")
        self.lock = self.root / "vanilla.lock.toml"
        self.lock.write_text("", encoding="utf-8")
        self.records = self.root / "records"
        self.records.mkdir()
        self.capture = self.root / "capture.json"
        self.capture.write_text("{}", encoding="utf-8")
        self.generated = self.root / "generated.rs"
        self.generated.write_text("pub const TEST: i32 = 1;\n", encoding="utf-8")

        self.contract = {
            "schema": 1,
            "id": "PROTO-TEST-STATUS-001",
            "target": {
                "minecraft": "test-version",
                "protocol": 42,
                "source_archive_sha256": "a" * 64,
                "fingerprint_algorithm": "test-fingerprint-v1",
            },
            "packets": [
                {
                    "name": "status-request",
                    "phase": "status",
                    "direction": "serverbound",
                    "id": 0,
                    "semantic_rules": ["SEM-TEST-001"],
                    "source_records": ["VAR-TEST-001"],
                    "golden": {"body_hex": "00", "frame_hex": "0100"},
                },
                {
                    "name": "status-response",
                    "phase": "status",
                    "direction": "clientbound",
                    "id": 0,
                    "semantic_rules": ["SEM-TEST-001"],
                    "source_records": ["VAR-TEST-001"],
                    "golden": {"body_hex": "0001", "frame_hex": "020001"},
                },
            ],
        }
        self.contract_path = self.root / "contract.json"
        self._write_contract()

        self.source_report = {
            "schema": 1,
            "gate_id": "GATE-TEST-R0-001",
            "admitted": True,
            "gate_path": str(self.gate),
            "gate_sha256": "d" * 64,
            "source": {
                "minecraft_version": "test-version",
                "protocol_version": "42",
                "archive_sha256": "a" * 64,
                "fingerprint_algorithm": "test-fingerprint-v1",
            },
            "required_methods": [
                {
                    "var_id": "VAR-TEST-001",
                    "record_sha256": "e" * 64,
                    "source": "net.minecraft.test.Status#handle()",
                    "normalized_sha256": "b" * 64,
                    "body_sha256": "c" * 64,
                    "semantic_rules": ["SEM-TEST-001"],
                }
            ],
            "failures": [],
        }
        self.convergence = {
            "schema": 1,
            "contract_id": "PROTO-TEST-STATUS-001",
            "capture_sha256": "f" * 64,
            "minecraft": "test-version",
            "protocol": 42,
            "client_to_server_frames": 1,
            "server_to_client_frames": 1,
            "frames_matched": 2,
        }
        self.rendered = "pub const TEST: i32 = 1;\n"

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _write_contract(self, value: dict | None = None) -> None:
        self.contract_path.write_text(
            json.dumps(self.contract if value is None else value, separators=(",", ":")),
            encoding="utf-8",
        )

    def _run(
        self,
        *,
        source_report: dict | None = None,
        convergence: dict | None = None,
        rendered: str | None = None,
    ) -> dict[str, object]:
        source = self.source_report if source_report is None else source_report
        converged = self.convergence if convergence is None else convergence
        generated = self.rendered if rendered is None else rendered
        with (
            mock.patch.object(admission, "evaluate_source_gate", return_value=source) as source_mock,
            mock.patch.object(admission, "crosscheck_capture", return_value=converged) as capture_mock,
            mock.patch.object(admission, "generate_protocol", return_value=generated) as codegen_mock,
        ):
            result = admission.admit_r0_session(
                db_path=self.db,
                source_gate_path=self.gate,
                contract_path=self.contract_path,
                capture_path=self.capture,
                generated_rust_path=self.generated,
                lock_path=self.lock,
                records_root=self.records,
            )
        source_mock.assert_called_once_with(
            db_path=self.db,
            gate_path=self.gate,
            records_dir=self.records,
        )
        capture_mock.assert_called_once_with(
            self.contract_path,
            self.capture,
            lock_path=self.lock,
            records_root=self.records,
        )
        codegen_mock.assert_called_once_with(
            self.contract_path,
            lock_path=self.lock,
            records_root=self.records,
            output_path=self.generated,
            check=True,
        )
        return result

    def test_valid_session_binds_all_independent_evidence(self) -> None:
        result = self._run()
        self.assertEqual(result["schema"], 1)
        self.assertEqual(result["kind"], "r0-status-admission-v1")
        self.assertEqual(result["target"], self.contract["target"])
        self.assertEqual(result["source_gate"]["id"], "GATE-TEST-R0-001")
        self.assertEqual(
            result["contract"],
            {
                "id": "PROTO-TEST-STATUS-001",
                "source_records": ["VAR-TEST-001"],
            },
        )
        self.assertEqual(result["capture"]["sha256"], "f" * 64)
        self.assertEqual(result["capture"]["frames_matched"], 2)
        self.assertEqual(
            result["generated_rust"]["sha256"],
            hashlib.sha256(self.rendered.encode("utf-8")).hexdigest(),
        )
        identity = dict(result)
        session_digest = identity.pop("session_sha256")
        self.assertEqual(session_digest, admission._session_digest(identity))

    def test_unchanged_evidence_produces_identical_session_identity(self) -> None:
        first = self._run()
        second = self._run()
        self.assertEqual(first, second)
        self.assertEqual(first["session_sha256"], second["session_sha256"])

    def test_source_gate_rejection_stops_before_capture_or_codegen(self) -> None:
        rejected = copy.deepcopy(self.source_report)
        rejected["admitted"] = False
        rejected["failures"] = ["VAR-TEST-001: stale"]
        with (
            mock.patch.object(admission, "evaluate_source_gate", return_value=rejected),
            mock.patch.object(admission, "crosscheck_capture") as capture_mock,
            mock.patch.object(admission, "generate_protocol") as codegen_mock,
            self.assertRaisesRegex(admission.R0AdmissionError, "source admission gate rejected"),
        ):
            admission.admit_r0_session(
                db_path=self.db,
                source_gate_path=self.gate,
                contract_path=self.contract_path,
                capture_path=self.capture,
                generated_rust_path=self.generated,
                lock_path=self.lock,
                records_root=self.records,
            )
        capture_mock.assert_not_called()
        codegen_mock.assert_not_called()

    def test_every_target_identity_dimension_must_match_source_gate(self) -> None:
        mutations = {
            "minecraft": "other-version",
            "protocol": 43,
            "source_archive_sha256": "9" * 64,
            "fingerprint_algorithm": "other-fingerprint",
        }
        for field, value in mutations.items():
            with self.subTest(field=field):
                contract = copy.deepcopy(self.contract)
                contract["target"][field] = value
                self._write_contract(contract)
                with (
                    mock.patch.object(
                        admission, "evaluate_source_gate", return_value=self.source_report
                    ),
                    mock.patch.object(admission, "crosscheck_capture") as capture_mock,
                    mock.patch.object(admission, "generate_protocol") as codegen_mock,
                    self.assertRaisesRegex(admission.R0AdmissionError, "identity does not match"),
                ):
                    admission.admit_r0_session(
                        db_path=self.db,
                        source_gate_path=self.gate,
                        contract_path=self.contract_path,
                        capture_path=self.capture,
                        generated_rust_path=self.generated,
                        lock_path=self.lock,
                        records_root=self.records,
                    )
                capture_mock.assert_not_called()
                codegen_mock.assert_not_called()
                self._write_contract()

    def test_contract_cannot_cite_reviewed_but_currently_ungated_var(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["packets"][0]["source_records"].append("VAR-TEST-UNGATED-002")
        self._write_contract(contract)
        with (
            mock.patch.object(admission, "evaluate_source_gate", return_value=self.source_report),
            mock.patch.object(admission, "crosscheck_capture") as capture_mock,
            mock.patch.object(admission, "generate_protocol") as codegen_mock,
            self.assertRaisesRegex(admission.R0AdmissionError, "not admitted by the current source gate"),
        ):
            admission.admit_r0_session(
                db_path=self.db,
                source_gate_path=self.gate,
                contract_path=self.contract_path,
                capture_path=self.capture,
                generated_rust_path=self.generated,
                lock_path=self.lock,
                records_root=self.records,
            )
        capture_mock.assert_not_called()
        codegen_mock.assert_not_called()

    def test_capture_convergence_failure_propagates_and_blocks_codegen(self) -> None:
        with (
            mock.patch.object(admission, "evaluate_source_gate", return_value=self.source_report),
            mock.patch.object(
                admission,
                "crosscheck_capture",
                side_effect=EvidenceConvergenceError("captured frame disagrees"),
            ),
            mock.patch.object(admission, "generate_protocol") as codegen_mock,
            self.assertRaisesRegex(EvidenceConvergenceError, "captured frame disagrees"),
        ):
            admission.admit_r0_session(
                db_path=self.db,
                source_gate_path=self.gate,
                contract_path=self.contract_path,
                capture_path=self.capture,
                generated_rust_path=self.generated,
                lock_path=self.lock,
                records_root=self.records,
            )
        codegen_mock.assert_not_called()

    def test_convergence_summary_target_must_still_match_source_gate(self) -> None:
        convergence = copy.deepcopy(self.convergence)
        convergence["protocol"] = 43
        with self.assertRaisesRegex(admission.R0AdmissionError, "convergence summary"):
            self._run(convergence=convergence)

    def test_generated_adapter_drift_is_a_hard_failure(self) -> None:
        with (
            mock.patch.object(admission, "evaluate_source_gate", return_value=self.source_report),
            mock.patch.object(admission, "crosscheck_capture", return_value=self.convergence),
            mock.patch.object(
                admission,
                "generate_protocol",
                side_effect=CodegenError("generated Rust output drifted"),
            ),
            self.assertRaisesRegex(CodegenError, "drifted"),
        ):
            admission.admit_r0_session(
                db_path=self.db,
                source_gate_path=self.gate,
                contract_path=self.contract_path,
                capture_path=self.capture,
                generated_rust_path=self.generated,
                lock_path=self.lock,
                records_root=self.records,
            )

    def test_source_report_required_method_metadata_is_fail_closed(self) -> None:
        for field, value in [
            ("record_sha256", "bad"),
            ("normalized_sha256", "bad"),
            ("body_sha256", "bad"),
            ("semantic_rules", []),
        ]:
            with self.subTest(field=field):
                source = copy.deepcopy(self.source_report)
                source["required_methods"][0][field] = value
                with self.assertRaises(admission.R0AdmissionError):
                    self._run(source_report=source)

    def test_atlas_database_must_be_a_real_non_symlink_file(self) -> None:
        missing = self.root / "missing.sqlite"
        with self.assertRaisesRegex(admission.R0AdmissionError, "real non-symlink"):
            admission.admit_r0_session(
                db_path=missing,
                source_gate_path=self.gate,
                contract_path=self.contract_path,
                capture_path=self.capture,
                generated_rust_path=self.generated,
                lock_path=self.lock,
                records_root=self.records,
            )

        link = self.root / "atlas-link.sqlite"
        try:
            link.symlink_to(self.db)
        except (OSError, NotImplementedError):
            return
        with self.assertRaisesRegex(admission.R0AdmissionError, "real non-symlink"):
            admission.admit_r0_session(
                db_path=link,
                source_gate_path=self.gate,
                contract_path=self.contract_path,
                capture_path=self.capture,
                generated_rust_path=self.generated,
                lock_path=self.lock,
                records_root=self.records,
            )


if __name__ == "__main__":
    unittest.main()
