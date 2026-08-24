from __future__ import annotations

import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools import r0_external_probe_admission as probe
from tools.protocol_capture_admission import EvidenceConvergenceError

COMMIT = "a" * 40
SESSION = "b" * 64
CAPTURE = "c" * 64


class FakeRunner:
    def __init__(self, repo: Path, *, dirty: bool = False) -> None:
        self.repo = repo
        self.dirty = dirty

    def __call__(
        self, argv: list[str] | tuple[str, ...], cwd: Path
    ) -> subprocess.CompletedProcess[str]:
        args = list(argv)
        if cwd.resolve() != self.repo.resolve():
            return subprocess.CompletedProcess(args, 1, "", "wrong cwd")
        if args == ["git", "rev-parse", "--show-toplevel"]:
            return subprocess.CompletedProcess(args, 0, f"{self.repo}\n", "")
        if args == ["git", "status", "--porcelain", "--untracked-files=all"]:
            status = "?? dirty.txt\n" if self.dirty else ""
            return subprocess.CompletedProcess(args, 0, status, "")
        if args == ["git", "rev-parse", "HEAD"]:
            return subprocess.CompletedProcess(args, 0, f"{COMMIT}\n", "")
        return subprocess.CompletedProcess(args, 1, "", "unexpected command")


def convergence(**_: object) -> dict[str, object]:
    raise AssertionError("keyword-only fake should not be called this way")


class R0ExternalProbeAdmissionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name) / "repo"
        self.repo.mkdir()

        server = self.repo / "crates/crucible-server/src"
        server.mkdir(parents=True)
        (server / "lib.rs").write_text(
            "pub const R0_ADMISSION_SESSION_SHA256: &str =\n"
            f'    "{SESSION}";\n',
            encoding="utf-8",
        )

        generated = self.repo / "crates/network/crucible-target-26-2/src/generated"
        generated.mkdir(parents=True)
        self.generated_bytes = b"generated target facts\n"
        (generated / "status_26_2.rs").write_bytes(self.generated_bytes)
        self.generated_sha = hashlib.sha256(self.generated_bytes).hexdigest()

        self.admission = Path(self.temp.name) / "admission.json"
        self.admission.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "kind": "r0-status-admission-v1",
                    "session_sha256": SESSION,
                    "contract": {"id": probe.EXPECTED_CONTRACT_ID},
                    "generated_rust": {"sha256": self.generated_sha},
                }
            ),
            encoding="utf-8",
        )

        self.observation = Path(self.temp.name) / "observation.json"
        self.observation_payload: dict[str, object] = {
            "schema": 1,
            "kind": probe.OBSERVATION_KIND,
            "minecraft": probe.EXPECTED_MINECRAFT,
            "client_distribution": probe.EXPECTED_CLIENT_DISTRIBUTION,
            "modified": False,
            "endpoint": probe.EXPECTED_ENDPOINT,
            "server_list_visible": True,
            "status_rendered": True,
            "ping_completed_without_protocol_error": True,
        }
        self._write_observation()

        self.ui = Path(self.temp.name) / "ui.png"
        self.ui.write_bytes(b"synthetic-ui-evidence")
        self.contract = Path(self.temp.name) / "contract.json"
        self.capture = Path(self.temp.name) / "capture.json"
        self.lock = Path(self.temp.name) / "lock.toml"
        self.records = Path(self.temp.name) / "records"

    def _write_observation(self) -> None:
        self.observation.write_text(
            json.dumps(self.observation_payload, sort_keys=True), encoding="utf-8"
        )

    @staticmethod
    def _convergence(
        contract_path: Path,
        capture_path: Path,
        *,
        lock_path: Path,
        records_root: Path,
    ) -> dict[str, object]:
        del contract_path, capture_path, lock_path, records_root
        return {
            "schema": 1,
            "contract_id": probe.EXPECTED_CONTRACT_ID,
            "capture_sha256": CAPTURE,
            "minecraft": probe.EXPECTED_MINECRAFT,
            "protocol": probe.EXPECTED_PROTOCOL,
            "client_to_server_frames": probe.EXPECTED_CLIENT_TO_SERVER_FRAMES,
            "server_to_client_frames": probe.EXPECTED_SERVER_TO_CLIENT_FRAMES,
            "frames_matched": probe.EXPECTED_MATCHED_FRAMES,
        }

    def admit(self, **overrides: object) -> dict[str, object]:
        arguments: dict[str, object] = {
            "repo_root": self.repo,
            "contract_path": self.contract,
            "capture_path": self.capture,
            "observation_path": self.observation,
            "ui_evidence_path": self.ui,
            "admission_path": self.admission,
            "lock_path": self.lock,
            "records_root": self.records,
            "runner": FakeRunner(self.repo),
            "convergence": self._convergence,
        }
        arguments.update(overrides)
        return probe.admit_external_probe(**arguments)

    def test_valid_probe_binds_commit_capture_ui_and_admission_deterministically(self) -> None:
        first = self.admit()
        second = self.admit()
        self.assertEqual(first, second)
        self.assertTrue(first["admitted"])
        self.assertEqual(first["server_commit"], COMMIT)
        self.assertEqual(first["admission_session_sha256"], SESSION)
        self.assertEqual(first["generated_rust_sha256"], self.generated_sha)
        self.assertEqual(first["capture_sha256"], CAPTURE)
        self.assertEqual(first["frames_matched"], 5)
        self.assertEqual(first["ui_evidence"]["bytes"], len(b"synthetic-ui-evidence"))
        self.assertEqual(
            first["ui_evidence"]["sha256"], hashlib.sha256(b"synthetic-ui-evidence").hexdigest()
        )
        identity = dict(first)
        digest = identity.pop("report_sha256")
        self.assertEqual(digest, hashlib.sha256(probe._canonical_bytes(identity)).hexdigest())

    def test_dirty_checkout_is_rejected_before_evidence_is_admitted(self) -> None:
        with self.assertRaisesRegex(probe.ExternalProbeError, "clean worktree"):
            self.admit(runner=FakeRunner(self.repo, dirty=True))

    def test_operator_observation_must_be_exact_and_fail_closed(self) -> None:
        mutations = (
            ("modified", True),
            ("server_list_visible", False),
            ("status_rendered", False),
            ("ping_completed_without_protocol_error", False),
            ("server_list_visible", 1),
            ("client_distribution", "modded"),
            ("endpoint", "127.0.0.1:25565"),
            ("minecraft", "26.1"),
        )
        for field, value in mutations:
            with self.subTest(field=field, value=value):
                original = self.observation_payload[field]
                self.observation_payload[field] = value
                self._write_observation()
                with self.assertRaises(probe.ExternalProbeError):
                    self.admit()
                self.observation_payload[field] = original
                self._write_observation()

    def test_server_session_must_match_sealed_admission(self) -> None:
        path = self.repo / "crates/crucible-server/src/lib.rs"
        path.write_text(
            "pub const R0_ADMISSION_SESSION_SHA256: &str =\n"
            f'    "{"d" * 64}";\n',
            encoding="utf-8",
        )
        with self.assertRaisesRegex(probe.ExternalProbeError, "server R0 session"):
            self.admit()

    def test_generated_target_bytes_must_match_sealed_admission(self) -> None:
        path = self.repo / "crates/network/crucible-target-26-2/src/generated/status_26_2.rs"
        path.write_bytes(b"drifted generated target\n")
        with self.assertRaisesRegex(probe.ExternalProbeError, "generated 26.2 packet facts"):
            self.admit()

    def test_capture_convergence_must_be_exact_five_frame_r0_exchange(self) -> None:
        def wrong_count(
            contract_path: Path,
            capture_path: Path,
            *,
            lock_path: Path,
            records_root: Path,
        ) -> dict[str, object]:
            result = self._convergence(
                contract_path,
                capture_path,
                lock_path=lock_path,
                records_root=records_root,
            )
            result["frames_matched"] = 4
            return result

        with self.assertRaisesRegex(probe.ExternalProbeError, "frames_matched mismatch"):
            self.admit(convergence=wrong_count)

    def test_capture_gate_failure_is_preserved_as_external_probe_failure(self) -> None:
        def rejected(
            contract_path: Path,
            capture_path: Path,
            *,
            lock_path: Path,
            records_root: Path,
        ) -> dict[str, object]:
            del contract_path, capture_path, lock_path, records_root
            raise EvidenceConvergenceError("wrong byte")

        with self.assertRaisesRegex(probe.ExternalProbeError, "wrong byte"):
            self.admit(convergence=rejected)

    def test_ui_evidence_is_required_real_bounded_content(self) -> None:
        self.ui.write_bytes(b"")
        with self.assertRaisesRegex(probe.ExternalProbeError, "UI evidence must contain"):
            self.admit()


if __name__ == "__main__":
    unittest.main()
