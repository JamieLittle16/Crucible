#!/usr/bin/env python3
"""Materialize the finite Minecraft 26.2 Login contract from a plaintext witness capture.

The selected R1A2 path is source-admitted as ordinary TCP with authentication disabled and
compression threshold < 0. Capture bytes provide runtime witnesses only; packet identity, field
interpretation and policy law are fixed by reviewed source records and SEM rules.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path
from typing import Any

from tools import protocol_capture_admission as capture_admission
from tools.protocol_contract import ContractError, validate_contract

SCHEMA = 1
CONTRACT_ID = "PROTO-NET-LOGIN-26-2-001"
TARGET_MINECRAFT = "26.2"
TARGET_PROTOCOL = 776

HANDSHAKE_ID = 0
LOGIN_INTENT = 2
HELLO_ID = 0
LOGIN_FINISHED_ID = 2
LOGIN_ACK_ID = 3

MAX_SERVER_ADDRESS_UTF16_UNITS = 255
MAX_PLAYER_NAME_UTF16_UNITS = 16
MAX_PROPERTY_COUNT = 16
MAX_PROPERTY_NAME_UTF16_UNITS = 64
MAX_PROPERTY_VALUE_UTF16_UNITS = 32_767
MAX_PROPERTY_SIGNATURE_UTF16_UNITS = 1_024

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

HANDSHAKE_RECORDS = [
    "VAR-NET-R0-CLIENT-INTENT-DECL-001",
    "VAR-NET-R0-CLIENT-INTENT-BY-ID-001",
    "VAR-NET-R0-CLIENT-INTENTION-CODEC-001",
    "VAR-NET-R0-HANDSHAKE-DECODE-001",
    "VAR-NET-R0-HANDSHAKE-REGISTRATION-001",
    "VAR-NET-R1A-BEGIN-LOGIN-001",
    *ID_ORDER_RECORDS,
]

HELLO_RECORDS = [
    "VAR-NET-R1A-LOGIN-REGISTRATION-001",
    "VAR-NET-R1A-SERVERBOUND-HELLO-CODEC-001",
    "VAR-NET-R1A-SERVERBOUND-HELLO-DECODE-001",
    "VAR-NET-R1A-LOGIN-HELLO-HANDLER-001",
    "VAR-NET-R1A-UUID-STREAM-DECL-001",
    "VAR-NET-R1A-UUID-READ-001",
    "VAR-NET-R1A-OFFLINE-UUID-001",
    "VAR-NET-R1A-OFFLINE-PROFILE-001",
    *ID_ORDER_RECORDS,
]

LOGIN_FINISHED_RECORDS = [
    "VAR-NET-R1A-LOGIN-REGISTRATION-001",
    "VAR-NET-R1A-LOGIN-FINISHED-CODEC-001",
    "VAR-NET-R1A-LOGIN-FINISHED-HANDLER-001",
    "VAR-NET-R1A-LOGIN-VERIFY-FINISH-001",
    "VAR-NET-R1A-UUID-STREAM-DECL-001",
    "VAR-NET-R1A-UUID-WRITE-001",
    "VAR-NET-R1A-GAME-PROFILE-DECL-001",
    "VAR-NET-R1A-STRING-UTF8-FACTORY-001",
    "VAR-NET-R1A-PROPERTY-COUNT-WRITE-001",
    "VAR-NET-R1A-NULLABLE-WRITE-001",
    "VAR-NET-R1A-COMPOSITE-2-001",
    "VAR-NET-R1A-COMPOSITE-3-001",
    *ID_ORDER_RECORDS,
]

LOGIN_ACK_RECORDS = [
    "VAR-NET-R1A-LOGIN-REGISTRATION-001",
    "VAR-NET-R1A-LOGIN-ACK-CODEC-001",
    "VAR-NET-R1A-LOGIN-ACK-HANDLER-001",
    *ID_ORDER_RECORDS,
]


class R1LoginMaterializationError(ValueError):
    """Raised when a witness capture violates the source-admitted R1A2 Login law."""


def _read_target(lock_path: Path) -> dict[str, object]:
    if lock_path.is_symlink() or not lock_path.is_file():
        raise R1LoginMaterializationError(
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
        raise R1LoginMaterializationError(
            f"could not read target identity from {lock_path}: {error}"
        ) from error
    if target["minecraft"] != TARGET_MINECRAFT or target["protocol"] != TARGET_PROTOCOL:
        raise R1LoginMaterializationError(
            "R1 Login materializer is pinned to Minecraft 26.2 / protocol 776"
        )
    return target


def _encode_var_int(value: int) -> bytes:
    if not 0 <= value <= 0x7FFF_FFFF:
        raise R1LoginMaterializationError(f"non-negative VarInt is out of range: {value}")
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
            raise R1LoginMaterializationError(f"{label} is truncated")
        byte = data[cursor]
        cursor += 1
        value |= (byte & 0x7F) << (7 * index)
        if byte & 0x80 == 0:
            if value > 0x7FFF_FFFF:
                raise R1LoginMaterializationError(
                    f"{label} exceeds non-negative i32 VarInt range"
                )
            if data[start:cursor] != _encode_var_int(value):
                raise R1LoginMaterializationError(f"{label} uses a noncanonical VarInt")
            return value, cursor
    raise R1LoginMaterializationError(f"{label} exceeds the five-byte VarInt bound")


def _read_string(
    data: bytes,
    cursor: int,
    max_utf16_units: int,
    label: str,
) -> tuple[str, int]:
    byte_len, cursor = _read_var_int(data, cursor, f"{label} length")
    max_bytes = max_utf16_units * 3
    if byte_len > max_bytes:
        raise R1LoginMaterializationError(
            f"{label} encoded byte length {byte_len} exceeds source-backed bound {max_bytes}"
        )
    end = cursor + byte_len
    if end > len(data):
        raise R1LoginMaterializationError(f"{label} is truncated")
    try:
        value = data[cursor:end].decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise R1LoginMaterializationError(f"{label} is not valid UTF-8") from error
    utf16_units = len(value.encode("utf-16-le")) // 2
    if utf16_units > max_utf16_units:
        raise R1LoginMaterializationError(
            f"{label} has {utf16_units} UTF-16 units, maximum is {max_utf16_units}"
        )
    return value, end


def _read_uuid(data: bytes, cursor: int, label: str) -> tuple[bytes, int]:
    end = cursor + 16
    if end > len(data):
        raise R1LoginMaterializationError(f"{label} is truncated")
    return data[cursor:end], end


def _java_name_uuid_from_bytes(name: bytes) -> bytes:
    """Equivalent to OpenJDK 25 UUID.nameUUIDFromBytes(byte[])."""
    digest = bytearray(hashlib.md5(name).digest(),)  # noqa: S324 -- required UUIDv3 law
    digest[6] = (digest[6] & 0x0F) | 0x30
    digest[8] = (digest[8] & 0x3F) | 0x80
    return bytes(digest)


def offline_player_uuid(player_name: str) -> bytes:
    """Return the source/JDK-defined offline-player UUID bytes for one player name."""
    return _java_name_uuid_from_bytes(("OfflinePlayer:" + player_name).encode("utf-8"))


def _uuid_text(value: bytes) -> str:
    if len(value) != 16:
        raise AssertionError("UUID byte projection must be exactly 16 bytes")
    hexed = value.hex()
    return (
        f"{hexed[0:8]}-{hexed[8:12]}-{hexed[12:16]}-"
        f"{hexed[16:20]}-{hexed[20:32]}"
    )


def _payload(body: bytes, expected_id: int, label: str) -> bytes:
    packet_id, cursor = _read_var_int(body, 0, f"{label} packet id")
    if packet_id != expected_id:
        raise R1LoginMaterializationError(
            f"{label} packet id is {packet_id}, source-backed identity requires {expected_id}"
        )
    return body[cursor:]


def _check_handshake(body: bytes) -> None:
    payload = _payload(body, HANDSHAKE_ID, "Login handshake")
    cursor = 0
    protocol, cursor = _read_var_int(payload, cursor, "handshake protocol version")
    if protocol != TARGET_PROTOCOL:
        raise R1LoginMaterializationError(
            f"Login handshake protocol is {protocol}, expected {TARGET_PROTOCOL}"
        )
    _, cursor = _read_string(
        payload,
        cursor,
        MAX_SERVER_ADDRESS_UTF16_UNITS,
        "handshake server address",
    )
    if cursor + 2 > len(payload):
        raise R1LoginMaterializationError("handshake server port is truncated")
    cursor += 2
    intent, cursor = _read_var_int(payload, cursor, "handshake client intent")
    if intent != LOGIN_INTENT:
        raise R1LoginMaterializationError(
            f"handshake intent is {intent}, Login intent must be {LOGIN_INTENT}"
        )
    if cursor != len(payload):
        raise R1LoginMaterializationError("Login handshake has trailing payload bytes")


def _check_hello(body: bytes) -> tuple[str, bytes]:
    payload = _payload(body, HELLO_ID, "serverbound hello")
    name, cursor = _read_string(
        payload,
        0,
        MAX_PLAYER_NAME_UTF16_UNITS,
        "serverbound hello player name",
    )
    client_uuid, cursor = _read_uuid(payload, cursor, "serverbound hello UUID")
    if cursor != len(payload):
        raise R1LoginMaterializationError("serverbound hello has trailing payload bytes")
    return name, client_uuid


def _read_bool(data: bytes, cursor: int, label: str) -> tuple[bool, int]:
    if cursor >= len(data):
        raise R1LoginMaterializationError(f"{label} is truncated")
    value = data[cursor]
    if value not in (0, 1):
        raise R1LoginMaterializationError(
            f"{label} must use canonical boolean byte 0 or 1, got {value}"
        )
    return value == 1, cursor + 1


def _check_login_finished(body: bytes) -> dict[str, object]:
    payload = _payload(body, LOGIN_FINISHED_ID, "clientbound login_finished")
    profile_uuid, cursor = _read_uuid(payload, 0, "login_finished profile UUID")
    profile_name, cursor = _read_string(
        payload,
        cursor,
        MAX_PLAYER_NAME_UTF16_UNITS,
        "login_finished profile name",
    )
    property_count, cursor = _read_var_int(
        payload, cursor, "login_finished profile property count"
    )
    if property_count > MAX_PROPERTY_COUNT:
        raise R1LoginMaterializationError(
            f"login_finished profile has {property_count} properties, maximum is "
            f"{MAX_PROPERTY_COUNT}"
        )
    properties: list[dict[str, object]] = []
    for index in range(property_count):
        name, cursor = _read_string(
            payload,
            cursor,
            MAX_PROPERTY_NAME_UTF16_UNITS,
            f"login_finished property[{index}].name",
        )
        value, cursor = _read_string(
            payload,
            cursor,
            MAX_PROPERTY_VALUE_UTF16_UNITS,
            f"login_finished property[{index}].value",
        )
        present, cursor = _read_bool(
            payload, cursor, f"login_finished property[{index}].signature.present"
        )
        signature: str | None = None
        if present:
            signature, cursor = _read_string(
                payload,
                cursor,
                MAX_PROPERTY_SIGNATURE_UTF16_UNITS,
                f"login_finished property[{index}].signature",
            )
        properties.append({"name": name, "value": value, "signature": signature})

    session_uuid, cursor = _read_uuid(payload, cursor, "login_finished session UUID")
    if cursor != len(payload):
        raise R1LoginMaterializationError(
            "clientbound login_finished has trailing payload bytes"
        )
    return {
        "profile_uuid": profile_uuid,
        "profile_name": profile_name,
        "properties": properties,
        "session_uuid": session_uuid,
    }


def _check_login_ack(body: bytes) -> None:
    payload = _payload(body, LOGIN_ACK_ID, "serverbound login_acknowledged")
    if payload:
        raise R1LoginMaterializationError(
            "serverbound login_acknowledged must have an empty payload"
        )


def _unique(items: list[str]) -> list[str]:
    result: list[str] = []
    seen: set[str] = set()
    for item in items:
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
        "source_records": _unique(records),
        "golden": {
            "body_hex": body.hex(),
            "frame_hex": (_encode_var_int(len(body)) + body).hex(),
        },
    }


def _capture_prefix(
    capture_path: Path,
    expected_target: dict[str, object],
) -> tuple[str, tuple[bytes, bytes, bytes], bytes, dict[str, int]]:
    try:
        capture_sha, streams = capture_admission._validate_capture(  # noqa: SLF001
            capture_path,
            expected_target=expected_target,
        )
    except capture_admission.EvidenceConvergenceError as error:
        raise R1LoginMaterializationError(
            f"protocol capture failed artifact validation: {error}"
        ) from error

    client = streams["client-to-server"]
    server = streams["server-to-client"]
    if len(client) < 3:
        raise R1LoginMaterializationError(
            f"R1A2 Login requires at least 3 client-to-server frames, capture has {len(client)}"
        )
    if len(server) < 1:
        raise R1LoginMaterializationError(
            "R1A2 Login requires at least 1 server-to-client frame"
        )

    handshake = client[0][1]
    hello = client[1][1]
    ack = client[2][1]
    finished = server[0][1]
    extras = {
        "client_to_server_after_login": len(client) - 3,
        "server_to_client_after_login_finished": len(server) - 1,
    }
    return capture_sha, (handshake, hello, ack), finished, extras


def build_contract(
    capture_path: Path,
    *,
    lock_path: Path,
) -> tuple[dict[str, object], dict[str, object]]:
    """Build one source-interpreted finite Login contract from an opaque witness capture."""
    target = _read_target(lock_path)
    capture_sha, client, login_finished, extras = _capture_prefix(capture_path, target)
    handshake, hello, ack = client

    _check_handshake(handshake)
    player_name, client_uuid = _check_hello(hello)
    finished = _check_login_finished(login_finished)
    _check_login_ack(ack)

    expected_offline_uuid = offline_player_uuid(player_name)
    if finished["profile_name"] != player_name:
        raise R1LoginMaterializationError(
            "login_finished profile name does not equal serverbound hello player name"
        )
    if finished["profile_uuid"] != expected_offline_uuid:
        raise R1LoginMaterializationError(
            "login_finished profile UUID does not equal the Java-25/source-defined offline UUID "
            f"for {player_name!r}: expected {_uuid_text(expected_offline_uuid)}, "
            f"captured {_uuid_text(finished['profile_uuid'])}"
        )

    contract = {
        "schema": SCHEMA,
        "id": CONTRACT_ID,
        "target": target,
        "packets": [
            _packet(
                name="client-intention-login",
                phase="handshake",
                direction="serverbound",
                packet_id=HANDSHAKE_ID,
                rules=[
                    "SEM-NET-R0-001",
                    "SEM-NET-R0-002",
                    "SEM-NET-R0-004",
                    "SEM-NET-R0-014",
                    "SEM-NET-R0-015",
                    "SEM-NET-R1A-001",
                ],
                records=HANDSHAKE_RECORDS,
                body=handshake,
            ),
            _packet(
                name="login-hello",
                phase="login",
                direction="serverbound",
                packet_id=HELLO_ID,
                rules=[
                    "SEM-NET-R1A-002",
                    "SEM-NET-R1A-003",
                    "SEM-NET-R1A-008",
                    "SEM-NET-R1A-009",
                    "SEM-NET-R1A-010",
                ],
                records=HELLO_RECORDS,
                body=hello,
            ),
            _packet(
                name="login-finished",
                phase="login",
                direction="clientbound",
                packet_id=LOGIN_FINISHED_ID,
                rules=[
                    "SEM-NET-R1A-002",
                    "SEM-NET-R1A-006",
                    "SEM-NET-R1A-008",
                    "SEM-NET-R1A-009",
                    "SEM-NET-R1A-011",
                    "SEM-NET-R1A-012",
                ],
                records=LOGIN_FINISHED_RECORDS,
                body=login_finished,
            ),
            _packet(
                name="login-acknowledged",
                phase="login",
                direction="serverbound",
                packet_id=LOGIN_ACK_ID,
                rules=["SEM-NET-R1A-002", "SEM-NET-R1A-007"],
                records=LOGIN_ACK_RECORDS,
                body=ack,
            ),
        ],
    }
    witness = {
        "schema": 1,
        "kind": "r1-login-witness-v1",
        "contract_id": CONTRACT_ID,
        "capture_sha256": capture_sha,
        "player_name": player_name,
        "client_hello_uuid": _uuid_text(client_uuid),
        "offline_profile_uuid": _uuid_text(expected_offline_uuid),
        "session_uuid": _uuid_text(finished["session_uuid"]),
        "profile_property_count": len(finished["properties"]),
        "uninterpreted_post_login_frames": extras,
    }
    return contract, witness


def materialize(
    capture_path: Path,
    *,
    output_path: Path,
    witness_output_path: Path | None,
    lock_path: Path,
    records_root: Path,
) -> dict[str, object]:
    """Write, validate and summarize one finite R1A2 Login contract."""
    contract, witness = build_contract(capture_path, lock_path=lock_path)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(
        json.dumps(contract, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    if witness_output_path is not None:
        witness_output_path.parent.mkdir(parents=True, exist_ok=True)
        witness_output_path.write_text(
            json.dumps(witness, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    try:
        admission = validate_contract(
            output_path,
            lock_path=lock_path,
            records_root=records_root,
        )
    except ContractError as error:
        output_path.unlink(missing_ok=True)
        if witness_output_path is not None:
            witness_output_path.unlink(missing_ok=True)
        raise R1LoginMaterializationError(
            f"materialized Login contract failed source admission: {error}"
        ) from error

    return {
        "schema": 1,
        "contract_id": CONTRACT_ID,
        "capture_sha256": witness["capture_sha256"],
        "minecraft": admission["minecraft"],
        "protocol": admission["protocol"],
        "packets": admission["packets"],
        "player_name": witness["player_name"],
        "offline_profile_uuid": witness["offline_profile_uuid"],
        "session_uuid": witness["session_uuid"],
        "profile_property_count": witness["profile_property_count"],
        "uninterpreted_post_login_frames": witness["uninterpreted_post_login_frames"],
    }


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--witness-output", type=Path)
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--records-root", type=Path, default=Path("vanilla/records"))
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    try:
        report = materialize(
            args.capture,
            output_path=args.output,
            witness_output_path=args.witness_output,
            lock_path=args.lock,
            records_root=args.records_root,
        )
    except R1LoginMaterializationError as error:
        print(f"R1 Login materialization error: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
