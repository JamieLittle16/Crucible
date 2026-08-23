import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools.protocol_capture_admission import EvidenceConvergenceError, crosscheck_capture
from tools.protocol_capture_proxy import FrameStreamCapture, _read_target, build_artifact


LOCK_TEXT = """schema = 1
minecraft = "test-version"
protocol = 42
data_version = 7

[source]
kind = "test-source"
archive_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
java_files = 1

[atlas]
schema = 1
version = "test"
fingerprint_algorithm = "test-fingerprint-v1"
database = ".test/atlas.sqlite"
"""


def encode_var_int(value: int) -> bytes:
    encoded = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            encoded.append(byte | 0x80)
        else:
            encoded.append(byte)
            return bytes(encoded)


def frame(body: bytes) -> bytes:
    return encode_var_int(len(body)) + body


def capture_stream(frames: list[bytes]) -> FrameStreamCapture:
    capture = FrameStreamCapture(
        max_frame_bytes=4_096,
        max_stream_bytes=65_536,
        max_frames=32,
    )
    stream = b"".join(frames)
    for byte in stream:
        capture.feed(bytes([byte]))
    capture.finish()
    return capture


def canonical_capture_digest(artifact: dict) -> str:
    identity = copy.deepcopy(artifact)
    identity.pop("capture_sha256", None)
    canonical = json.dumps(
        identity, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("ascii")
    return hashlib.sha256(canonical).hexdigest()


class ProtocolCaptureAdmissionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.lock = self.root / "vanilla.lock.toml"
        self.lock.write_text(LOCK_TEXT, encoding="utf-8")
        self.records = self.root / "records"
        self.records.mkdir()
        self.record = {
            "schema": 1,
            "id": "VAR-PROTOCOL-STATUS-001",
            "status": "VAR_REVIEWED",
            "source": {
                "type": "net.minecraft.test.StatusPath",
                "signature": "status()",
                "fingerprint_algorithm": "test-fingerprint-v1",
                "normalized_sha256": "b" * 64,
                "body_sha256": "c" * 64,
            },
            "classifications": ["SEMANTIC_NETWORK"],
            "hazards_reviewed": [],
            "semantic_rules": [
                "SEM-PROTOCOL-HANDSHAKE-001",
                "SEM-PROTOCOL-STATUS-001",
                "SEM-PROTOCOL-PING-001",
            ],
            "evidence": [],
            "notes": [],
        }
        (self.records / "VAR-PROTOCOL-STATUS-001.json").write_text(
            json.dumps(self.record, separators=(",", ":")), encoding="utf-8"
        )

        self.packet_specs = [
            ("handshake", "handshake", "serverbound", 0, b"\x2aexample"),
            ("status-request", "status", "serverbound", 0, b""),
            ("status-response", "status", "clientbound", 0, b'{"ok":true}'),
            ("ping-request", "status", "serverbound", 1, b"12345678"),
            ("ping-response", "status", "clientbound", 1, b"12345678"),
        ]
        self.contract = self._make_contract()
        self.contract_path = self.root / "contract.json"
        self.capture_path = self.root / "capture.json"
        self._write_contract()
        self.artifact = self._make_capture()
        self._write_capture()

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _packet(self, name: str, phase: str, direction: str, packet_id: int, payload: bytes) -> dict:
        body = encode_var_int(packet_id) + payload
        semantic_rule = {
            "handshake": "SEM-PROTOCOL-HANDSHAKE-001",
            "status-request": "SEM-PROTOCOL-STATUS-001",
            "status-response": "SEM-PROTOCOL-STATUS-001",
            "ping-request": "SEM-PROTOCOL-PING-001",
            "ping-response": "SEM-PROTOCOL-PING-001",
        }[name]
        return {
            "name": name,
            "phase": phase,
            "direction": direction,
            "id": packet_id,
            "semantic_rules": [semantic_rule],
            "source_records": ["VAR-PROTOCOL-STATUS-001"],
            "golden": {
                "body_hex": body.hex(),
                "frame_hex": frame(body).hex(),
            },
        }

    def _make_contract(self) -> dict:
        return {
            "schema": 1,
            "id": "PROTO-TEST-STATUS-001",
            "target": {
                "minecraft": "test-version",
                "protocol": 42,
                "source_archive_sha256": "a" * 64,
                "fingerprint_algorithm": "test-fingerprint-v1",
            },
            "packets": [self._packet(*spec) for spec in self.packet_specs],
        }

    def _direction_frames(self, direction: str) -> list[bytes]:
        contract_direction = {
            "client-to-server": "serverbound",
            "server-to-client": "clientbound",
        }[direction]
        return [
            bytes.fromhex(packet["golden"]["frame_hex"])
            for packet in self.contract["packets"]
            if packet["direction"] == contract_direction
        ]

    def _make_capture(
        self,
        *,
        client_to_server: list[bytes] | None = None,
        server_to_client: list[bytes] | None = None,
        target: dict | None = None,
    ) -> dict:
        c2s = self._direction_frames("client-to-server") if client_to_server is None else client_to_server
        s2c = self._direction_frames("server-to-client") if server_to_client is None else server_to_client
        return build_artifact(
            target=_read_target(self.lock) if target is None else target,
            client_to_server=capture_stream(c2s),
            server_to_client=capture_stream(s2c),
        )

    def _write_contract(self, value: dict | None = None) -> None:
        self.contract_path.write_text(
            json.dumps(self.contract if value is None else value, separators=(",", ":")),
            encoding="utf-8",
        )

    def _write_capture(self, value: dict | None = None) -> None:
        self.capture_path.write_text(
            json.dumps(self.artifact if value is None else value, separators=(",", ":")),
            encoding="utf-8",
        )

    def _crosscheck(self) -> dict[str, object]:
        return crosscheck_capture(
            self.contract_path,
            self.capture_path,
            lock_path=self.lock,
            records_root=self.records,
        )

    def test_valid_source_contract_and_capture_converge(self) -> None:
        summary = self._crosscheck()
        self.assertEqual(summary["schema"], 1)
        self.assertEqual(summary["contract_id"], "PROTO-TEST-STATUS-001")
        self.assertEqual(summary["minecraft"], "test-version")
        self.assertEqual(summary["protocol"], 42)
        self.assertEqual(summary["client_to_server_frames"], 3)
        self.assertEqual(summary["server_to_client_frames"], 2)
        self.assertEqual(summary["frames_matched"], 5)
        self.assertEqual(summary["capture_sha256"], self.artifact["capture_sha256"])
        self.assertNotIn("packets", summary)
        self.assertNotIn("packet_ids", summary)

    def test_capture_top_level_stream_and_frame_unknown_fields_fail_closed(self) -> None:
        mutations = [
            (("extra",), True),
            (("streams", 0, "extra"), True),
            (("streams", 0, "frames", 0, "extra"), True),
        ]
        for path, value in mutations:
            with self.subTest(path=path):
                artifact = copy.deepcopy(self.artifact)
                cursor = artifact
                for key in path[:-1]:
                    cursor = cursor[key]
                cursor[path[-1]] = value
                artifact["capture_sha256"] = canonical_capture_digest(artifact)
                self._write_capture(artifact)
                with self.assertRaisesRegex(EvidenceConvergenceError, "unknown keys"):
                    self._crosscheck()

    def test_capture_digest_detects_unreconciled_tampering(self) -> None:
        artifact = copy.deepcopy(self.artifact)
        artifact["limits"]["max_stream_bytes"] += 1
        self._write_capture(artifact)
        with self.assertRaisesRegex(EvidenceConvergenceError, "capture_sha256"):
            self._crosscheck()

    def test_self_consistent_wrong_target_still_fails_convergence(self) -> None:
        target = copy.deepcopy(self.artifact["target"])
        target["protocol"] = 43
        artifact = self._make_capture(target=target)
        self._write_capture(artifact)
        with self.assertRaisesRegex(EvidenceConvergenceError, "target does not match"):
            self._crosscheck()

    def test_boolean_cannot_masquerade_as_integer_capture_fields(self) -> None:
        mutations = [
            (("schema",), True),
            (("target", "protocol"), True),
            (("limits", "max_frames_per_direction"), True),
            (("streams", 0, "frame_count"), True),
            (("streams", 0, "frames", 0, "ordinal"), True),
        ]
        for path, value in mutations:
            with self.subTest(path=path):
                artifact = copy.deepcopy(self.artifact)
                cursor = artifact
                for key in path[:-1]:
                    cursor = cursor[key]
                cursor[path[-1]] = value
                artifact["capture_sha256"] = canonical_capture_digest(artifact)
                self._write_capture(artifact)
                with self.assertRaises(EvidenceConvergenceError):
                    self._crosscheck()

    def test_reordered_capture_with_all_metadata_recomputed_is_rejected(self) -> None:
        frames = self._direction_frames("client-to-server")
        artifact = self._make_capture(client_to_server=[frames[1], frames[0], frames[2]])
        self._write_capture(artifact)
        with self.assertRaisesRegex(EvidenceConvergenceError, "golden frame"):
            self._crosscheck()

    def test_extra_and_missing_frames_are_rejected(self) -> None:
        frames = self._direction_frames("client-to-server")
        variants = [frames[:-1], frames + [frames[-1]]]
        for candidate in variants:
            with self.subTest(frame_count=len(candidate)):
                artifact = self._make_capture(client_to_server=candidate)
                self._write_capture(artifact)
                with self.assertRaisesRegex(EvidenceConvergenceError, "frame count differs"):
                    self._crosscheck()

    def test_altered_valid_frame_with_recomputed_hashes_is_rejected(self) -> None:
        frames = self._direction_frames("server-to-client")
        body = bytes.fromhex(self.contract["packets"][2]["golden"]["body_hex"])
        altered_body = body[:-1] + bytes([body[-1] ^ 1])
        altered = frame(altered_body)
        artifact = self._make_capture(server_to_client=[altered, frames[1]])
        self._write_capture(artifact)
        with self.assertRaisesRegex(EvidenceConvergenceError, "golden frame"):
            self._crosscheck()

    def test_frame_metadata_and_stream_metadata_are_independently_revalidated(self) -> None:
        cases = [
            (0, "stream_bytes", 1, "stream_bytes"),
            (0, "stream_sha256", "d" * 64, "stream_sha256"),
            (0, "frame_count", 99, "frame_count"),
            (0, ("frames", 0, "stream_offset"), 1, "stream_offset"),
            (0, ("frames", 0, "frame_bytes"), 999, "frame_bytes"),
            (0, ("frames", 0, "body_bytes"), 999, "body_bytes"),
            (0, ("frames", 0, "frame_sha256"), "e" * 64, "frame_sha256"),
        ]
        for stream_index, field, value, expected in cases:
            with self.subTest(field=field):
                artifact = copy.deepcopy(self.artifact)
                if isinstance(field, tuple):
                    cursor = artifact["streams"][stream_index]
                    for key in field[:-1]:
                        cursor = cursor[key]
                    cursor[field[-1]] = value
                else:
                    artifact["streams"][stream_index][field] = value
                artifact["capture_sha256"] = canonical_capture_digest(artifact)
                self._write_capture(artifact)
                with self.assertRaisesRegex(EvidenceConvergenceError, expected):
                    self._crosscheck()

    def test_noncanonical_frame_prefix_is_rejected_even_with_consistent_metadata(self) -> None:
        artifact = copy.deepcopy(self.artifact)
        captured = artifact["streams"][0]["frames"][0]
        raw = bytes.fromhex(captured["frame_hex"])
        body = bytes.fromhex(captured["body_hex"])
        noncanonical = b"\x80\x00" + body
        captured["frame_hex"] = noncanonical.hex()
        captured["frame_bytes"] = len(noncanonical)
        captured["frame_sha256"] = hashlib.sha256(noncanonical).hexdigest()
        # Make the enclosing stream self-consistent so the frame parser is the rejecting boundary.
        stream = artifact["streams"][0]
        stream_bytes = [bytes.fromhex(item["frame_hex"]) for item in stream["frames"]]
        offset = 0
        for item, raw_frame in zip(stream["frames"], stream_bytes, strict=True):
            item["stream_offset"] = offset
            offset += len(raw_frame)
        image = b"".join(stream_bytes)
        stream["stream_bytes"] = len(image)
        stream["stream_sha256"] = hashlib.sha256(image).hexdigest()
        artifact["capture_sha256"] = canonical_capture_digest(artifact)
        self._write_capture(artifact)
        with self.assertRaisesRegex(EvidenceConvergenceError, "noncanonical VarInt"):
            self._crosscheck()

    def test_contract_must_still_pass_source_admission(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["packets"][0]["id"] = 99
        self._write_contract(contract)
        with self.assertRaisesRegex(EvidenceConvergenceError, "failed source admission"):
            self._crosscheck()

    def test_non_r0_contract_phase_is_rejected_even_if_capture_bytes_match(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["packets"][1]["phase"] = "login"
        self._write_contract(contract)
        # The generic protocol-contract firewall accepts login as a valid phase. P0L must not,
        # because the P0K plaintext capture boundary is intentionally narrower.
        with self.assertRaisesRegex(EvidenceConvergenceError, "rejects non-R0 phase"):
            self._crosscheck()

    def test_duplicate_or_missing_capture_directions_fail_closed(self) -> None:
        artifact = copy.deepcopy(self.artifact)
        artifact["streams"][1]["direction"] = "client-to-server"
        artifact["capture_sha256"] = canonical_capture_digest(artifact)
        self._write_capture(artifact)
        with self.assertRaisesRegex(EvidenceConvergenceError, "duplicate stream direction"):
            self._crosscheck()

        artifact = copy.deepcopy(self.artifact)
        artifact["streams"].pop()
        artifact["capture_sha256"] = canonical_capture_digest(artifact)
        self._write_capture(artifact)
        with self.assertRaisesRegex(EvidenceConvergenceError, "exactly two"):
            self._crosscheck()

    def test_capture_and_contract_files_must_not_be_symlinks(self) -> None:
        capture_link = self.root / "capture-link.json"
        try:
            capture_link.symlink_to(self.capture_path)
        except (OSError, NotImplementedError):
            self.skipTest("symlinks unavailable")
        with self.assertRaisesRegex(EvidenceConvergenceError, "non-symlink"):
            crosscheck_capture(
                self.contract_path,
                capture_link,
                lock_path=self.lock,
                records_root=self.records,
            )


if __name__ == "__main__":
    unittest.main()
