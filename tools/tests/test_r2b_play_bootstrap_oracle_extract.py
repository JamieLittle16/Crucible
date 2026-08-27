from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r1b_pack_join_replay as replay
from tools import r2b_play_bootstrap_oracle_extract as oracle


def _varint(value: int) -> bytes:
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def _frame(body: bytes) -> dict[str, object]:
    return {
        "body_bytes": len(body),
        "body_hex": body.hex(),
        "body_sha256": hashlib.sha256(body).hexdigest(),
    }


def _fixture() -> tuple[dict[str, object], list[bytes], list[bytes]]:
    config = [b"\x01config"]
    commands = _varint(oracle.COMMANDS_ID) + b"command-image"
    recipes = _varint(oracle.UPDATE_RECIPES_ID) + b"recipe-image"
    play = [b"\x01other", commands, b"\x02other", recipes]
    config_concat = b"".join(config)
    play_concat = b"".join(play)
    value: dict[str, object] = {
        "schema": replay.SCHEMA,
        "kind": replay.KIND,
        "target": {
            "minecraft": replay.EXPECTED_MINECRAFT,
            "protocol": replay.EXPECTED_PROTOCOL,
            "source_archive_sha256": replay.EXPECTED_SOURCE_SHA256,
            "capture_sha256": replay.EXPECTED_CAPTURE_SHA256,
        },
        "selected_capture_profile": {
            "player_name": replay.EXPECTED_PLAYER_NAME,
            "offline_profile_uuid": replay.EXPECTED_OFFLINE_UUID,
            "session_uuid": replay.EXPECTED_SESSION_UUID,
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


class BootstrapOracleExtractTests(unittest.TestCase):
    def _patched_capture(self, config: list[bytes], play: list[bytes]):
        config_concat = b"".join(config)
        play_concat = b"".join(play)
        return (
            mock.patch.object(replay, "EXPECTED_CONFIG_COUNT", len(config)),
            mock.patch.object(replay, "EXPECTED_CONFIG_BYTES", len(config_concat)),
            mock.patch.object(
                replay,
                "EXPECTED_CONFIG_BODY_SHA256",
                hashlib.sha256(config_concat).hexdigest(),
            ),
            mock.patch.object(replay, "EXPECTED_PLAY_COUNT", len(play)),
            mock.patch.object(replay, "EXPECTED_PLAY_BYTES", len(play_concat)),
            mock.patch.object(
                replay,
                "EXPECTED_PLAY_BODY_SHA256",
                hashlib.sha256(play_concat).hexdigest(),
            ),
        )

    def test_extracts_exactly_one_source_qualified_body_per_target(self) -> None:
        value, config, play = _fixture()
        patches = self._patched_capture(config, play)
        with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
            result = oracle.extract(value)

        artifacts = result["artifacts"]
        self.assertEqual([item["packet_id"] for item in artifacts], [16, 133])
        self.assertEqual([item["play_body_index"] for item in artifacts], [1, 3])
        self.assertEqual(artifacts[0]["packet_id_bytes"], 1)
        self.assertEqual(artifacts[1]["packet_id_bytes"], 2)
        self.assertEqual(bytes.fromhex(artifacts[0]["body_hex"]), play[1])
        self.assertEqual(bytes.fromhex(artifacts[1]["body_hex"]), play[3])
        self.assertTrue(result["oracle_only"])
        self.assertFalse(result["production_admitted"])

    def test_duplicate_target_packet_fails_closed(self) -> None:
        value, config, play = _fixture()
        play.append(_varint(oracle.COMMANDS_ID) + b"duplicate")
        value["play_replay"]["server_bodies"] = [_frame(body) for body in play]
        value["play_replay"]["frame_count"] = len(play)
        value["play_replay"]["body_bytes"] = len(b"".join(play))
        value["play_replay"]["body_concat_sha256"] = hashlib.sha256(b"".join(play)).hexdigest()
        patches = self._patched_capture(config, play)
        with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
            with self.assertRaisesRegex(oracle.OracleExtractError, "exactly one Play commands"):
                oracle.extract(value)

    def test_missing_target_packet_fails_closed(self) -> None:
        value, config, play = _fixture()
        play.pop()
        value["play_replay"]["server_bodies"] = [_frame(body) for body in play]
        value["play_replay"]["frame_count"] = len(play)
        value["play_replay"]["body_bytes"] = len(b"".join(play))
        value["play_replay"]["body_concat_sha256"] = hashlib.sha256(b"".join(play)).hexdigest()
        patches = self._patched_capture(config, play)
        with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
            with self.assertRaisesRegex(oracle.OracleExtractError, "update-recipes"):
                oracle.extract(value)

    def test_packet_id_varint_must_be_complete_and_signed_32_bit(self) -> None:
        with self.assertRaisesRegex(oracle.OracleExtractError, "complete packet-id"):
            oracle.decode_varint_prefix(b"\x80\x80\x80\x80\x80")
        with self.assertRaisesRegex(oracle.OracleExtractError, "signed 32-bit"):
            oracle.decode_varint_prefix(b"\xff\xff\xff\xff\x0f")

    def test_write_is_atomic_and_source_free(self) -> None:
        value, config, play = _fixture()
        patches = self._patched_capture(config, play)
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "oracle.json"
            with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
                result = oracle.extract(value)
            oracle.write(output, result)
            loaded = json.loads(output.read_text(encoding="utf-8"))

        self.assertEqual(loaded, result)
        self.assertNotIn("source_excerpt", output.read_text(encoding="utf-8") if output.exists() else "")


if __name__ == "__main__":
    unittest.main()
