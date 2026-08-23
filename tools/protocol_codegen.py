#!/usr/bin/env python3
"""Generate zero-runtime-registry Rust packet constants from an admitted protocol contract."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
from pathlib import Path
from typing import Any

from tools.protocol_contract import ContractError, validate_contract

PHASE_ORDER = {"handshake": 0, "status": 1, "login": 2, "configuration": 3, "play": 4}
DIRECTION_ORDER = {"serverbound": 0, "clientbound": 1}


class CodegenError(ValueError):
    """Raised when an admitted contract cannot be represented safely as static Rust."""


def _read_contract(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise CodegenError(f"could not read admitted contract {path}: {error}") from error
    if not isinstance(value, dict):
        raise CodegenError("admitted protocol contract must be a JSON object")
    return value


def _rust_ascii_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise CodegenError(f"{label} must be a non-empty string")
    try:
        value.encode("ascii")
    except UnicodeEncodeError as error:
        raise CodegenError(f"{label} must be ASCII for generated Rust") from error
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def _rust_const_name(packet_name: object) -> str:
    if not isinstance(packet_name, str) or not packet_name:
        raise CodegenError("packet name must be a non-empty string")
    name = packet_name.replace("-", "_").upper()
    if not name or not (name[0].isalpha() or name[0] == "_"):
        raise CodegenError(f"packet name cannot become a Rust constant: {packet_name!r}")
    if any(not (char.isalnum() or char == "_") for char in name):
        raise CodegenError(f"packet name cannot become a Rust constant: {packet_name!r}")
    return name


def _hex_bytes(value: object, label: str) -> bytes:
    if not isinstance(value, str):
        raise CodegenError(f"{label} must be hexadecimal text")
    try:
        return bytes.fromhex(value)
    except ValueError as error:
        raise CodegenError(f"{label} is not valid hexadecimal") from error


def _canonical_contract(contract: dict[str, Any]) -> dict[str, object]:
    target = contract["target"]
    packets = []
    for packet in contract["packets"]:
        packets.append(
            {
                "name": packet["name"],
                "phase": packet["phase"],
                "direction": packet["direction"],
                "id": packet["id"],
                "semantic_rules": sorted(packet["semantic_rules"]),
                "source_records": sorted(packet["source_records"]),
                "golden": {
                    "body_hex": packet["golden"]["body_hex"],
                    "frame_hex": packet["golden"]["frame_hex"],
                },
            }
        )
    packets.sort(
        key=lambda packet: (
            PHASE_ORDER[packet["phase"]],
            DIRECTION_ORDER[packet["direction"]],
            packet["id"],
            packet["name"],
        )
    )
    return {
        "schema": contract["schema"],
        "id": contract["id"],
        "target": {
            "minecraft": target["minecraft"],
            "protocol": target["protocol"],
            "source_archive_sha256": target["source_archive_sha256"],
            "fingerprint_algorithm": target["fingerprint_algorithm"],
        },
        "packets": packets,
    }


def _canonical_digest(contract: dict[str, Any]) -> str:
    encoded = json.dumps(
        _canonical_contract(contract),
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=True,
    ).encode("ascii")
    return hashlib.sha256(encoded).hexdigest()


def _render_bytes(data: bytes, indent: str) -> str:
    if not data:
        return "&[]"
    chunks = [data[index : index + 16] for index in range(0, len(data), 16)]
    if len(chunks) == 1:
        return "&[" + ", ".join(f"0x{byte:02x}" for byte in chunks[0]) + "]"
    lines = ["&["]
    for chunk in chunks:
        lines.append(indent + "    " + ", ".join(f"0x{byte:02x}" for byte in chunk) + ",")
    lines.append(indent + "]")
    return "\n".join(lines)


def render_rust(contract: dict[str, Any]) -> str:
    """Render one already-validated contract as direct compile-time Rust constants."""
    canonical = _canonical_contract(contract)
    digest = _canonical_digest(contract)
    target = canonical["target"]
    packets = canonical["packets"]

    seen_constants: set[tuple[str, str, str]] = set()
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for packet in packets:
        constant = _rust_const_name(packet["name"])
        key = (packet["phase"], packet["direction"], constant)
        if key in seen_constants:
            raise CodegenError(
                "packet names collide after Rust normalization in "
                f"{packet['phase']}/{packet['direction']}: {constant}"
            )
        seen_constants.add(key)
        grouped.setdefault((packet["phase"], packet["direction"]), []).append(packet)

    lines = [
        "// @generated by tools/protocol_codegen.py; do not edit by hand.",
        "// Runtime packet identity is encoded entirely as compile-time constants.",
        "",
        f"pub const CONTRACT_ID: &str = {_rust_ascii_string(canonical['id'], 'contract id')};",
        f"pub const CONTRACT_SHA256: &str = \"{digest}\";",
        f"pub const MINECRAFT_VERSION: &str = {_rust_ascii_string(target['minecraft'], 'Minecraft version')};",
        f"pub const PROTOCOL_VERSION: i32 = {target['protocol']};",
        f"pub const SOURCE_ARCHIVE_SHA256: &str = \"{target['source_archive_sha256']}\";",
        f"pub const FINGERPRINT_ALGORITHM: &str = {_rust_ascii_string(target['fingerprint_algorithm'], 'fingerprint algorithm')};",
        "",
    ]

    for phase in PHASE_ORDER:
        phase_groups = [key for key in grouped if key[0] == phase]
        if not phase_groups:
            continue
        lines.append(f"pub mod {phase} {{")
        for direction in DIRECTION_ORDER:
            key = (phase, direction)
            current = grouped.get(key)
            if not current:
                continue
            lines.append(f"    pub mod {direction} {{")
            for packet in current:
                constant = _rust_const_name(packet["name"])
                lines.append(f"        pub const {constant}: i32 = {packet['id']};")
            lines.append("    }")
        lines.append("}")
        lines.append("")

    lines.extend(
        [
            "#[cfg(test)]",
            "pub mod golden {",
            "    // Golden bytes are qualification evidence only and disappear from production builds.",
        ]
    )
    for packet in packets:
        phase = packet["phase"].upper()
        direction = packet["direction"].upper()
        constant = _rust_const_name(packet["name"])
        prefix = f"{phase}_{direction}_{constant}"
        body = _hex_bytes(packet["golden"]["body_hex"], f"{packet['name']} body")
        frame = _hex_bytes(packet["golden"]["frame_hex"], f"{packet['name']} frame")
        lines.append(
            f"    pub const {prefix}_BODY: &[u8] = {_render_bytes(body, '    ')};"
        )
        lines.append(
            f"    pub const {prefix}_FRAME: &[u8] = {_render_bytes(frame, '    ')};"
        )
    lines.append("}")
    lines.append("")
    return "\n".join(lines)


def generate(
    contract_path: Path,
    *,
    lock_path: Path,
    records_root: Path,
    output_path: Path,
    check: bool,
) -> str:
    """Validate the evidence contract, render Rust, and write or byte-check the output."""
    validate_contract(contract_path, lock_path=lock_path, records_root=records_root)
    contract = _read_contract(contract_path)
    rendered = render_rust(contract)

    if check:
        if output_path.is_symlink() or not output_path.is_file():
            raise CodegenError(f"generated Rust output is missing or unsafe: {output_path}")
        if output_path.read_text(encoding="utf-8") != rendered:
            raise CodegenError(f"generated Rust output drifted: {output_path}")
    else:
        output_path.parent.mkdir(parents=True, exist_ok=True)
        if output_path.exists() and output_path.is_symlink():
            raise CodegenError(f"refusing to replace symlink output: {output_path}")
        output_path.write_text(rendered, encoding="utf-8")
    return rendered


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("contract", type=Path)
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--records-root", type=Path, default=Path("vanilla/records"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    try:
        rendered = generate(
            args.contract,
            lock_path=args.lock,
            records_root=args.records_root,
            output_path=args.output,
            check=args.check,
        )
    except (ContractError, CodegenError, OSError, KeyError, TypeError) as error:
        print(f"protocol codegen error: {error}", file=sys.stderr)
        return 1
    mode = "check" if args.check else "generate"
    print(f"protocol codegen {mode}: PASS bytes={len(rendered.encode('utf-8'))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
