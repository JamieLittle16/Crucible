#!/usr/bin/env python3
"""Materialize the finite Minecraft 26.2 R0 status contract from one black-box witness capture.

Capture bytes are used only as witness values for runtime-varying data. Packet identity and
semantic interpretation remain fixed by the reviewed 26.2 source records. Before an output is
committed, this tool independently checks the source-backed R0 field laws, derives the expected
pong from the captured ping payload, validates the resulting finite protocol contract, and runs the
existing P0L capture convergence gate.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
import tomllib
from pathlib import Path
from typing import Any

from tools.protocol_capture_admission import EvidenceConvergenceError, crosscheck_capture
from tools.protocol_contract import ContractError, validate_contract

CONTRACT_SCHEMA = 1
CONTRACT_ID = "PROTO-NET-STATUS-26-2-001"
TARGET_MINECRAFT = "26.2"
TARGET_PROTOCOL = 776
STATUS_INTENT = 1
HANDSHAKE_ID = 0
STATUS_REQUEST_ID = 0
PING_REQUEST_ID = 1
STATUS_RESPONSE_ID = 0
PONG_RESPONSE_ID = 1
HANDSHAKE_ADDRESS_UNITS = 255
STATUS_JSON_UNITS = 32_767

ID_ORDER_RECORDS = [
    "VAR-NET-R0-PROTOCOL-BUILDER-ADD-001",
    "VAR-NET-R0-PROTOCOL-BUILDER-CODEC-001",
    "VAR-NET-R0-PROTOCOL-BUILDER-ENTRY-001",
    "VAR-NET-R0-PROTOCOL-CODEC-ADD-001",
    "VAR-NET-R0-PROTOCOL-CODEC-BUILD-001",
    "VAR-NET-R0-ID-DISPATCH-BUILDER-ADD-001",
    "VAR-NET-R0-ID-DISPATCH-BUILDER-BUILD-001",
    "VAR-NET-R0-ID-DISPATCH-DECODE-001",
    "VAR-NET-R0-ID-DISPATCH-ENCODE-001",
]


class R0MaterializationError(ValueError):
    """Raised when a witness capture does not satisfy the source-backed R0 status law."""


def _read_json(path: Path, label: str) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise R0MaterializationError(f"{label} must be a real non-symlink file: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise R0MaterializationError(f"could not read {label} {path}: {error}") from error
    if not isinstance(value, dict):
        raise R0MaterializationError(f"{label} must be a JSON object")
    return value


def _read_target(lock_path: Path) -> dict[str, object]:
    if lock_path.is_symlink() or not lock_path.is_file():
        raise R0MaterializationError(
            f"vanilla lock must be a real non-symlink file: {lock_path}"
        )
    try:
        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
        source = lock["source"]
        atlas = lock["atlas"]
        target = {
            "minecraft": lock["minecraft"],
            "protocol": lock["protocol"],
            "source_archive_sha256": source["archive_sha256"],
            "fingerprint_algorithm": atlas["fingerprint_algorithm"],
        }
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError, KeyError, TypeError) as error:
        raise R0MaterializationError(f"could not read target identity from {lock_path}: {error}") from error
    if target["minecraft"] != TARGET_MINECRAFT or target["protocol"] != TARGET_PROTOCOL:
        raise R0MaterializationError(
            "R0 materializer is pinned to Minecraft 26.2 / protocol 776"
        )
    return target


def _encode_var_int(value: int) -> bytes:
    if not 0 <= value <= 0x7FFF_FFFF:
        raise R0MaterializationError(f"non-negative VarInt is out of range: {value}")
    output = bytearray()
    remaining = value
    while True:
        byte = remaining & 0x7F
        remaining >>= 7
        if remaining:
            output.append(byte | 0x80)
        else:
            output.append(byte)
            return bytes(output)


def _read_var_int(data: bytes, cursor: int, label: str) -> tuple[int, int]:
    value = 0
    start = cursor
    for index in range(5):
        if cursor >= len(data):
            raise R0MaterializationError(f"{label} is truncated")
        byte = data[cursor]
        cursor += 1
        value |= (byte & 0x7F) << (index * 7)
        if byte & 0x80 == 0:
            if value > 0x7FFF_FFFF:
                raise R0MaterializationError(f"{label} exceeds non-negative i32 range")
            if data[start:cursor] != _encode_var_int(value):
                raise R0MaterializationError(f"{label} uses a noncanonical VarInt")
            return value, cursor
    raise R0MaterializationError(f"{label} exceeds the five-byte VarInt bound")


def _read_string(data: bytes, cursor: int, max_utf16_units: int, label: str) -> tuple[str, int]:
    byte_len, cursor = _read_var_int(data, cursor, f"{label} length")
    max_bytes = max_utf16_units * 3
    if byte_len > max_bytes:
        raise R0MaterializationError(
            f"{label} encoded byte length {byte_len} exceeds source-backed bound {max_bytes}"
        )
    end = cursor + byte_len
    if end > len(data):
        raise R0MaterializationError(f"{label} is truncated")
    try:
        value = data[cursor:end].decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise R0MaterializationError(f"{label} is not valid UTF-8") from error
    utf16_units = len(value.encode("utf-16-le")) // 2
    if utf16_units > max_utf16_units:
        raise R0MaterializationError(
            f"{label} has {utf16_units} UTF-16 units, maximum is {max_utf16_units}"
        )
    return value, end


def _decode_hex(value: object, label: str) -> bytes:
    if not isinstance(value, str) or len(value) % 2 != 0:
        raise R0MaterializationError(f"{label} must be even-length hexadecimal text")
    try:
        return bytes.fromhex(value)
    except ValueError as error:
        raise R0MaterializationError(f"{label} is not valid hexadecimal") from error


def _canonical_frame(body: bytes) -> bytes:
    return _encode_var_int(len(body)) + body


def _payload(body: bytes, expected_id: int, label: str) -> bytes:
    packet_id, cursor = _read_var_int(body, 0, f"{label} packet id")
    if packet_id != expected_id:
        raise R0MaterializationError(
            f"{label} packet id is {packet_id}, source-backed identity requires {expected_id}"
        )
    return body[cursor:]


def _stream_frames(capture: dict[str, Any], direction: str, expected: int) -> list[dict[str, Any]]:
    streams = capture.get("streams")
    if not isinstance(streams, list):
        raise R0MaterializationError("capture streams must be an array")
    matches = [
        stream
        for stream in streams
        if isinstance(stream, dict) and stream.get("direction") == direction
    ]
    if len(matches) != 1:
        raise R0MaterializationError(
            f"capture must contain exactly one {direction!r} stream"
        )
    frames = matches[0].get("frames")
    if not isinstance(frames, list) or len(frames) != expected:
        actual = len(frames) if isinstance(frames, list) else "non-array"
        raise R0MaterializationError(
            f"R0 requires exactly {expected} {direction} frames, capture has {actual}"
        )
    for index, frame in enumerate(frames):
        if not isinstance(frame, dict):
            raise R0MaterializationError(f"{direction} frame[{index}] must be an object")
    return frames


def _frame_body(frame: dict[str, Any], label: str) -> bytes:
    return _decode_hex(frame.get("body_hex"), f"{label}.body_hex")


def _check_handshake(body: bytes, expected_protocol: int) -> None:
    payload = _payload(body, HANDSHAKE_ID, "handshake client intention")
    cursor = 0
    protocol, cursor = _read_var_int(payload, cursor, "handshake protocol version")
    if protocol != expected_protocol:
        raise R0MaterializationError(
            f"handshake protocol is {protocol}, expected pinned protocol {expected_protocol}"
        )
    _, cursor = _read_string(
        payload, cursor, HANDSHAKE_ADDRESS_UNITS, "handshake server address"
    )
    if cursor + 2 > len(payload):
        raise R0MaterializationError("handshake server port is truncated")
    cursor += 2
    intent, cursor = _read_var_int(payload, cursor, "handshake client intent")
    if intent != STATUS_INTENT:
        raise R0MaterializationError(
            f"handshake intent is {intent}, R0 status intent must be {STATUS_INTENT}"
        )
    if cursor != len(payload):
        raise R0MaterializationError("handshake client intention has trailing payload bytes")


def _check_status_request(body: bytes) -> None:
    payload = _payload(body, STATUS_REQUEST_ID, "status request")
    if payload:
        raise R0MaterializationError("status request must have an empty payload")


def _check_ping(body: bytes) -> bytes:
    payload = _payload(body, PING_REQUEST_ID, "ping request")
    if len(payload) != 8:
        raise R0MaterializationError(
            f"ping request payload must be exactly 8 bytes, got {len(payload)}"
        )
    return payload


def _check_status_response(body: bytes, expected_protocol: int) -> None:
    payload = _payload(body, STATUS_RESPONSE_ID, "status response")
    text, cursor = _read_string(payload, 0, STATUS_JSON_UNITS, "status response JSON")
    if cursor != len(payload):
        raise R0MaterializationError("status response has trailing payload bytes")
    try:
        value = json.loads(text)
    except json.JSONDecodeError as error:
        raise R0MaterializationError("status response string is not valid JSON") from error
    if not isinstance(value, dict):
        raise R0MaterializationError("status response JSON must be an object")
    allowed = {"description", "players", "version", "favicon", "enforcesSecureChat"}
    unknown = sorted(set(value) - allowed)
    if unknown:
        raise R0MaterializationError(
            "status response contains fields outside the reviewed ServerStatus codec: "
            + ", ".join(unknown)
        )

    version = value.get("version")
    if not isinstance(version, dict):
        raise R0MaterializationError(
            "controlled 26.2 oracle status response must contain a version object"
        )
    name = version.get("name")
    protocol = version.get("protocol")
    if not isinstance(name, str) or not name:
        raise R0MaterializationError("status version.name must be a non-empty string")
    if type(protocol) is not int or protocol != expected_protocol:
        raise R0MaterializationError(
            f"status version.protocol must be {expected_protocol}, got {protocol!r}"
        )

    players = value.get("players")
    if players is not None:
        if not isinstance(players, dict):
            raise R0MaterializationError("status players must be an object when present")
        if type(players.get("max")) is not int or type(players.get("online")) is not int:
            raise R0MaterializationError("status players requires integer max and online fields")
        if "sample" in players and not isinstance(players["sample"], list):
            raise R0MaterializationError("status players.sample must be an array when present")

    if "favicon" in value and not isinstance(value["favicon"], str):
        raise R0MaterializationError("status favicon must be a string when present")
    if "enforcesSecureChat" in value and type(value["enforcesSecureChat"]) is not bool:
        raise R0MaterializationError(
            "status enforcesSecureChat must be a boolean when present"
        )


def _check_pong(body: bytes, ping_payload: bytes) -> bytes:
    expected = _encode_var_int(PONG_RESPONSE_ID) + ping_payload
    if body != expected:
        raise R0MaterializationError(
            "pong response is not the source-backed exact 64-bit echo of the ping request"
        )
    return expected


def _unique(*groups: list[str]) -> list[str]:
    result: list[str] = []
    seen: set[str] = set()
    for group in groups:
        for item in group:
            if item not in seen:
                seen.add(item)
                result.append(item)
    return result


def _packet(
    *,
    name: str,
    phase: str,
    direction: str,
    packet_id: int,
    rules: list[str],
    records: list[str],
    body: bytes,
) -> dict[str, object]:
    return {
        "name": name,
        "phase": phase,
        "direction": direction,
        "id": packet_id,
        "semantic_rules": rules,
        "source_records": records,
        "golden": {
            "body_hex": body.hex(),
            "frame_hex": _canonical_frame(body).hex(),
        },
    }


def build_contract(capture_path: Path, *, lock_path: Path) -> dict[str, object]:
    """Build one source-interpreted R0 contract in memory from an opaque witness capture."""
    target = _read_target(lock_path)
    capture = _read_json(capture_path, "protocol capture")
    if capture.get("target") != target:
        raise R0MaterializationError("capture target identity does not match vanilla lock")

    client = _stream_frames(capture, "client-to-server", 3)
    server = _stream_frames(capture, "server-to-client", 2)

    handshake = _frame_body(client[0], "client-to-server frame[0]")
    status_request = _frame_body(client[1], "client-to-server frame[1]")
    ping_request = _frame_body(client[2], "client-to-server frame[2]")
    status_response = _frame_body(server[0], "server-to-client frame[0]")
    captured_pong = _frame_body(server[1], "server-to-client frame[1]")

    _check_handshake(handshake, TARGET_PROTOCOL)
    _check_status_request(status_request)
    ping_payload = _check_ping(ping_request)
    _check_status_response(status_response, TARGET_PROTOCOL)
    pong_response = _check_pong(captured_pong, ping_payload)

    id_records = ID_ORDER_RECORDS
    packets = [
        _packet(
            name="client-intention",
            phase="handshake",
            direction="serverbound",
            packet_id=HANDSHAKE_ID,
            rules=[
                "SEM-NET-R0-001",
                "SEM-NET-R0-002",
                "SEM-NET-R0-003",
                "SEM-NET-R0-004",
                "SEM-NET-R0-014",
                "SEM-NET-R0-015",
            ],
            records=_unique(
                [
                    "VAR-NET-R0-CLIENT-INTENT-DECL-001",
                    "VAR-NET-R0-CLIENT-INTENT-BY-ID-001",
                    "VAR-NET-R0-CLIENT-INTENTION-CODEC-001",
                    "VAR-NET-R0-HANDSHAKE-DECODE-001",
                    "VAR-NET-R0-HANDSHAKE-REGISTRATION-001",
                    "VAR-NET-R0-HANDSHAKE-STATUS-TRANSITION-001",
                ],
                id_records,
            ),
            body=handshake,
        ),
        _packet(
            name="status-request",
            phase="status",
            direction="serverbound",
            packet_id=STATUS_REQUEST_ID,
            rules=[
                "SEM-NET-R0-005",
                "SEM-NET-R0-006",
                "SEM-NET-R0-010",
                "SEM-NET-R0-014",
                "SEM-NET-R0-015",
            ],
            records=_unique(
                [
                    "VAR-NET-R0-STATUS-REGISTRATION-001",
                    "VAR-NET-R0-STATUS-REQUEST-CODEC-001",
                    "VAR-NET-R0-STATUS-REQUEST-HANDLER-001",
                ],
                id_records,
            ),
            body=status_request,
        ),
        _packet(
            name="ping-request",
            phase="status",
            direction="serverbound",
            packet_id=PING_REQUEST_ID,
            rules=[
                "SEM-NET-R0-005",
                "SEM-NET-R0-011",
                "SEM-NET-R0-013",
                "SEM-NET-R0-014",
                "SEM-NET-R0-015",
            ],
            records=_unique(
                [
                    "VAR-NET-R0-STATUS-REGISTRATION-001",
                    "VAR-NET-R0-PING-REQUEST-CODEC-001",
                    "VAR-NET-R0-PING-DECODE-001",
                    "VAR-NET-R0-PING-HANDLER-001",
                ],
                id_records,
            ),
            body=ping_request,
        ),
        _packet(
            name="status-response",
            phase="status",
            direction="clientbound",
            packet_id=STATUS_RESPONSE_ID,
            rules=[
                "SEM-NET-R0-005",
                "SEM-NET-R0-007",
                "SEM-NET-R0-008",
                "SEM-NET-R0-009",
                "SEM-NET-R0-010",
                "SEM-NET-R0-014",
                "SEM-NET-R0-015",
            ],
            records=_unique(
                [
                    "VAR-NET-R0-STATUS-REGISTRATION-001",
                    "VAR-NET-R0-STATUS-RESPONSE-CODEC-001",
                    "VAR-NET-R0-SERVER-STATUS-CODEC-001",
                    "VAR-NET-R0-SERVER-STATUS-PLAYERS-CODEC-001",
                    "VAR-NET-R0-SERVER-STATUS-VERSION-CODEC-001",
                    "VAR-NET-R0-STATUS-REQUEST-HANDLER-001",
                ],
                id_records,
            ),
            body=status_response,
        ),
        _packet(
            name="pong-response",
            phase="status",
            direction="clientbound",
            packet_id=PONG_RESPONSE_ID,
            rules=[
                "SEM-NET-R0-005",
                "SEM-NET-R0-012",
                "SEM-NET-R0-013",
                "SEM-NET-R0-014",
                "SEM-NET-R0-015",
            ],
            records=_unique(
                [
                    "VAR-NET-R0-STATUS-REGISTRATION-001",
                    "VAR-NET-R0-PONG-RESPONSE-CODEC-001",
                    "VAR-NET-R0-PONG-ENCODE-001",
                    "VAR-NET-R0-PING-HANDLER-001",
                ],
                id_records,
            ),
            body=pong_response,
        ),
    ]

    return {
        "schema": CONTRACT_SCHEMA,
        "id": CONTRACT_ID,
        "target": target,
        "packets": packets,
    }


def materialize(
    capture_path: Path,
    *,
    lock_path: Path,
    records_root: Path,
    output_path: Path,
) -> dict[str, object]:
    """Write the contract only after source validation and P0L convergence both succeed."""
    contract = build_contract(capture_path, lock_path=lock_path)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    if output_path.exists() and output_path.is_symlink():
        raise R0MaterializationError(f"refusing to replace symlink output: {output_path}")

    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=output_path.parent,
            prefix=f".{output_path.name}.",
            suffix=".tmp",
            delete=False,
        ) as handle:
            temporary = Path(handle.name)
            json.dump(contract, handle, indent=2, sort_keys=True)
            handle.write("\n")

        admission = validate_contract(
            temporary,
            lock_path=lock_path,
            records_root=records_root,
        )
        convergence = crosscheck_capture(
            temporary,
            capture_path,
            lock_path=lock_path,
            records_root=records_root,
        )
        os.replace(temporary, output_path)
        temporary = None
        return {
            "contract": admission,
            "convergence": convergence,
            "output": str(output_path),
        }
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--records-root", type=Path, default=Path("vanilla/records"))
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    try:
        result = materialize(
            args.capture,
            lock_path=args.lock,
            records_root=args.records_root,
            output_path=args.output,
        )
    except (R0MaterializationError, ContractError, EvidenceConvergenceError, OSError) as error:
        print(f"R0 status contract error: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
