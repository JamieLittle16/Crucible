import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools.protocol_capture_admission import crosscheck_capture
from tools.protocol_codegen import generate
from tools.r0_status_contract import materialize

ROOT = Path(__file__).resolve().parents[2]
LOCK = ROOT / "vanilla" / "vanilla.lock.toml"
RECORDS = ROOT / "vanilla" / "records"
CAPTURE = ROOT / "vanilla" / "fixtures" / "protocol" / "26.2-status-capture.json"
CONTRACT = ROOT / "vanilla" / "protocol" / "PROTO-NET-STATUS-26-2-001.json"
ADMISSION = ROOT / "vanilla" / "reports" / "r0-status-admission-26.2.json"

EXPECTED_CAPTURE_SHA256 = "63635b9176d465bcb92414b091dcd231a8bef25dc001d88f80190a85e7cf3132"
EXPECTED_SESSION_SHA256 = "fb57c003d0e96c467dad55c209237dd23478ff287caea51943823cc62848cea0"
EXPECTED_GENERATED_SHA256 = "77aec1160385078ffe8757c362196b41b4801433088d06e3d9c68207c2efecf8"


class RealR0EvidenceFixtureTests(unittest.TestCase):
    def test_real_26_2_oracle_fixture_reproduces_the_sealed_r0_evidence(self) -> None:
        capture = json.loads(CAPTURE.read_text(encoding="utf-8"))
        contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
        admission = json.loads(ADMISSION.read_text(encoding="utf-8"))

        self.assertEqual(capture["capture_sha256"], EXPECTED_CAPTURE_SHA256)
        self.assertEqual(admission["capture"]["sha256"], EXPECTED_CAPTURE_SHA256)
        self.assertEqual(admission["session_sha256"], EXPECTED_SESSION_SHA256)
        self.assertEqual(admission["generated_rust"]["sha256"], EXPECTED_GENERATED_SHA256)
        self.assertEqual(contract["id"], "PROTO-NET-STATUS-26-2-001")
        self.assertEqual(admission["contract"]["id"], contract["id"])

        cited_records = sorted(
            {
                record
                for packet in contract["packets"]
                for record in packet["source_records"]
            }
        )
        self.assertEqual(cited_records, admission["contract"]["source_records"])

        unsigned = dict(admission)
        declared_session_sha256 = unsigned.pop("session_sha256")
        canonical = json.dumps(
            unsigned,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=True,
        ).encode("ascii")
        self.assertEqual(hashlib.sha256(canonical).hexdigest(), declared_session_sha256)

        with tempfile.TemporaryDirectory() as temporary:
            temporary_root = Path(temporary)
            materialized = temporary_root / "PROTO-NET-STATUS-26-2-001.json"
            result = materialize(
                CAPTURE,
                lock_path=LOCK,
                records_root=RECORDS,
                output_path=materialized,
            )
            self.assertEqual(result["contract"]["id"], contract["id"])
            self.assertEqual(result["convergence"]["frames_matched"], 5)
            self.assertEqual(materialized.read_bytes(), CONTRACT.read_bytes())

            convergence = crosscheck_capture(
                CONTRACT,
                CAPTURE,
                lock_path=LOCK,
                records_root=RECORDS,
            )
            self.assertEqual(convergence["capture_sha256"], EXPECTED_CAPTURE_SHA256)
            self.assertEqual(convergence["client_to_server_frames"], 3)
            self.assertEqual(convergence["server_to_client_frames"], 2)
            self.assertEqual(convergence["frames_matched"], 5)

            generated = temporary_root / "status_26_2.rs"
            rendered = generate(
                CONTRACT,
                lock_path=LOCK,
                records_root=RECORDS,
                output_path=generated,
                check=False,
            )
            generated_sha256 = hashlib.sha256(rendered.encode("utf-8")).hexdigest()
            self.assertEqual(generated_sha256, EXPECTED_GENERATED_SHA256)
            self.assertEqual(generated_sha256, admission["generated_rust"]["sha256"])


if __name__ == "__main__":
    unittest.main()
