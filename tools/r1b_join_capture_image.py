#!/usr/bin/env python3
"""Extract a bounded, source-free R1B join replay image from a pinned protocol capture.

The input capture may contain a long post-login session. This tool validates the exact pinned
Minecraft 26.2 capture identity and the committed Configuration witness, then exports only packet
bodies needed by the experimental selected-profile join replay. The output contains no official
source text and is safe to inspect/commit subject to normal evidence review.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any, Sequence

SCHEMA = 1
KIND = "r1b-join-replay-image-v1"
CAPTURE_KIND = "preplay-frame-capture-v1"
EXPECTED_CAPTURE_SHA256 = "11ead8de74df70b40d7fb045ff9561f06f6e24238765d4141a1d090cab546b57"
EXPECTED_SOURCE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
EXPECTED_PROTOCOL = 776
EXPECTED_MINECRAFT = "26.2"
EXPECTED_CONFIG_FRAME_COUNT = 34
EXPECTED_CONFIG_BODY_BYTES = 44_432
EXPECTED_CONFIG_BODY_SHA256 = "a27058707adc7ad73f7960ce5a06a1443ada8e38ffe5a647452aadd44a1a0ec7"
EXPECTED_CONFIG_FRAME_SHA256 = "6f6a5f8ed72fda010453ecbfdfc3c5bd2959b08d5902b0254ee4c2e7716df6cd"


class ReplayImageError(ValueError):
    """Fail-closed replay-image extraction error."""


def _read_json(path: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise ReplayImageError(f"capture must be a real non-symlink file: {path}")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ReplayImageError("capture root must be an object")
    return value


def _canonical_capture_sha256(capture: dict[str, Any]) -> str:
    identity = dict(capture)
    identity.pop("capture_sha256", None)
    encoded = json.dumps(
        identity, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("ascii")
    return hashlib.sha256(encoded).hexdigest()


def _decode_hex(value: object, label: str) -> bytes:
    if not isinstance(value, str):
        raise ReplayImageError(f"{label} must be hexadecimal text")
    try:
        return bytes.fromhex(value)
    except ValueError as error:
        raise ReplayImageError(f"{label} is not valid hexadecimal") from error


def _stream(capture: dict[str, Any], direction: str) -> dict[str, Any]:
    streams = capture.get("streams")
    if not isinstance(streams, list):
        raise ReplayImageError("capture streams must be an array")
    matches = [
        item
        for item in streams
        if isinstance(item, dict) and item.get("direction") == direction
    ]
    if len(matches) != 1:
        raise ReplayImageError(f"capture must contain exactly one {direction} stream")
    return matches[0]


def _frames(stream: dict[str, Any], label: str) -> list[dict[str, Any]]:
    frames = stream.get("frames")
    if not isinstance(frames, list) or any(not isinstance(item, dict) for item in frames):
        raise ReplayImageError(f"{label}.frames must be an object array")
    for index, frame in enumerate(frames):
        if frame.get("ordinal") != index:
            raise ReplayImageError(f"{label} frame ordinals must be contiguous from zero")
        raw = _decode_hex(frame.get("frame_hex"), f"{label}[{index}].frame_hex")
        body = _decode_hex(frame.get("body_hex"), f"{label}[{index}].body_hex")
        if len(raw) != frame.get("frame_bytes") or len(body) != frame.get("body_bytes"):
            raise ReplayImageError(f"{label}[{index}] byte counts do not match hexadecimal data")
        if hashlib.sha256(raw).hexdigest() != frame.get("frame_sha256"):
            raise ReplayImageError(f"{label}[{index}] frame SHA-256 mismatch")
    return frames  # type: ignore[return-value]


def _concat(frames: list[dict[str, Any]], field: str) -> bytes:
    return b"".join(_decode_hex(frame[field], field) for frame in frames)


def extract(capture_path: Path, play_frame_limit: int | None) -> dict[str, Any]:
    capture = _read_json(capture_path)
    if capture.get("schema") != 1 or capture.get("kind") != CAPTURE_KIND:
        raise ReplayImageError("unsupported protocol capture schema/kind")
    declared_capture_sha = capture.get("capture_sha256")
    actual_capture_sha = _canonical_capture_sha256(capture)
    if declared_capture_sha != actual_capture_sha:
        raise ReplayImageError("capture canonical SHA-256 is invalid")
    if actual_capture_sha != EXPECTED_CAPTURE_SHA256:
        raise ReplayImageError(
            f"capture SHA-256 mismatch: expected {EXPECTED_CAPTURE_SHA256}, got {actual_capture_sha}"
        )
    target = capture.get("target")
    if not isinstance(target, dict):
        raise ReplayImageError("capture target must be an object")
    if target.get("minecraft") != EXPECTED_MINECRAFT or target.get("protocol") != EXPECTED_PROTOCOL:
        raise ReplayImageError("capture target is not Minecraft 26.2 / protocol 776")
    if target.get("source_archive_sha256") != EXPECTED_SOURCE_SHA256:
        raise ReplayImageError("capture source archive pin mismatch")

    server = _frames(_stream(capture, "server-to-client"), "server-to-client")
    client = _frames(_stream(capture, "client-to-server"), "client-to-server")
    if len(server) <= EXPECTED_CONFIG_FRAME_COUNT:
        raise ReplayImageError("capture does not contain the required post-Configuration Play traffic")
    if len(client) <= 6:
        raise ReplayImageError("capture does not contain the required Configuration client traffic")

    # Server ordinal 0 is the already-admitted LoginFinished packet. Configuration is ordinals 1..34.
    config = server[1 : 1 + EXPECTED_CONFIG_FRAME_COUNT]
    config_body_concat = _concat(config, "body_hex")
    config_frame_concat = _concat(config, "frame_hex")
    if len(config_body_concat) != EXPECTED_CONFIG_BODY_BYTES:
        raise ReplayImageError("Configuration body byte count does not match committed witness")
    if hashlib.sha256(config_body_concat).hexdigest() != EXPECTED_CONFIG_BODY_SHA256:
        raise ReplayImageError("Configuration body concatenation does not match committed witness")
    if hashlib.sha256(config_frame_concat).hexdigest() != EXPECTED_CONFIG_FRAME_SHA256:
        raise ReplayImageError("Configuration frame concatenation does not match committed witness")

    play = server[1 + EXPECTED_CONFIG_FRAME_COUNT :]
    if play_frame_limit is not None:
        if play_frame_limit < 0:
            raise ReplayImageError("play-frame-limit must be non-negative")
        play = play[:play_frame_limit]

    # Preserve exact captured client Configuration bodies for selected-route comparison/debugging.
    client_config = client[3:7]

    def export_frame(frame: dict[str, Any]) -> dict[str, Any]:
        body = _decode_hex(frame["body_hex"], "body_hex")
        return {
            "ordinal": frame["ordinal"],
            "body_bytes": frame["body_bytes"],
            "body_hex": frame["body_hex"],
            "body_sha256": hashlib.sha256(body).hexdigest(),
        }

    play_bodies = [export_frame(frame) for frame in play]
    play_concat = _concat(play, "body_hex") if play else b""
    result = {
        "schema": SCHEMA,
        "kind": KIND,
        "target": {
            "minecraft": EXPECTED_MINECRAFT,
            "protocol": EXPECTED_PROTOCOL,
            "source_archive_sha256": EXPECTED_SOURCE_SHA256,
            "capture_sha256": EXPECTED_CAPTURE_SHA256,
        },
        "selected_capture_profile": {
            "player_name": "Stato16",
            "offline_profile_uuid": "682014fe-ad63-3699-aada-79aa08d95b45",
            "session_uuid": "4d7f604f-196a-43b0-8987-f0b2a27c2663",
        },
        "configuration": {
            "server_bodies": [export_frame(frame) for frame in config],
            "body_bytes": len(config_body_concat),
            "body_concat_sha256": hashlib.sha256(config_body_concat).hexdigest(),
            "frame_concat_sha256": hashlib.sha256(config_frame_concat).hexdigest(),
            "client_bodies": [export_frame(frame) for frame in client_config],
        },
        "play_replay": {
            "server_bodies": play_bodies,
            "frame_count": len(play_bodies),
            "body_bytes": len(play_concat),
            "body_concat_sha256": hashlib.sha256(play_concat).hexdigest(),
            "experimental": True,
        },
        "production_admitted": False,
        "note": (
            "Experimental selected-profile replay image. Configuration bytes are independently source-admitted; "
            "captured Play bytes remain a smoke-test aid until GATE-NET-PLAY-ENTRY-26_2-001 is admitted."
        ),
    }
    return result


def render(value: object) -> str:
    return json.dumps(value, indent=2, sort_keys=True) + "\n"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-join-capture-image")
    parser.add_argument("capture", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--play-frame-limit", type=int)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        image = extract(args.capture, args.play_frame_limit)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(render(image), encoding="utf-8")
    except (OSError, json.JSONDecodeError, ReplayImageError) as error:
        print(f"R1B join capture-image error: {error}", file=sys.stderr)
        return 2
    print(f"replay_image={args.output}")
    print(f"configuration_frames={len(image['configuration']['server_bodies'])}")
    print(f"play_frames={image['play_replay']['frame_count']}")
    print(f"capture_sha256={EXPECTED_CAPTURE_SHA256}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
