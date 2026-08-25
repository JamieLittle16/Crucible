from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r1b_join_capture_image as image


def _var_int(value: int) -> bytes:
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def _frame(ordinal: int, body: bytes) -> dict[str, object]:
    raw = _var_int(len(body)) + body
    return {
        "ordinal": ordinal,
        "stream_offset": 0,
        "frame_bytes": len(raw),
        "body_bytes": len(body),
        "frame_sha256": hashlib.sha256(raw).hexdigest(),
        "frame_hex": raw.hex(),
        "body_hex": body.hex(),
    }


def _capture() -> dict[str, object]:
    server_bodies = [bytes([0x02])] + [bytes([index & 0x7F]) for index in range(1, 35)] + [b"play-a", b"play-b"]
    client_bodies = [bytes([index]) for index in range(7)]
    server = [_frame(index, body) for index, body in enumerate(server_bodies)]
    client = [_frame(index, body) for index, body in enumerate(client_bodies)]
    value: dict[str, object] = {
        "schema": 1,
        "kind": image.CAPTURE_KIND,
        "target": {
            "minecraft": image.EXPECTED_MINECRAFT,
            "protocol": image.EXPECTED_PROTOCOL,
            "source_archive_sha256": image.EXPECTED_SOURCE_SHA256,
        },
        "limits": {},
        "streams": [
            {"direction": "client-to-server", "frames": client},
            {"direction": "server-to-client", "frames": server},
        ],
    }
    canonical = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")
    value["capture_sha256"] = hashlib.sha256(canonical).hexdigest()
    return value


class R1BJoinCaptureImageTests(unittest.TestCase):
    def test_extract_validates_configuration_commitment_and_slices_play(self) -> None:
        capture = _capture()
        server = next(stream for stream in capture["streams"] if stream["direction"] == "server-to-client")
        config = server["frames"][1:35]
        config_bodies = b"".join(bytes.fromhex(frame["body_hex"]) for frame in config)
        config_frames = b"".join(bytes.fromhex(frame["frame_hex"]) for frame in config)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "capture.json"
            path.write_text(json.dumps(capture), encoding="utf-8")
            with (
                mock.patch.object(image, "EXPECTED_CAPTURE_SHA256", capture["capture_sha256"]),
                mock.patch.object(image, "EXPECTED_CONFIG_BODY_BYTES", len(config_bodies)),
                mock.patch.object(image, "EXPECTED_CONFIG_BODY_SHA256", hashlib.sha256(config_bodies).hexdigest()),
                mock.patch.object(image, "EXPECTED_CONFIG_FRAME_SHA256", hashlib.sha256(config_frames).hexdigest()),
            ):
                result = image.extract(path, 1)
        self.assertEqual(len(result["configuration"]["server_bodies"]), 34)
        self.assertEqual(result["play_replay"]["frame_count"], 1)
        self.assertFalse(result["production_admitted"])

    def test_capture_identity_mismatch_fails_closed(self) -> None:
        capture = _capture()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "capture.json"
            path.write_text(json.dumps(capture), encoding="utf-8")
            with self.assertRaisesRegex(image.ReplayImageError, "capture SHA-256 mismatch"):
                image.extract(path, None)

    def test_negative_play_limit_is_rejected(self) -> None:
        capture = _capture()
        server = next(stream for stream in capture["streams"] if stream["direction"] == "server-to-client")
        config = server["frames"][1:35]
        config_bodies = b"".join(bytes.fromhex(frame["body_hex"]) for frame in config)
        config_frames = b"".join(bytes.fromhex(frame["frame_hex"]) for frame in config)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "capture.json"
            path.write_text(json.dumps(capture), encoding="utf-8")
            with (
                mock.patch.object(image, "EXPECTED_CAPTURE_SHA256", capture["capture_sha256"]),
                mock.patch.object(image, "EXPECTED_CONFIG_BODY_BYTES", len(config_bodies)),
                mock.patch.object(image, "EXPECTED_CONFIG_BODY_SHA256", hashlib.sha256(config_bodies).hexdigest()),
                mock.patch.object(image, "EXPECTED_CONFIG_FRAME_SHA256", hashlib.sha256(config_frames).hexdigest()),
            ):
                with self.assertRaisesRegex(image.ReplayImageError, "non-negative"):
                    image.extract(path, -1)


if __name__ == "__main__":
    unittest.main()
