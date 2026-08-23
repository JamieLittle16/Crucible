#!/usr/bin/env python3
"""Cross-check a source-admitted R0 protocol contract against black-box frame evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any

from tools.protocol_contract import ContractError, validate_contract

SCHEMA = 1
CAPTURE_SCHEMA = 1
CAPTURE_KIND = "preplay-frame-capture-v1"
HEX_64 = re.compile(r"^[0-9a-f]{64}$")
HEX_BYTES = re.compile(r"^(?:[0-9a-f]{2})*$")
R0_PHASES = {"handshake", "status"}
CAPTURE_DIRECTIONS = {"client-to-server", "server-to-client"}
CONTRACT_TO_CAPTURE_DIRECTION = {
    "serverbound": "client-to-server",
    "clientbound": "server-to-client",
}


class EvidenceConvergenceError(ValueError):
    """Raised when source-admitted protocol bytes and black-box evidence do not converge."""


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise EvidenceConvergenceError(f"{label} must be an object")
    return value


def _keys(
    value: dict[str, Any], *, allowed: set[str], required: set[str], label: str
) -> None:
    unknown = sorted(set(value) - allowed)
    missing = sorted(required - set(value))
    if unknown:
        raise EvidenceConvergenceError(
            f"{label} contains unknown keys: {', '.join(unknown)}"
        )
    if missing:
        raise EvidenceConvergenceError(
            f"{label} is missing required keys: {', '.join(missing)}"
        )


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise EvidenceConvergenceError(f"{label} must be a non-empty string")
    return value


def _integer(value: object, label: str) -> int:
    if type(value) is not int:
        raise EvidenceConvergenceError(f"{label} must be an integer")
    return value


def _positive_integer(value: object, label: str) -> int:
    result = _integer(value, label)
    if result <= 0:
        raise EvidenceConvergenceError(f"{label} must be positive")
    return result


def _sha256(value: object, label: str) -> str:
    digest = _string(value, label)
    if HEX_64.fullmatch(digest) is None:
        raise EvidenceConvergenceError(f"{label} must be lowercase SHA-256")
    return digest


def _hex_bytes(value: object, label: str) -> bytes:
    text = _string(value, label)
    if HEX_BYTES.fullmatch(text) is None:
        raise EvidenceConvergenceError(
            f"{label} must be canonical lowercase hexadecimal bytes"
        )
    return bytes.fromhex(text)


def _read_json(path: Path, label: str) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise EvidenceConvergenceError(
            f"{label} must be a real non-symlink file: {path}"
        )
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvidenceConvergenceError(f"could not read {label} {path}: {error}") from error
    return _object(value, label)


def _encode_nonnegative_var_int(value: int) -> bytes:
    if not 0 <= value <= 0x7FFF_FFFF:
        raise EvidenceConvergenceError(
            f"frame length is outside non-negative i32 range: {value}"
        )
    encoded = bytearray()
    remaining = value
    while True:
        byte = remaining & 0x7F
        remaining >>= 7
        if remaining:
            encoded.append(byte | 0x80)
        else:
            encoded.append(byte)
            return bytes(encoded)


def _decode_frame_length(data: bytes, label: str) -> tuple[int, int]:
    value = 0
    for index in range(min(len(data), 5)):
        byte = data[index]
        value |= (byte & 0x7F) << (7 * index)
        if byte & 0x80 == 0:
            consumed = index + 1
            if value > 0x7FFF_FFFF:
                raise EvidenceConvergenceError(
                    f"{label} exceeds non-negative i32 VarInt range"
                )
            if data[:consumed] != _encode_nonnegative_var_int(value):
                raise EvidenceConvergenceError(
                    f"{label} uses a noncanonical VarInt encoding"
                )
            return value, consumed
    if len(data) < 5:
        raise EvidenceConvergenceError(f"{label} is truncated")
    raise EvidenceConvergenceError(f"{label} exceeds the five-byte VarInt bound")


def _validate_target(value: object, label: str) -> dict[str, object]:
    target = _object(value, label)
    _keys(
        target,
        allowed={
            "minecraft",
            "protocol",
            "source_archive_sha256",
            "fingerprint_algorithm",
        },
        required={
            "minecraft",
            "protocol",
            "source_archive_sha256",
            "fingerprint_algorithm",
        },
        label=label,
    )
    return {
        "minecraft": _string(target["minecraft"], f"{label}.minecraft"),
        "protocol": _integer(target["protocol"], f"{label}.protocol"),
        "source_archive_sha256": _sha256(
            target["source_archive_sha256"], f"{label}.source_archive_sha256"
        ),
        "fingerprint_algorithm": _string(
            target["fingerprint_algorithm"], f"{label}.fingerprint_algorithm"
        ),
    }


def _validate_frame(
    value: object,
    *,
    direction: str,
    index: int,
    expected_offset: int,
    max_frame_bytes: int,
) -> tuple[bytes, bytes]:
    label = f"capture {direction} frames[{index}]"
    frame = _object(value, label)
    _keys(
        frame,
        allowed={
            "ordinal",
            "stream_offset",
            "frame_bytes",
            "body_bytes",
            "frame_sha256",
            "frame_hex",
            "body_hex",
        },
        required={
            "ordinal",
            "stream_offset",
            "frame_bytes",
            "body_bytes",
            "frame_sha256",
            "frame_hex",
            "body_hex",
        },
        label=label,
    )
    ordinal = _integer(frame["ordinal"], f"{label}.ordinal")
    if ordinal != index:
        raise EvidenceConvergenceError(
            f"{label}.ordinal must be contiguous from zero; expected {index}, got {ordinal}"
        )
    stream_offset = _integer(frame["stream_offset"], f"{label}.stream_offset")
    if stream_offset != expected_offset:
        raise EvidenceConvergenceError(
            f"{label}.stream_offset must be contiguous; expected {expected_offset}, got {stream_offset}"
        )
    frame_bytes = _integer(frame["frame_bytes"], f"{label}.frame_bytes")
    body_bytes = _integer(frame["body_bytes"], f"{label}.body_bytes")
    if frame_bytes < 0 or body_bytes < 0:
        raise EvidenceConvergenceError(f"{label} byte counts must be non-negative")
    raw_frame = _hex_bytes(frame["frame_hex"], f"{label}.frame_hex")
    body = _hex_bytes(frame["body_hex"], f"{label}.body_hex")
    if frame_bytes != len(raw_frame):
        raise EvidenceConvergenceError(
            f"{label}.frame_bytes does not match decoded frame length"
        )
    if body_bytes != len(body):
        raise EvidenceConvergenceError(
            f"{label}.body_bytes does not match decoded body length"
        )
    if body_bytes > max_frame_bytes:
        raise EvidenceConvergenceError(
            f"{label} exceeds capture max_frame_bytes {max_frame_bytes}"
        )
    declared_sha = _sha256(frame["frame_sha256"], f"{label}.frame_sha256")
    actual_sha = hashlib.sha256(raw_frame).hexdigest()
    if declared_sha != actual_sha:
        raise EvidenceConvergenceError(f"{label}.frame_sha256 does not match frame bytes")
    framed_length, prefix_bytes = _decode_frame_length(raw_frame, f"{label} frame length")
    framed_body = raw_frame[prefix_bytes:]
    if framed_length != len(framed_body):
        raise EvidenceConvergenceError(
            f"{label} frame length {framed_length} does not match {len(framed_body)} body bytes"
        )
    if framed_body != body:
        raise EvidenceConvergenceError(f"{label} frame body does not match body_hex")
    if raw_frame != _encode_nonnegative_var_int(len(body)) + body:
        raise EvidenceConvergenceError(f"{label} frame is not canonical")
    return raw_frame, body


def _validate_stream(
    value: object,
    *,
    expected_direction: str,
    max_frame_bytes: int,
    max_stream_bytes: int,
    max_frames: int,
) -> tuple[tuple[bytes, bytes], ...]:
    label = f"capture stream {expected_direction}"
    stream = _object(value, label)
    _keys(
        stream,
        allowed={"direction", "stream_bytes", "stream_sha256", "frame_count", "frames"},
        required={"direction", "stream_bytes", "stream_sha256", "frame_count", "frames"},
        label=label,
    )
    direction = _string(stream["direction"], f"{label}.direction")
    if direction != expected_direction:
        raise EvidenceConvergenceError(
            f"{label}.direction mismatch: expected {expected_direction!r}, got {direction!r}"
        )
    stream_bytes = _integer(stream["stream_bytes"], f"{label}.stream_bytes")
    if not 0 <= stream_bytes <= max_stream_bytes:
        raise EvidenceConvergenceError(
            f"{label}.stream_bytes exceeds configured bound {max_stream_bytes}"
        )
    frame_count = _integer(stream["frame_count"], f"{label}.frame_count")
    if not 0 <= frame_count <= max_frames:
        raise EvidenceConvergenceError(
            f"{label}.frame_count exceeds configured bound {max_frames}"
        )
    frames = stream["frames"]
    if not isinstance(frames, list):
        raise EvidenceConvergenceError(f"{label}.frames must be an array")
    if frame_count != len(frames):
        raise EvidenceConvergenceError(
            f"{label}.frame_count does not match frames array length"
        )

    validated: list[tuple[bytes, bytes]] = []
    offset = 0
    stream_image = bytearray()
    for index, frame in enumerate(frames):
        raw_frame, body = _validate_frame(
            frame,
            direction=expected_direction,
            index=index,
            expected_offset=offset,
            max_frame_bytes=max_frame_bytes,
        )
        validated.append((raw_frame, body))
        stream_image.extend(raw_frame)
        offset += len(raw_frame)
    if offset != stream_bytes:
        raise EvidenceConvergenceError(
            f"{label}.stream_bytes does not match concatenated frame bytes"
        )
    declared_sha = _sha256(stream["stream_sha256"], f"{label}.stream_sha256")
    actual_sha = hashlib.sha256(stream_image).hexdigest()
    if declared_sha != actual_sha:
        raise EvidenceConvergenceError(
            f"{label}.stream_sha256 does not match concatenated frame bytes"
        )
    return tuple(validated)


def _validate_capture(
    capture_path: Path,
    *,
    expected_target: dict[str, object],
) -> tuple[str, dict[str, tuple[tuple[bytes, bytes], ...]]]:
    capture = _read_json(capture_path, "protocol capture")
    _keys(
        capture,
        allowed={"schema", "kind", "target", "limits", "streams", "capture_sha256"},
        required={"schema", "kind", "target", "limits", "streams", "capture_sha256"},
        label="protocol capture",
    )
    if _integer(capture["schema"], "protocol capture schema") != CAPTURE_SCHEMA:
        raise EvidenceConvergenceError("unsupported protocol-capture schema")
    if _string(capture["kind"], "protocol capture kind") != CAPTURE_KIND:
        raise EvidenceConvergenceError("unsupported protocol-capture kind")
    target = _validate_target(capture["target"], "protocol capture target")
    if target != expected_target:
        raise EvidenceConvergenceError(
            "protocol capture target does not match admitted contract target"
        )

    limits = _object(capture["limits"], "protocol capture limits")
    _keys(
        limits,
        allowed={"max_frame_bytes", "max_stream_bytes", "max_frames_per_direction"},
        required={"max_frame_bytes", "max_stream_bytes", "max_frames_per_direction"},
        label="protocol capture limits",
    )
    max_frame_bytes = _positive_integer(
        limits["max_frame_bytes"], "protocol capture limits.max_frame_bytes"
    )
    if max_frame_bytes > 0x7FFF_FFFF:
        raise EvidenceConvergenceError(
            "protocol capture max_frame_bytes exceeds non-negative i32 range"
        )
    max_stream_bytes = _positive_integer(
        limits["max_stream_bytes"], "protocol capture limits.max_stream_bytes"
    )
    max_frames = _positive_integer(
        limits["max_frames_per_direction"],
        "protocol capture limits.max_frames_per_direction",
    )

    streams = capture["streams"]
    if not isinstance(streams, list) or len(streams) != 2:
        raise EvidenceConvergenceError(
            "protocol capture must contain exactly two directional streams"
        )
    by_direction: dict[str, dict[str, Any]] = {}
    for index, value in enumerate(streams):
        stream = _object(value, f"protocol capture streams[{index}]")
        direction = _string(
            stream.get("direction"), f"protocol capture streams[{index}].direction"
        )
        if direction not in CAPTURE_DIRECTIONS:
            raise EvidenceConvergenceError(
                f"protocol capture stream has unsupported direction {direction!r}"
            )
        if direction in by_direction:
            raise EvidenceConvergenceError(
                f"protocol capture contains duplicate stream direction {direction!r}"
            )
        by_direction[direction] = stream
    if set(by_direction) != CAPTURE_DIRECTIONS:
        raise EvidenceConvergenceError(
            "protocol capture must contain client-to-server and server-to-client streams"
        )

    validated = {
        direction: _validate_stream(
            by_direction[direction],
            expected_direction=direction,
            max_frame_bytes=max_frame_bytes,
            max_stream_bytes=max_stream_bytes,
            max_frames=max_frames,
        )
        for direction in sorted(CAPTURE_DIRECTIONS)
    }

    capture_sha256 = _sha256(capture["capture_sha256"], "protocol capture capture_sha256")
    identity = dict(capture)
    identity.pop("capture_sha256")
    canonical = json.dumps(
        identity, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("ascii")
    actual_capture_sha256 = hashlib.sha256(canonical).hexdigest()
    if capture_sha256 != actual_capture_sha256:
        raise EvidenceConvergenceError(
            "protocol capture capture_sha256 does not match canonical artifact bytes"
        )
    return capture_sha256, validated


def _contract_sequences(contract: dict[str, Any]) -> dict[str, tuple[tuple[str, bytes, bytes], ...]]:
    packets = contract.get("packets")
    if not isinstance(packets, list):
        raise EvidenceConvergenceError("validated protocol contract packets disappeared")
    sequences: dict[str, list[tuple[str, bytes, bytes]]] = {
        direction: [] for direction in CAPTURE_DIRECTIONS
    }
    for index, value in enumerate(packets):
        packet = _object(value, f"validated contract packets[{index}]")
        phase = _string(packet.get("phase"), f"validated contract packets[{index}].phase")
        if phase not in R0_PHASES:
            raise EvidenceConvergenceError(
                f"P0L v1 rejects non-R0 phase {phase!r}; capture semantics end at handshake/status"
            )
        contract_direction = _string(
            packet.get("direction"), f"validated contract packets[{index}].direction"
        )
        capture_direction = CONTRACT_TO_CAPTURE_DIRECTION.get(contract_direction)
        if capture_direction is None:
            raise EvidenceConvergenceError(
                f"validated contract uses unsupported direction {contract_direction!r}"
            )
        name = _string(packet.get("name"), f"validated contract packets[{index}].name")
        golden = _object(
            packet.get("golden"), f"validated contract packets[{index}].golden"
        )
        body = _hex_bytes(
            golden.get("body_hex"), f"validated contract packets[{index}].golden.body_hex"
        )
        frame = _hex_bytes(
            golden.get("frame_hex"), f"validated contract packets[{index}].golden.frame_hex"
        )
        sequences[capture_direction].append((name, frame, body))
    return {direction: tuple(items) for direction, items in sequences.items()}


def crosscheck_capture(
    contract_path: Path,
    capture_path: Path,
    *,
    lock_path: Path,
    records_root: Path,
) -> dict[str, object]:
    """Validate and cross-check one finite R0 contract against one P0K capture artifact."""
    try:
        admission = validate_contract(
            contract_path,
            lock_path=lock_path,
            records_root=records_root,
        )
    except ContractError as error:
        raise EvidenceConvergenceError(
            f"protocol contract failed source admission: {error}"
        ) from error

    contract = _read_json(contract_path, "validated protocol contract")
    expected_target = _validate_target(contract.get("target"), "validated protocol contract target")
    contract_sequences = _contract_sequences(contract)
    capture_sha256, captured_sequences = _validate_capture(
        capture_path,
        expected_target=expected_target,
    )

    total_frames = 0
    counts: dict[str, int] = {}
    for direction in sorted(CAPTURE_DIRECTIONS):
        expected = contract_sequences[direction]
        actual = captured_sequences[direction]
        if len(expected) != len(actual):
            raise EvidenceConvergenceError(
                f"{direction} frame count differs from source-admitted contract: "
                f"expected {len(expected)}, captured {len(actual)}"
            )
        for index, ((name, expected_frame, expected_body), (actual_frame, actual_body)) in enumerate(
            zip(expected, actual, strict=True)
        ):
            if actual_frame != expected_frame:
                raise EvidenceConvergenceError(
                    f"{direction} frame[{index}] does not match contract packet {name!r} golden frame"
                )
            if actual_body != expected_body:
                raise EvidenceConvergenceError(
                    f"{direction} frame[{index}] body does not match contract packet {name!r} golden body"
                )
        counts[direction] = len(actual)
        total_frames += len(actual)

    if total_frames == 0:
        raise EvidenceConvergenceError("R0 evidence convergence requires at least one captured frame")
    if counts["client-to-server"] == 0 or counts["server-to-client"] == 0:
        raise EvidenceConvergenceError(
            "R0 evidence convergence requires traffic in both directions"
        )

    return {
        "schema": SCHEMA,
        "contract_id": admission["id"],
        "capture_sha256": capture_sha256,
        "minecraft": admission["minecraft"],
        "protocol": admission["protocol"],
        "client_to_server_frames": counts["client-to-server"],
        "server_to_client_frames": counts["server-to-client"],
        "frames_matched": total_frames,
    }


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("contract", type=Path)
    parser.add_argument("capture", type=Path)
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--records-root", type=Path, default=Path("vanilla/records"))
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    try:
        summary = crosscheck_capture(
            args.contract,
            args.capture,
            lock_path=args.lock,
            records_root=args.records_root,
        )
    except (EvidenceConvergenceError, OSError) as error:
        print(f"protocol capture admission error: {error}", file=sys.stderr)
        return 1
    print(json.dumps(summary, sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
