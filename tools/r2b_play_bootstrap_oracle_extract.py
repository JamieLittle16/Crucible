#!/usr/bin/env python3
"""Extract source-qualified composition-stable R2B oracle bodies from the pinned R1X capture.

This tool does not promote replay ordering to semantic authority. It first validates the complete
source-free R1X capture with the existing fail-closed validator, then selects only packet identities
whose presence/meaning have been independently source-reviewed for R2B.

The output is source-free black-box evidence for immutable projection candidates. Production
admission still requires the R2B VAR/SEM gate and an explicit composition key/invalidation law.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any, Sequence

try:
    from . import r1b_pack_join_replay as replay
except ImportError:  # Direct `python3 tools/...` execution.
    import r1b_pack_join_replay as replay  # type: ignore[no-redef]

SCHEMA = 1
KIND = "r2b-play-bootstrap-oracle-v1"

# Source-derived Play identities for the selected 26.2 bootstrap surface.
COMMANDS_ID = 16
UPDATE_RECIPES_ID = 133

TARGETS: tuple[tuple[str, int, str], ...] = (
    ("commands", COMMANDS_ID, "COMMAND_TREE"),
    ("update-recipes", UPDATE_RECIPES_ID, "SYNCHRONIZED_RECIPES"),
)


class OracleExtractError(ValueError):
    """Fail-closed oracle extraction error."""


def decode_varint_prefix(body: bytes) -> tuple[int, int]:
    """Decode one non-negative Minecraft VarInt from the start of a packet body."""

    value = 0
    for index, byte in enumerate(body[:5]):
        value |= (byte & 0x7F) << (7 * index)
        if byte & 0x80 == 0:
            if value > 0x7FFF_FFFF:
                raise OracleExtractError("packet id VarInt exceeds signed 32-bit range")
            return value, index + 1
    raise OracleExtractError("packet body does not start with a complete packet-id VarInt")


def _frame(body: bytes, *, index: int, packet_id_bytes: int) -> dict[str, object]:
    return {
        "play_body_index": index,
        "packet_id_bytes": packet_id_bytes,
        "body_bytes": len(body),
        "body_sha256": hashlib.sha256(body).hexdigest(),
        "body_hex": body.hex(),
    }


def extract(value: dict[str, Any]) -> dict[str, object]:
    # Validate the complete capture, not merely the selected bodies.
    _config, play = replay._validate(value)

    by_id: dict[int, list[tuple[int, bytes, int]]] = {}
    for index, body in enumerate(play):
        packet_id, width = decode_varint_prefix(body)
        by_id.setdefault(packet_id, []).append((index, body, width))

    artifacts: list[dict[str, object]] = []
    for name, packet_id, semantic_group in TARGETS:
        matches = by_id.get(packet_id, [])
        if len(matches) != 1:
            raise OracleExtractError(
                f"expected exactly one Play {name} packet id {packet_id}, got {len(matches)}"
            )
        index, body, width = matches[0]
        artifacts.append(
            {
                "name": name,
                "semantic_group": semantic_group,
                "phase": "play",
                "direction": "clientbound",
                "packet_id": packet_id,
                **_frame(body, index=index, packet_id_bytes=width),
            }
        )

    profile = value["selected_capture_profile"]
    return {
        "schema": SCHEMA,
        "kind": KIND,
        "oracle_only": True,
        "production_admitted": False,
        "target": {
            "minecraft": replay.EXPECTED_MINECRAFT,
            "protocol": replay.EXPECTED_PROTOCOL,
            "source_archive_sha256": replay.EXPECTED_SOURCE_SHA256,
            "capture_sha256": replay.EXPECTED_CAPTURE_SHA256,
        },
        "selected_capture_profile": {
            "player_name": profile["player_name"],
            "offline_profile_uuid": profile["offline_profile_uuid"],
            "session_uuid": profile["session_uuid"],
        },
        "semantic_authority": "R2B source VAR/SEM gate; capture bytes are black-box confirmation only",
        "artifacts": artifacts,
    }


def _read(path: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise OracleExtractError(f"input must be a real non-symlink file: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise OracleExtractError(f"could not read replay JSON: {error}") from error
    if not isinstance(value, dict):
        raise OracleExtractError("replay JSON root must be an object")
    return value


def write(output: Path, result: dict[str, object]) -> None:
    if output.is_symlink():
        raise OracleExtractError(f"output must not be a symlink: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.tmp")
    temporary.unlink(missing_ok=True)
    try:
        temporary.write_text(
            json.dumps(result, sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
        temporary.replace(output)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="validated source-free R1X replay JSON")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        result = extract(_read(args.input))
        write(args.output, result)
    except (OracleExtractError, replay.PackError, OSError, KeyError, TypeError, ValueError) as error:
        print(f"R2B bootstrap oracle extraction error: {error}", file=sys.stderr)
        return 2

    print(f"r2b_bootstrap_oracle={args.output}")
    print(f"artifacts={len(result['artifacts'])}")
    print(f"capture_sha256={replay.EXPECTED_CAPTURE_SHA256}")
    print("oracle_only=true")
    print("production_admitted=false")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
