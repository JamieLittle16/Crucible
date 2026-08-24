import json
import tempfile
import tomllib
import unittest
from pathlib import Path

from tools.protocol_capture_admission import EvidenceConvergenceError
from tools.protocol_capture_proxy import FrameStreamCapture, build_artifact, render_artifact
from tools.r0_status_contract import R0MaterializationError, materialize

ROOT = Path(__file__).resolve().parents[2]
LOCK = ROOT / "vanilla" / "vanilla.lock.toml"
RECORDS = ROOT / "vanilla" / "records"


def encode_var_int(value: int) -> bytes:
    result = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            result.append(byte | 0x80)
        else:
            result.append(byte)
            return bytes(result)


def encode_string(value: str) -> bytes:
    raw = value.encode("utf-8")
    return encode_var_int(len(raw)) + raw


def frame(body: bytes) -> bytes:
    return encode_var_int(len(body)) + body


def target() -> dict[str, object]:
    lock = tomllib.loads(LOCK.read_text(encoding="utf-8"))
    return {
        "minecraft": lock["minecraft"],
        "protocol": lock["protocol"],
        "source_archive_sha256": lock["source"]["archive_sha256"],
        "fingerprint_algorithm": lock["atlas"]["fingerprint_algorithm"],
    }


def status_json(protocol: int = 776) -> str:
    return json.dumps(
        {
            "description": {"text": "Crucible R0 Oracle"},
            "players": {"max": 20, "online": 0},
            "version": {"name": "26.2", "protocol": protocol},
            "enforcesSecureChat": False,
        },
        separators=(",", ":"),
        ensure_ascii=True,
    )


def capture_artifact(
    *, intent: int = 1, status_protocol: int = 776, pong_echo: bool = True
) -> dict:
    handshake = (
        encode_var_int(0)
        + encode_var_int(776)
        + encode_string("127.0.0.1")
        + (25566).to_bytes(2, "big")
        + encode_var_int(intent)
    )
    status_request = encode_var_int(0)
    ping_payload = bytes.fromhex("0102030405060708")
    ping_request = encode_var_int(1) + ping_payload
    status_response = encode_var_int(0) + encode_string(status_json(status_protocol))
    pong_payload = ping_payload if pong_echo else bytes.fromhex("1112131415161718")
    pong_response = encode_var_int(1) + pong_payload

    c2s = FrameStreamCapture(
        max_frame_bytes=1 << 20, max_stream_bytes=8 << 20, max_frames=16
    )
    s2c = FrameStreamCapture(
        max_frame_bytes=1 << 20, max_stream_bytes=8 << 20, max_frames=16
    )
    c2s.feed(frame(handshake) + frame(status_request) + frame(ping_request))
    s2c.feed(frame(status_response) + frame(pong_response))
    return build_artifact(target=target(), client_to_server=c2s, server_to_client=s2c)


class R0StatusContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.capture = self.root / "capture.json"
        self.output = self.root / "contract.json"

    def tearDown(self) -> None:
        self.temp.cleanup()

    def write_capture(self, artifact: dict) -> None:
        self.capture.write_text(render_artifact(artifact), encoding="utf-8")

    def materialize(self) -> dict[str, object]:
        return materialize(
            self.capture,
            lock_path=LOCK,
            records_root=RECORDS,
            output_path=self.output,
        )

    def test_valid_witness_materializes_and_converges_five_packet_contract(self) -> None:
        self.write_capture(capture_artifact())
        result = self.materialize()
        self.assertEqual(result["contract"]["id"], "PROTO-NET-STATUS-26-2-001")
        self.assertEqual(result["contract"]["packets"], 5)
        self.assertEqual(result["convergence"]["client_to_server_frames"], 3)
        self.assertEqual(result["convergence"]["server_to_client_frames"], 2)
        self.assertEqual(result["convergence"]["frames_matched"], 5)

        contract = json.loads(self.output.read_text(encoding="utf-8"))
        identities = [
            (packet["phase"], packet["direction"], packet["id"])
            for packet in contract["packets"]
        ]
        self.assertEqual(
            identities,
            [
                ("handshake", "serverbound", 0),
                ("status", "serverbound", 0),
                ("status", "serverbound", 1),
                ("status", "clientbound", 0),
                ("status", "clientbound", 1),
            ],
        )

    def test_wrong_status_intent_fails_before_output(self) -> None:
        self.write_capture(capture_artifact(intent=2))
        with self.assertRaisesRegex(R0MaterializationError, "R0 status intent"):
            self.materialize()
        self.assertFalse(self.output.exists())

    def test_wrong_reported_server_protocol_fails_before_output(self) -> None:
        self.write_capture(capture_artifact(status_protocol=775))
        with self.assertRaisesRegex(R0MaterializationError, "version.protocol"):
            self.materialize()
        self.assertFalse(self.output.exists())

    def test_non_echo_pong_fails_even_when_capture_is_internally_self_consistent(self) -> None:
        self.write_capture(capture_artifact(pong_echo=False))
        with self.assertRaisesRegex(R0MaterializationError, "exact 64-bit echo"):
            self.materialize()
        self.assertFalse(self.output.exists())

    def test_capture_digest_tampering_is_rejected_by_existing_p0l_gate(self) -> None:
        artifact = capture_artifact()
        artifact["capture_sha256"] = "0" * 64
        self.write_capture(artifact)
        with self.assertRaisesRegex(EvidenceConvergenceError, "capture_sha256"):
            self.materialize()
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
