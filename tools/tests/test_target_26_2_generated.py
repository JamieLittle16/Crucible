from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools.protocol_codegen import generate

REPO_ROOT = Path(__file__).resolve().parents[2]
LOCK = REPO_ROOT / "vanilla/vanilla.lock.toml"
RECORDS = REPO_ROOT / "vanilla/records"

STATUS_CONTRACT = REPO_ROOT / "vanilla/protocol/PROTO-NET-STATUS-26-2-001.json"
STATUS_ADMISSION = REPO_ROOT / "vanilla/reports/r0-status-admission-26.2.json"
STATUS_COMMITTED = (
    REPO_ROOT
    / "crates/network/crucible-target-26-2/src/generated/status_26_2.rs"
)
EXPECTED_STATUS_GENERATED_SHA256 = (
    "77aec1160385078ffe8757c362196b41b4801433088d06e3d9c68207c2efecf8"
)

LOGIN_CONTRACT = REPO_ROOT / "vanilla/protocol/PROTO-NET-LOGIN-26-2-001.json"
LOGIN_COMMITTED = (
    REPO_ROOT
    / "crates/network/crucible-target-26-2/src/generated/login_26_2.rs"
)
EXPECTED_LOGIN_GENERATED_SHA256 = (
    "c20ee1905265502380af13bb5396acc16144ca42923dd3dc54d640dd391afd29"
)

PLAY_LIVENESS_CONTRACT = (
    REPO_ROOT / "vanilla/protocol/PROTO-NET-PLAY-LIVENESS-26-2-001.json"
)
PLAY_LIVENESS_COMMITTED = (
    REPO_ROOT
    / "crates/network/crucible-target-26-2/src/generated/play_liveness_26_2.rs"
)
EXPECTED_PLAY_LIVENESS_GENERATED_SHA256 = (
    "4db12fd5a859539eb019033a21efdb4ca8f3b2e39f61175fda65ace43935d6b0"
)


class Target26_2GeneratedTests(unittest.TestCase):
    def test_committed_status_packet_facts_are_exact_admitted_codegen(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "status_26_2.rs"
            generate(
                STATUS_CONTRACT,
                lock_path=LOCK,
                records_root=RECORDS,
                output_path=output,
                check=False,
            )
            expected = output.read_bytes()

        committed = STATUS_COMMITTED.read_bytes()
        self.assertEqual(committed, expected)
        digest = hashlib.sha256(committed).hexdigest()
        self.assertEqual(digest, EXPECTED_STATUS_GENERATED_SHA256)

        admission = json.loads(STATUS_ADMISSION.read_text(encoding="utf-8"))
        self.assertEqual(admission["generated_rust"]["sha256"], digest)
        self.assertEqual(
            admission["session_sha256"],
            "fb57c003d0e96c467dad55c209237dd23478ff287caea51943823cc62848cea0",
        )

    def test_committed_login_packet_facts_are_exact_admitted_codegen(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "login_26_2.rs"
            generate(
                LOGIN_CONTRACT,
                lock_path=LOCK,
                records_root=RECORDS,
                output_path=output,
                check=False,
            )
            expected = output.read_bytes()

        committed = LOGIN_COMMITTED.read_bytes()
        self.assertEqual(committed, expected)
        self.assertEqual(
            hashlib.sha256(committed).hexdigest(),
            EXPECTED_LOGIN_GENERATED_SHA256,
        )

    def test_committed_play_liveness_facts_are_exact_admitted_codegen(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "play_liveness_26_2.rs"
            generate(
                PLAY_LIVENESS_CONTRACT,
                lock_path=LOCK,
                records_root=RECORDS,
                output_path=output,
                check=False,
            )
            expected = output.read_bytes()

        committed = PLAY_LIVENESS_COMMITTED.read_bytes()
        self.assertEqual(committed, expected)
        self.assertEqual(
            hashlib.sha256(committed).hexdigest(),
            EXPECTED_PLAY_LIVENESS_GENERATED_SHA256,
        )


if __name__ == "__main__":
    unittest.main()
