#!/usr/bin/env python3
"""Emit exact, line-numbered excerpts from the pinned official Java source archive.

The source archive remains local and uncommitted.  This helper is for reproducible
human/AI review handoffs: before emitting any requested archive member it verifies
the same target identity pinned by ``vanilla/vanilla.lock.toml``.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
import zipfile
from pathlib import Path
from typing import Any, Sequence

KIND = "vanilla-source-excerpt-v1"


class ExcerptError(RuntimeError):
    """Raised when pinned-source excerpt production cannot proceed safely."""


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _load_lock(path: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise ExcerptError(f"lock must be a real non-symlink file: {path}")
    try:
        lock = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ExcerptError(f"cannot read lock {path}: {error}") from error
    if lock.get("schema") != 1:
        raise ExcerptError(f"unsupported vanilla lock schema: {lock.get('schema')!r}")
    source = lock.get("source")
    if not isinstance(source, dict):
        raise ExcerptError("vanilla lock is missing [source]")
    return lock


def _verified_archive(source: Path, lock: dict[str, Any]) -> tuple[zipfile.ZipFile, dict[str, Any]]:
    if source.is_symlink() or not source.is_file():
        raise ExcerptError(f"source must be a real non-symlink file: {source}")

    archive_sha256 = _sha256_file(source)
    try:
        archive = zipfile.ZipFile(source)
    except (OSError, zipfile.BadZipFile) as error:
        raise ExcerptError(f"cannot open source archive {source}: {error}") from error

    try:
        version = json.loads(archive.read("src/version.json"))
    except (KeyError, json.JSONDecodeError, UnicodeDecodeError) as error:
        archive.close()
        raise ExcerptError("source archive has no valid src/version.json") from error

    source_lock = lock["source"]
    assert isinstance(source_lock, dict)
    actual = {
        "minecraft": str(version.get("id", "")),
        "protocol": int(version.get("protocol_version", -1)),
        "data_version": int(version.get("world_version", -1)),
        "archive_sha256": archive_sha256,
        "java_files": sum(1 for name in archive.namelist() if name.endswith(".java")),
    }
    expected = {
        "minecraft": str(lock.get("minecraft", "")),
        "protocol": int(lock.get("protocol", -1)),
        "data_version": int(lock.get("data_version", -1)),
        "archive_sha256": str(source_lock.get("archive_sha256", "")),
        "java_files": int(source_lock.get("java_files", -1)),
    }
    mismatches = [
        f"{key}: actual={actual[key]!r} expected={expected[key]!r}"
        for key in actual
        if actual[key] != expected[key]
    ]
    if mismatches:
        archive.close()
        raise ExcerptError("source identity mismatch: " + "; ".join(mismatches))
    return archive, actual


def render_excerpt(source: Path, lock_path: Path, requested_paths: Sequence[str]) -> str:
    """Verify ``source`` and return deterministic line-numbered exact archive members."""
    if not requested_paths:
        raise ExcerptError("at least one source path is required")
    if any(not path or not path.startswith("src/") for path in requested_paths):
        raise ExcerptError("every source path must be a non-empty exact src/... archive path")
    if len(set(requested_paths)) != len(requested_paths):
        raise ExcerptError("duplicate source paths are not allowed")

    lock = _load_lock(lock_path)
    archive, identity = _verified_archive(source, lock)
    try:
        names = set(archive.namelist())
        missing = sorted(path for path in requested_paths if path not in names)
        if missing:
            raise ExcerptError("source archive is missing requested paths: " + ", ".join(missing))

        paths = sorted(requested_paths)
        lines = [
            f"kind: {KIND}",
            f"minecraft: {identity['minecraft']}",
            f"protocol: {identity['protocol']}",
            f"data_version: {identity['data_version']}",
            f"source_archive_sha256: {identity['archive_sha256']}",
            f"java_files: {identity['java_files']}",
            f"files: {len(paths)}",
        ]
        for path in paths:
            raw = archive.read(path)
            try:
                text = raw.decode("utf-8", errors="strict")
            except UnicodeDecodeError as error:
                raise ExcerptError(f"requested source is not valid UTF-8: {path}") from error
            lines.extend(
                [
                    "",
                    "=" * 110,
                    path,
                    f"sha256: {_sha256_bytes(raw)}",
                    "=" * 110,
                ]
            )
            for number, line in enumerate(text.splitlines(), 1):
                lines.append(f"{number:5}: {line}")
        return "\n".join(lines) + "\n"
    finally:
        archive.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="local official mc-src.zip")
    parser.add_argument("paths", nargs="+", help="exact src/... archive members to emit")
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--output", type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        rendered = render_excerpt(args.source, args.lock, args.paths)
        if args.output is None:
            print(rendered, end="")
        else:
            if args.output.is_symlink():
                raise ExcerptError(f"output must not be a symlink: {args.output}")
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(rendered, encoding="utf-8")
            print(
                json.dumps(
                    {
                        "kind": KIND,
                        "files": len(args.paths),
                        "output": str(args.output),
                        "sha256": _sha256_bytes(rendered.encode("utf-8")),
                    },
                    sort_keys=True,
                    separators=(",", ":"),
                )
            )
    except (ExcerptError, OSError) as error:
        print(f"source excerpt error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
