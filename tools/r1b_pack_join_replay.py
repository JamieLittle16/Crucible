#!/usr/bin/env python3
"""Pack a validated source-free R1X join replay JSON into a compact runtime image.

The JSON is a review/debug artifact and intentionally verbose. The runtime image is a cold-path
development fixture: packet bodies are validated once here, then Crucible can load them without a
JSON parser or per-connection reconstruction.

This does not promote captured Play traffic to production evidence. Configuration is independently
source-admitted; Play remains an explicitly experimental smoke-test replay.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path
from typing import Any, Sequence

MAGIC = b"CRR1X001"
SCHEMA = 1
KIND = "r1b-join-replay-image-v1"
EXPECTED_MINECRAFT = "26.2"
EXPECTED_PROTOCOL = 776
EXPECTED_SOURCE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
EXPECTED_CAPTURE_SHA256 = "11ead8de74df70b40d7fb045ff9561f06f6e24238765d4141a1d090cab546b57"
EXPECTED_CONFIG_COUNT = 34
EXPECTED_CONFIG_BYTES = 44_432
EXPECTED_CONFIG_BODY_SHA256 = "a27058707adc7ad73f7960ce5a06a1443ada8e38ffe5a647452aadd44a1a0ec7"
EXPECTED_PLAY_COUNT = 2_331
EXPECTED_PLAY_BYTES = 6_135_522
EXPECTED_PLAY_BODY_SHA256 = "6dc8314fb04cc0a88729fd02a9a7ff17d40ae2f58de67719e3205881b90528b5"
EXPECTED_PLAYER_NAME = "Stato16"
EXPECTED_OFFLINE_UUID = "682014fe-ad63-3699-aada-79aa08d95b45"
EXPECTED_SESSION_UUID = "4d7f604f-196a-43b0-8987-f0b2a27c2663"
MAX_BODY_BYTES = 65_536


class PackError(ValueError):
    """Fail-closed runtime-image packing error."""


def _read_json(path: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise PackError(f"input must be a real non-symlink file: {path}")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise PackError("input root must be an object")
    return value


def _hex_body(frame: object, label: str) -> bytes:
    if not isinstance(frame, dict):
        raise PackError(f"{label} must be an object")
    text = frame.get("body_hex")
    if not isinstance(text, str):
        raise PackError(f"{label}.body_hex must be hexadecimal text")
    try:
        body = bytes.fromhex(text)
    except ValueError as error:
        raise PackError(f"{label}.body_hex is not valid hexadecimal") from error
    if not body:
        raise PackError(f"{label} body must contain at least a packet id")
    if len(body) > MAX_BODY_BYTES:
        raise PackError(f"{label} exceeds the R1X {MAX_BODY_BYTES}-byte body bound")
    if frame.get("body_bytes") != len(body):
        raise PackError(f"{label}.body_bytes does not match body_hex")
    if frame.get("body_sha256") != hashlib.sha256(body).hexdigest():
        raise PackError(f"{label}.body_sha256 mismatch")
    return body


def _bodies(section: object, label: str) -> tuple[list[bytes], dict[str, Any]]:
    if not isinstance(section, dict):
        raise PackError(f"{label} must be an object")
    raw = section.get("server_bodies")
    if not isinstance(raw, list):
        raise PackError(f"{label}.server_bodies must be an array")
    bodies = [
        _hex_body(frame, f"{label}.server_bodies[{index}]")
        for index, frame in enumerate(raw)
    ]
    return bodies, section


def _validate(value: dict[str, Any]) -> tuple[list[bytes], list[bytes]]:
    if value.get("schema") != SCHEMA or value.get("kind") != KIND:
        raise PackError("unsupported replay JSON schema/kind")
    if value.get("production_admitted") is not False:
        raise PackError("R1X source JSON must remain explicitly non-production")

    target = value.get("target")
    if not isinstance(target, dict):
        raise PackError("target must be an object")
    expected_target = {
        "minecraft": EXPECTED_MINECRAFT,
        "protocol": EXPECTED_PROTOCOL,
        "source_archive_sha256": EXPECTED_SOURCE_SHA256,
        "capture_sha256": EXPECTED_CAPTURE_SHA256,
    }
    for key, expected in expected_target.items():
        if target.get(key) != expected:
            raise PackError(f"target.{key} mismatch")

    profile = value.get("selected_capture_profile")
    if not isinstance(profile, dict):
        raise PackError("selected_capture_profile must be an object")
    expected_profile = {
        "player_name": EXPECTED_PLAYER_NAME,
        "offline_profile_uuid": EXPECTED_OFFLINE_UUID,
        "session_uuid": EXPECTED_SESSION_UUID,
    }
    for key, expected in expected_profile.items():
        if profile.get(key) != expected:
            raise PackError(f"selected_capture_profile.{key} mismatch")

    config, config_section = _bodies(value.get("configuration"), "configuration")
    if len(config) != EXPECTED_CONFIG_COUNT:
        raise PackError(
            f"expected {EXPECTED_CONFIG_COUNT} Configuration bodies, got {len(config)}"
        )
    config_concat = b"".join(config)
    if len(config_concat) != EXPECTED_CONFIG_BYTES:
        raise PackError("Configuration byte count mismatch")
    if hashlib.sha256(config_concat).hexdigest() != EXPECTED_CONFIG_BODY_SHA256:
        raise PackError("Configuration concatenation SHA-256 mismatch")
    if config_section.get("body_bytes") != EXPECTED_CONFIG_BYTES:
        raise PackError("Configuration declared body_bytes mismatch")
    if config_section.get("body_concat_sha256") != EXPECTED_CONFIG_BODY_SHA256:
        raise PackError("Configuration declared body hash mismatch")

    play, play_section = _bodies(value.get("play_replay"), "play_replay")
    if play_section.get("experimental") is not True:
        raise PackError("Play replay must remain explicitly experimental")
    if len(play) != EXPECTED_PLAY_COUNT or play_section.get("frame_count") != EXPECTED_PLAY_COUNT:
        raise PackError(f"expected exact {EXPECTED_PLAY_COUNT}-frame captured Play image")
    play_concat = b"".join(play)
    if len(play_concat) != EXPECTED_PLAY_BYTES or play_section.get("body_bytes") != EXPECTED_PLAY_BYTES:
        raise PackError("captured Play byte count mismatch")
    if hashlib.sha256(play_concat).hexdigest() != EXPECTED_PLAY_BODY_SHA256:
        raise PackError("captured Play concatenation SHA-256 mismatch")
    if play_section.get("body_concat_sha256") != EXPECTED_PLAY_BODY_SHA256:
        raise PackError("captured Play declared body hash mismatch")

    return config, play


def _write_u32(output, value: int) -> None:
    output.write(struct.pack("<I", value))


def _write_u64(output, value: int) -> None:
    output.write(struct.pack("<Q", value))


def pack(input_path: Path, output_path: Path, play_frame_limit: int | None) -> tuple[int, int]:
    value = _read_json(input_path)
    config, full_play = _validate(value)

    if play_frame_limit is None:
        play = full_play
    else:
        if not 0 <= play_frame_limit <= len(full_play):
            raise PackError(f"play-frame-limit must be between 0 and {len(full_play)}")
        play = full_play[:play_frame_limit]

    config_bytes = sum(map(len, config))
    play_bytes = sum(map(len, play))

    if output_path.is_symlink():
        raise PackError(f"output must not be a symlink: {output_path}")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    temporary = output_path.with_name(f".{output_path.name}.tmp")
    if temporary.exists() or temporary.is_symlink():
        temporary.unlink()
    try:
        with temporary.open("xb") as output:
            output.write(MAGIC)
            _write_u32(output, EXPECTED_PROTOCOL)
            output.write(bytes.fromhex(EXPECTED_SOURCE_SHA256))
            output.write(bytes.fromhex(EXPECTED_CAPTURE_SHA256))
            _write_u32(output, len(config))
            _write_u32(output, len(play))
            _write_u64(output, config_bytes)
            _write_u64(output, play_bytes)
            for body in [*config, *play]:
                _write_u32(output, len(body))
                output.write(body)
        temporary.replace(output_path)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise

    return len(play), play_bytes


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-pack-join-replay")
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--play-frame-limit",
        type=int,
        help="pack only the first N captured Play bodies after validating the complete source JSON",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        play_frames, play_bytes = pack(args.input, args.output, args.play_frame_limit)
    except (OSError, json.JSONDecodeError, PackError) as error:
        print(f"R1X replay pack error: {error}", file=sys.stderr)
        return 2

    print(f"r1x_image={args.output}")
    print(f"configuration_frames={EXPECTED_CONFIG_COUNT}")
    print(f"configuration_bytes={EXPECTED_CONFIG_BYTES}")
    print(f"play_frames={play_frames}")
    print(f"play_bytes={play_bytes}")
    print(f"capture_sha256={EXPECTED_CAPTURE_SHA256}")
    print("production_admitted=false")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
