from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r1_login_contract as login

ROOT = Path(__file__).resolve().parents[2]
LOCK = ROOT / "vanilla" / "vanilla.lock.toml"
RECORDS = ROOT / "vanilla" / "records"
RUNTIME_FIXTURE = ROOT / "vanilla" / "fixtures" / "runtime" / "java25-name-uuid-v3.json"


def varint(value: int) -> bytes:
    out = bytearray()
    remaining = value
    while True:
        byte = remaining & 0x7F
        remaining >>= 7
        if remaining:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def string(value: str) -> bytes:
    raw = value.encode("utf-8")
    return varint(len(raw)) + raw


def frame(body: bytes) -> bytes:
    return varint(len(body)) + body


def stream(direction: str, bodies: list[bytes]) -> dict[str, object]:
    frames: list[dict[str, object]] = []
    image = bytearray()
    offset = 0
    for ordinal, body in enumerate(bodies):
        raw = frame(body)
        frames.append(
            {
                "ordinal": ordinal,
                "stream_offset": offset,
                "frame_bytes": len(raw),
                "body_bytes": len(body),
                "frame_sha256": hashlib.sha256(raw).hexdigest(),
                "frame_hex": raw.hex(),
                "body_hex": body.hex(),
            }
        )
        image.extend(raw)
        offset += len(raw)
    return {
        "direction": direction,
        "stream_bytes": len(image),
        "stream_sha256": hashlib.sha256(image).hexdigest(),
        "frame_count": len(frames),
        "frames": frames,
    }


def capture(
    *,
    name: str = "CrucibleR1",
    intent: int = 2,
    profile_uuid: bytes | None = None,
    property_count: int = 0,
    ack_payload: bytes = b"",
    hello_tail: bytes = b"",
    include_configuration_tail: bool = True,
) -> dict[str, object]:
    target = login._read_target(LOCK)
    offline_uuid = login.offline_player_uuid(name) if profile_uuid is None else profile_uuid
    client_uuid = bytes.fromhex("00112233445566778899aabbccddeeff")
    session_uuid = bytes.fromhex("ffeeddccbbaa99887766554433221100")

    handshake = (
        varint(0)
        + varint(776)
        + string("127.0.0.1")
        + (25_565).to_bytes(2, "big")
        + varint(intent)
    )
    hello = varint(0) + string(name) + client_uuid + hello_tail
    ack = varint(3) + ack_payload
    finished = varint(2) + offline_uuid + string(name) + varint(property_count) + session_uuid

    client_bodies = [handshake, hello, ack]
    server_bodies = [finished]
    if include_configuration_tail:
        client_bodies.append(b"\x00")
        server_bodies.append(b"\x00")

    artifact: dict[str, object] = {
        "schema": 1,
        "kind": "preplay-frame-capture-v1",
        "target": target,
        "limits": {
            "max_frame_bytes": 4096,
            "max_stream_bytes": 16384,
            "max_frames_per_direction": 16,
        },
        "streams": [
            stream("client-to-server", client_bodies),
            stream("server-to-client", server_bodies),
        ],
    }
    canonical = json.dumps(
        artifact, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("ascii")
    artifact["capture_sha256"] = hashlib.sha256(canonical).hexdigest()
    return artifact


class R1LoginContractTests(unittest.TestCase):
    def test_openjdk25_algorithm_matches_independent_runtime_vectors(self) -> None:
        fixture = json.loads(RUNTIME_FIXTURE.read_text(encoding="utf-8"))
        self.assertEqual(fixture["reference"]["ref"], "jdk-25+36")
        self.assertEqual(fixture["witness_runtime"]["version"], "26.0.2")
        for vector in fixture["vectors"]:
            actual = login._uuid_text(login.offline_player_uuid(vector["name"]))
            self.assertEqual(actual, vector["uuid"])

    def test_materializes_login_prefix_and_leaves_configuration_opaque(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            capture_path = root / "capture.json"
            output = root / "contract.json"
            witness = root / "witness.json"
            capture_path.write_text(json.dumps(capture()), encoding="utf-8")

            report = login.materialize(
                capture_path,
                output_path=output,
                witness_output_path=witness,
                lock_path=LOCK,
                records_root=RECORDS,
            )

            self.assertEqual(report["contract_id"], "PROTO-NET-LOGIN-26-2-001")
            self.assertEqual(report["packets"], 4)
            self.assertEqual(report["player_name"], "CrucibleR1")
            self.assertEqual(
                report["offline_profile_uuid"],
                "18030a72-9cb6-38f0-a5ba-e1c75f1314e9",
            )
            self.assertEqual(
                report["uninterpreted_post_login_frames"],
                {
                    "client_to_server_after_login": 1,
                    "server_to_client_after_login_finished": 1,
                },
            )
            contract_json = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(
                [packet["name"] for packet in contract_json["packets"]],
                [
                    "client-intention-login",
                    "login-hello",
                    "login-finished",
                    "login-acknowledged",
                ],
            )

    def test_rejects_wrong_offline_profile_uuid(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "capture.json"
            path.write_text(
                json.dumps(capture(profile_uuid=b"\0" * 16)),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                login.R1LoginMaterializationError,
                "does not equal the Java-25/source-defined offline UUID",
            ):
                login.build_contract(path, lock_path=LOCK)

    def test_rejects_wrong_handshake_intent(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "capture.json"
            path.write_text(json.dumps(capture(intent=1)), encoding="utf-8")
            with self.assertRaisesRegex(login.R1LoginMaterializationError, "Login intent must be 2"):
                login.build_contract(path, lock_path=LOCK)

    def test_rejects_nonempty_login_acknowledgement(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "capture.json"
            path.write_text(json.dumps(capture(ack_payload=b"\x01")), encoding="utf-8")
            with self.assertRaisesRegex(
                login.R1LoginMaterializationError,
                "must have an empty payload",
            ):
                login.build_contract(path, lock_path=LOCK)

    def test_rejects_hello_trailing_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "capture.json"
            path.write_text(json.dumps(capture(hello_tail=b"\x00")), encoding="utf-8")
            with self.assertRaisesRegex(login.R1LoginMaterializationError, "hello has trailing"):
                login.build_contract(path, lock_path=LOCK)

    def test_rejects_profile_property_count_above_source_bound(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "capture.json"
            path.write_text(json.dumps(capture(property_count=17)), encoding="utf-8")
            with self.assertRaisesRegex(login.R1LoginMaterializationError, "maximum is 16"):
                login.build_contract(path, lock_path=LOCK)


if __name__ == "__main__":
    unittest.main()
