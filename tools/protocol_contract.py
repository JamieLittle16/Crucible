#!/usr/bin/env python3
"""Validate finite protocol-contract evidence against Crucible's pinned source identity."""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any

SCHEMA = 1
HEX_64 = re.compile(r"^[0-9a-f]{64}$")
HEX_BYTES = re.compile(r"^(?:[0-9a-f]{2})*$")
SEM_ID = re.compile(r"^SEM-[A-Z0-9][A-Z0-9-]*$")
VAR_ID = re.compile(r"^VAR-[A-Z0-9][A-Z0-9-]*$")
CONTRACT_ID = re.compile(r"^PROTO-[A-Z0-9][A-Z0-9-]*$")
PACKET_NAME = re.compile(r"^[a-z][a-z0-9-]*$")
PHASES = {"handshake", "status", "login", "configuration", "play"}
DIRECTIONS = {"serverbound", "clientbound"}


class ContractError(ValueError):
    """Raised when a protocol-contract artifact fails admission."""


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ContractError(f"{label} must be an object")
    return value


def _keys(
    value: dict[str, Any], *, allowed: set[str], required: set[str], label: str
) -> None:
    unknown = sorted(set(value) - allowed)
    missing = sorted(required - set(value))
    if unknown:
        raise ContractError(f"{label} contains unknown keys: {', '.join(unknown)}")
    if missing:
        raise ContractError(f"{label} is missing required keys: {', '.join(missing)}")


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ContractError(f"{label} must be a non-empty string")
    return value


def _integer(value: object, label: str) -> int:
    if type(value) is not int:
        raise ContractError(f"{label} must be an integer")
    return value


def _string_list(value: object, label: str, pattern: re.Pattern[str]) -> tuple[str, ...]:
    if not isinstance(value, list) or not value:
        raise ContractError(f"{label} must be a non-empty array")
    result: list[str] = []
    for index, item in enumerate(value):
        text = _string(item, f"{label}[{index}]")
        if pattern.fullmatch(text) is None:
            raise ContractError(f"{label}[{index}] is not canonical: {text!r}")
        result.append(text)
    if len(set(result)) != len(result):
        raise ContractError(f"{label} must not contain duplicates")
    return tuple(result)


def _read_json(path: Path, label: str) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise ContractError(f"{label} must be a real non-symlink file: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ContractError(f"could not read {label} {path}: {error}") from error
    return _object(value, label)


def _read_lock(path: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise ContractError(f"vanilla lock must be a real non-symlink file: {path}")
    try:
        value = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise ContractError(f"could not read vanilla lock {path}: {error}") from error
    lock = _object(value, "vanilla lock")
    for key in ("minecraft", "protocol", "source", "atlas"):
        if key not in lock:
            raise ContractError(f"vanilla lock is missing {key!r}")
    source = _object(lock["source"], "vanilla lock [source]")
    atlas = _object(lock["atlas"], "vanilla lock [atlas]")
    archive_sha256 = _string(source.get("archive_sha256"), "source.archive_sha256")
    if HEX_64.fullmatch(archive_sha256) is None:
        raise ContractError("source.archive_sha256 must be lowercase SHA-256")
    return {
        "minecraft": _string(lock["minecraft"], "minecraft"),
        "protocol": _integer(lock["protocol"], "protocol"),
        "source_archive_sha256": archive_sha256,
        "fingerprint_algorithm": _string(
            atlas.get("fingerprint_algorithm"), "atlas.fingerprint_algorithm"
        ),
    }


def _encode_var_int(value: int) -> bytes:
    if not 0 <= value <= 0x7FFF_FFFF:
        raise ContractError(f"non-negative VarInt is out of range: {value}")
    remaining = value
    encoded = bytearray()
    while True:
        byte = remaining & 0x7F
        remaining >>= 7
        if remaining:
            encoded.append(byte | 0x80)
        else:
            encoded.append(byte)
            return bytes(encoded)


def _decode_nonnegative_var_int(data: bytes, label: str) -> tuple[int, int]:
    value = 0
    for index in range(min(len(data), 5)):
        byte = data[index]
        value |= (byte & 0x7F) << (7 * index)
        if byte & 0x80 == 0:
            consumed = index + 1
            if value > 0x7FFF_FFFF:
                raise ContractError(f"{label} exceeds non-negative i32 VarInt range")
            if data[:consumed] != _encode_var_int(value):
                raise ContractError(f"{label} uses a noncanonical VarInt encoding")
            return value, consumed
    if len(data) < 5:
        raise ContractError(f"{label} is truncated")
    raise ContractError(f"{label} exceeds the five-byte VarInt bound")


def _hex_bytes(value: object, label: str) -> bytes:
    text = _string(value, label)
    if HEX_BYTES.fullmatch(text) is None:
        raise ContractError(f"{label} must be canonical lowercase hexadecimal bytes")
    return bytes.fromhex(text)


def _load_records(records_root: Path) -> dict[str, dict[str, Any]]:
    if records_root.is_symlink() or not records_root.is_dir():
        raise ContractError(f"records root must be a real directory: {records_root}")
    result: dict[str, dict[str, Any]] = {}
    for path in sorted(records_root.rglob("*.json")):
        if path.is_symlink():
            raise ContractError(f"review record must not be a symlink: {path}")
        record = _read_json(path, "review record")
        record_id = _string(record.get("id"), f"review record {path}: id")
        if VAR_ID.fullmatch(record_id) is None:
            raise ContractError(f"review record has noncanonical id: {record_id!r}")
        if record_id in result:
            raise ContractError(f"duplicate review record id: {record_id}")
        result[record_id] = record
    return result


def _validate_record(
    record_id: str,
    record: dict[str, Any],
    *,
    fingerprint_algorithm: str,
) -> set[str]:
    if _integer(record.get("schema"), f"{record_id}: schema") != 1:
        raise ContractError(f"{record_id} uses an unsupported review-record schema")
    if record.get("status") != "VAR_REVIEWED":
        raise ContractError(f"{record_id} is not VAR_REVIEWED")
    source = _object(record.get("source"), f"{record_id}: source")
    if source.get("fingerprint_algorithm") != fingerprint_algorithm:
        raise ContractError(f"{record_id} fingerprint algorithm does not match vanilla lock")
    for digest_name in ("normalized_sha256", "body_sha256"):
        digest = _string(source.get(digest_name), f"{record_id}: source.{digest_name}")
        if HEX_64.fullmatch(digest) is None:
            raise ContractError(f"{record_id} source.{digest_name} is not lowercase SHA-256")
    rules = record.get("semantic_rules")
    if not isinstance(rules, list):
        raise ContractError(f"{record_id}: semantic_rules must be an array")
    semantic_rules: set[str] = set()
    for index, value in enumerate(rules):
        rule = _string(value, f"{record_id}: semantic_rules[{index}]")
        if SEM_ID.fullmatch(rule) is None:
            raise ContractError(f"{record_id} contains noncanonical SEM id {rule!r}")
        semantic_rules.add(rule)
    return semantic_rules


def _validate_packet(
    packet: dict[str, Any],
    *,
    index: int,
    records: dict[str, dict[str, Any]],
    fingerprint_algorithm: str,
) -> tuple[str, tuple[str, str, int]]:
    label = f"packets[{index}]"
    _keys(
        packet,
        allowed={
            "name",
            "phase",
            "direction",
            "id",
            "semantic_rules",
            "source_records",
            "golden",
        },
        required={
            "name",
            "phase",
            "direction",
            "id",
            "semantic_rules",
            "source_records",
            "golden",
        },
        label=label,
    )
    name = _string(packet["name"], f"{label}.name")
    if PACKET_NAME.fullmatch(name) is None:
        raise ContractError(f"{label}.name is not canonical: {name!r}")
    phase = _string(packet["phase"], f"{label}.phase")
    if phase not in PHASES:
        raise ContractError(f"{label}.phase must be one of {sorted(PHASES)}")
    direction = _string(packet["direction"], f"{label}.direction")
    if direction not in DIRECTIONS:
        raise ContractError(f"{label}.direction must be one of {sorted(DIRECTIONS)}")
    packet_id = _integer(packet["id"], f"{label}.id")
    if not 0 <= packet_id <= 0x7FFF_FFFF:
        raise ContractError(f"{label}.id must fit a non-negative i32 VarInt")

    semantic_rules = _string_list(
        packet["semantic_rules"], f"{label}.semantic_rules", SEM_ID
    )
    source_records = _string_list(
        packet["source_records"], f"{label}.source_records", VAR_ID
    )
    supported_rules: set[str] = set()
    for record_id in source_records:
        record = records.get(record_id)
        if record is None:
            raise ContractError(f"{label} cites missing source record {record_id}")
        supported_rules.update(
            _validate_record(
                record_id,
                record,
                fingerprint_algorithm=fingerprint_algorithm,
            )
        )
    unsupported = sorted(set(semantic_rules) - supported_rules)
    if unsupported:
        raise ContractError(
            f"{label} semantic rules are not linked by cited VAR records: {', '.join(unsupported)}"
        )

    golden = _object(packet["golden"], f"{label}.golden")
    _keys(
        golden,
        allowed={"body_hex", "frame_hex"},
        required={"body_hex", "frame_hex"},
        label=f"{label}.golden",
    )
    body = _hex_bytes(golden["body_hex"], f"{label}.golden.body_hex")
    frame = _hex_bytes(golden["frame_hex"], f"{label}.golden.frame_hex")
    if not body:
        raise ContractError(f"{label}.golden.body_hex must contain a packet id")
    decoded_id, id_bytes = _decode_nonnegative_var_int(body, f"{label} packet id")
    if decoded_id != packet_id:
        raise ContractError(
            f"{label} declared id {packet_id} does not match golden body id {decoded_id}"
        )
    if id_bytes > len(body):
        raise ContractError(f"{label} packet id exceeds golden body")

    frame_length, length_bytes = _decode_nonnegative_var_int(frame, f"{label} frame length")
    framed_body = frame[length_bytes:]
    if frame_length != len(framed_body):
        raise ContractError(
            f"{label} frame length {frame_length} does not match {len(framed_body)} body bytes"
        )
    if framed_body != body:
        raise ContractError(f"{label} golden frame body does not match golden body")
    if frame != _encode_var_int(len(body)) + body:
        raise ContractError(f"{label} golden frame is not canonical")

    return name, (phase, direction, packet_id)


def validate_contract(
    contract_path: Path,
    *,
    lock_path: Path,
    records_root: Path,
) -> dict[str, object]:
    """Validate one protocol-contract artifact and return a compact admission summary."""
    expected_target = _read_lock(lock_path)
    contract = _read_json(contract_path, "protocol contract")
    _keys(
        contract,
        allowed={"schema", "id", "target", "packets"},
        required={"schema", "id", "target", "packets"},
        label="protocol contract",
    )
    if _integer(contract["schema"], "protocol contract schema") != SCHEMA:
        raise ContractError("unsupported protocol-contract schema")
    contract_id = _string(contract["id"], "protocol contract id")
    if CONTRACT_ID.fullmatch(contract_id) is None:
        raise ContractError(f"protocol contract id is not canonical: {contract_id!r}")

    target = _object(contract["target"], "protocol contract target")
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
        label="protocol contract target",
    )
    actual_target = {
        "minecraft": _string(target["minecraft"], "target.minecraft"),
        "protocol": _integer(target["protocol"], "target.protocol"),
        "source_archive_sha256": _string(
            target["source_archive_sha256"], "target.source_archive_sha256"
        ),
        "fingerprint_algorithm": _string(
            target["fingerprint_algorithm"], "target.fingerprint_algorithm"
        ),
    }
    if actual_target != expected_target:
        raise ContractError("protocol contract target does not match vanilla lock identity")

    packets = contract["packets"]
    if not isinstance(packets, list) or not packets:
        raise ContractError("protocol contract packets must be a non-empty array")
    records = _load_records(records_root)
    seen_names: set[tuple[str, str, str]] = set()
    seen_identities: set[tuple[str, str, int]] = set()
    for index, value in enumerate(packets):
        packet = _object(value, f"packets[{index}]")
        name, identity = _validate_packet(
            packet,
            index=index,
            records=records,
            fingerprint_algorithm=expected_target["fingerprint_algorithm"],
        )
        phase, direction, packet_id = identity
        name_identity = (phase, direction, name)
        if name_identity in seen_names:
            raise ContractError(f"duplicate packet name: {phase}/{direction}/{name}")
        if identity in seen_identities:
            raise ContractError(
                f"duplicate packet identity: {phase}/{direction}/{packet_id}"
            )
        seen_names.add(name_identity)
        seen_identities.add(identity)

    return {
        "schema": SCHEMA,
        "id": contract_id,
        "minecraft": expected_target["minecraft"],
        "protocol": expected_target["protocol"],
        "packets": len(packets),
    }


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("contract", type=Path)
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--records-root", type=Path, default=Path("vanilla/records"))
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    try:
        summary = validate_contract(
            args.contract,
            lock_path=args.lock,
            records_root=args.records_root,
        )
    except (ContractError, OSError) as error:
        print(f"protocol contract error: {error}", file=sys.stderr)
        return 1
    print(json.dumps(summary, sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
