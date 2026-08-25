from __future__ import annotations

import hashlib
import json
import struct
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r1b_pack_join_replay as packer


def _frame(body: bytes) -> dict[str, object]:
    return {
        "body_bytes": len(body),
        "body_hex": body.hex(),
        "body_sha256": hashlib.sha256(body).hexdigest(),
    }


def _fixture() -> tuple[dict[str, object], list[bytes], list[bytes]]:
    config = [b"\x01config-a", b"\x0cconfig-b"]
    play = [b"\x10play-a", b"\x11play-b", b"\x12play-c"]
    config_concat = b"".join(config)
    play_concat = b"".join(play)
    value: dict[str, object] = {
        "schema": packer.SCHEMA,
        "kind": packer.KIND,
        "target": {
            "minecraft": packer.EXPECTED_MINECRAFT,
            "protocol": packer.EXPECTED_PROTOCOL,
            "source_archive_sha256": packer.EXPECTED_SOURCE_SHA256,
            "capture_sha256": packer.EXPECTED_CAPTURE_SHA256,
        },
        "selected_capture_profile": {
            "player_name": packer.EXPECTED_PLAYER_NAME,
            "offline_profile_uuid": packer.EXPECTED_OFFLINE_UUID,
            "session_uuid": packer.EXPECTED_SESSION_UUID,
        },
        "configuration": {
            "server_bodies": [_frame(body) for body in config],
            "body_bytes": len(config_concat),
            "body_concat_sha256": hashlib.sha256(config_concat).hexdigest(),
        },
        "play_replay": {
            "server_bodies": [_frame(body) for body in play],
            "frame_count": len(play),
            "body_bytes": len(play_concat),
            "body_concat_sha256": hashlib.sha256(play_concat).hexdigest(),
            "experimental": True,
        },
        "production_admitted": False,
    }
    return value, config, play


class R1BPackJoinReplayTests(unittest.TestCase):
    def _patched_constants(self, config: list[bytes], play: list[bytes]):
        config_concat = b"".join(config)
        play_concat = b"".join(play)
        return (
            mock.patch.object(packer, "EXPECTED_CONFIG_COUNT", len(config)),
            mock.patch.object(packer, "EXPECTED_CONFIG_BYTES", len(config_concat)),
            mock.patch.object(
                packer,
                "EXPECTED_CONFIG_BODY_SHA256",
                hashlib.sha256(config_concat).hexdigest(),
            ),
            mock.patch.object(packer, "EXPECTED_PLAY_COUNT", len(play)),
            mock.patch.object(packer, "EXPECTED_PLAY_BYTES", len(play_concat)),
            mock.patch.object(
                packer,
                "EXPECTED_PLAY_BODY_SHA256",
                hashlib.sha256(play_concat).hexdigest(),
            ),
        )

    def test_pack_validates_full_source_then_emits_selected_prefix(self) -> None:
        value, config, play = _fixture()
        patches = self._patched_constants(config, play)
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "replay.json"
            output = Path(tmp) / "replay.r1x"
            source.write_text(json.dumps(value), encoding="utf-8")
            with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
                selected_count, selected_bytes = packer.pack(source, output, 2)

            raw = output.read_bytes()

        self.assertEqual(selected_count, 2)
        self.assertEqual(selected_bytes, sum(map(len, play[:2])))
        self.assertEqual(raw[:8], packer.MAGIC)
        self.assertEqual(struct.unpack_from("<I", raw, 8)[0], packer.EXPECTED_PROTOCOL)
        self.assertEqual(struct.unpack_from("<I", raw, 76)[0], len(config))
        self.assertEqual(struct.unpack_from("<I", raw, 80)[0], 2)
        self.assertEqual(struct.unpack_from("<Q", raw, 84)[0], sum(map(len, config)))
        self.assertEqual(struct.unpack_from("<Q", raw, 92)[0], sum(map(len, play[:2])))

    def test_tampered_body_hash_fails_before_output_creation(self) -> None:
        value, config, play = _fixture()
        value["play_replay"]["server_bodies"][1]["body_sha256"] = "0" * 64
        patches = self._patched_constants(config, play)
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "replay.json"
            output = Path(tmp) / "replay.r1x"
            source.write_text(json.dumps(value), encoding="utf-8")
            with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
                with self.assertRaisesRegex(packer.PackError, "body_sha256 mismatch"):
                    packer.pack(source, output, 1)
            self.assertFalse(output.exists())

    def test_play_limit_is_bounded_by_validated_full_capture(self) -> None:
        value, config, play = _fixture()
        patches = self._patched_constants(config, play)
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "replay.json"
            output = Path(tmp) / "replay.r1x"
            source.write_text(json.dumps(value), encoding="utf-8")
            with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
                with self.assertRaisesRegex(packer.PackError, "play-frame-limit"):
                    packer.pack(source, output, len(play) + 1)


if __name__ == "__main__":
    unittest.main()
