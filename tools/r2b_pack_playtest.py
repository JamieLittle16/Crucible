#!/usr/bin/env python3
"""Pack the source-admitted Configuration plus R2B shared artifacts for stock-client playtest.

The input is the already-qualified full R1X source-free JSON. The existing R1B validator is run over
that entire artifact first. This packer then writes only Configuration plus the three immutable R2B
shared projection bodies required by the selected profile: update-recipes, commands and server-data.
There is deliberately no captured-Play publication section in the output format.
"""
from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path
from typing import BinaryIO, Sequence

import r1b_pack_join_replay as r1b

MAGIC = b"CRR2B001"
UPDATE_RECIPES_PLAY_INDEX = 4
COMMANDS_PLAY_INDEX = 6
SERVER_DATA_PLAY_INDEX = 10
EXPECTED_PACKET_PREFIXES = (
    b"\x85\x01",  # update-recipes packet id 133
    b"\x10",      # commands packet id 16
    b"\x56",      # server-data packet id 86
)


class PackError(ValueError):
    """Fail-closed R2B playtest-image packing error."""


def _write_u32(output: BinaryIO, value: int) -> None:
    output.write(struct.pack("<I", value))


def _write_u64(output: BinaryIO, value: int) -> None:
    output.write(struct.pack("<Q", value))


def pack(input_path: Path, output_path: Path) -> tuple[int, int]:
    # The R1B validator checks schema, protocol/source/capture commitments, selected profile,
    # Configuration aggregate/hash and the complete 2,331-body captured Play aggregate/hash before
    # we select any bytes for this smaller development format.
    value = r1b._read_json(input_path)  # noqa: SLF001 - same-directory qualification tool boundary
    configuration, full_play = r1b._validate(value)  # noqa: SLF001

    indexes = (UPDATE_RECIPES_PLAY_INDEX, COMMANDS_PLAY_INDEX, SERVER_DATA_PLAY_INDEX)
    try:
        shared = [full_play[index] for index in indexes]
    except IndexError as error:
        raise PackError("validated capture no longer contains the selected R2B shared prefix") from error

    for body, prefix, index in zip(shared, EXPECTED_PACKET_PREFIXES, indexes, strict=True):
        if not body.startswith(prefix):
            raise PackError(f"captured Play body {index} no longer has the selected R2B packet identity")

    configuration_bytes = sum(map(len, configuration))
    if configuration_bytes != r1b.EXPECTED_CONFIG_BYTES:
        raise PackError("validated Configuration byte count drifted unexpectedly")

    if output_path.is_symlink():
        raise PackError(f"output must not be a symlink: {output_path}")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    temporary = output_path.with_name(f".{output_path.name}.tmp")
    if temporary.exists() or temporary.is_symlink():
        temporary.unlink()

    try:
        with temporary.open("xb") as output:
            output.write(MAGIC)
            _write_u32(output, r1b.EXPECTED_PROTOCOL)
            output.write(bytes.fromhex(r1b.EXPECTED_SOURCE_SHA256))
            output.write(bytes.fromhex(r1b.EXPECTED_CAPTURE_SHA256))
            _write_u32(output, len(configuration))
            _write_u64(output, configuration_bytes)
            for body in [*configuration, *shared]:
                _write_u32(output, len(body))
                output.write(body)
        temporary.replace(output_path)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise

    return len(configuration), sum(map(len, shared))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r2b-pack-playtest")
    parser.add_argument("input", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        configuration_frames, shared_bytes = pack(args.input, args.output)
    except (OSError, ValueError, PackError) as error:
        print(f"R2B playtest pack error: {error}", file=sys.stderr)
        return 2

    print(f"r2b_playtest_image={args.output}")
    print(f"configuration_frames={configuration_frames}")
    print(f"configuration_bytes={r1b.EXPECTED_CONFIG_BYTES}")
    print("captured_play_frames_written=0")
    print("shared_r2b_projection_frames=3")
    print(f"shared_r2b_projection_bytes={shared_bytes}")
    print(f"capture_sha256={r1b.EXPECTED_CAPTURE_SHA256}")
    print("production_admitted=false")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
