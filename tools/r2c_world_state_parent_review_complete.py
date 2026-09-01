#!/usr/bin/env python3
"""Complete the parent R2C BIOMES/HEIGHTMAPS/LIGHT review in one source-safe operation.

This is operator composition only. It applies a committed source-free human decision record to the
exact untouched parent worksheet, then runs the existing parent review finalizer against the bounded
source-rich review pack. The preferred path consumes the canonical parent source-review tar.gz
produced by ``r2c_world_state_source_review_bundle.py`` and atomically publishes only source-free
outputs.

The source-rich archive is never copied into the repository. Only the exact canonical regular-file
members are accepted; symlinks, hard links, extra regular files and oversized members fail closed.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tarfile
import tempfile
from pathlib import Path
from typing import Sequence

try:
    from . import r2c_world_state_source_review_finalize as finalize_review
    from . import r2c_world_state_source_review_parent_apply as apply_review
except ImportError:  # Direct `python3 tools/...` execution.
    import r2c_world_state_source_review_finalize as finalize_review  # type: ignore[no-redef]
    import r2c_world_state_source_review_parent_apply as apply_review  # type: ignore[no-redef]

COMPLETED_WORKSHEET = "completed-worksheet.json"
REVIEW_RESULT = "parent-review-result.json"
BUNDLE_MANIFEST = "bundle-manifest.json"
REVIEW_PACK = "world-state-review/review-pack.json"
WORKSHEET = "world-state-review/worksheet.json"
REVIEW_MANIFEST = "world-state-review/manifest.json"
DISCOVERY = "discovery/discovery.json"
BUNDLE_REGULAR_FILES = frozenset({BUNDLE_MANIFEST, REVIEW_PACK, WORKSHEET, REVIEW_MANIFEST, DISCOVERY})
ALLOWED_DIRECTORIES = frozenset({"discovery", "world-state-review"})
MAX_BUNDLE_MEMBER_BYTES = 20 * 1024 * 1024


class CompleteError(RuntimeError):
    """Fail-closed one-shot parent review completion error."""


def _prepare_output_parent(output_dir: Path) -> tuple[Path, Path]:
    if output_dir.exists() or output_dir.is_symlink():
        raise CompleteError(f"output directory must not already exist: {output_dir}")
    parent = output_dir.parent
    if parent.exists() and parent.is_symlink():
        raise CompleteError(f"output parent must not be a symlink: {parent}")
    if not parent.exists():
        try:
            parent.mkdir(parents=True)
        except OSError as error:
            raise CompleteError(f"cannot create output parent {parent}: {error}") from error
    return output_dir, parent


def _materialize_bundle(bundle: Path, directory: Path) -> dict[str, Path]:
    if bundle.is_symlink() or not bundle.is_file():
        raise CompleteError(f"parent review bundle must be a real non-symlink file: {bundle}")
    try:
        with tarfile.open(bundle, mode="r:gz") as archive:
            regular_names: set[str] = set()
            paths: dict[str, Path] = {}
            for member in archive.getmembers():
                if member.isdir():
                    name = member.name.rstrip("/")
                    if name not in ALLOWED_DIRECTORIES:
                        raise CompleteError(f"unexpected parent review bundle directory: {member.name}")
                    continue
                if not member.isfile() or member.issym() or member.islnk():
                    raise CompleteError(
                        f"parent review bundle member must be a regular file: {member.name}"
                    )
                if member.name not in BUNDLE_REGULAR_FILES:
                    raise CompleteError(f"unexpected parent review bundle file: {member.name}")
                if member.name in regular_names:
                    raise CompleteError(f"duplicate parent review bundle file: {member.name}")
                if member.size < 0 or member.size > MAX_BUNDLE_MEMBER_BYTES:
                    raise CompleteError(
                        f"parent review bundle member exceeds bounded size: {member.name}"
                    )
                stream = archive.extractfile(member)
                if stream is None:
                    raise CompleteError(f"parent review bundle member cannot be read: {member.name}")
                raw = stream.read(MAX_BUNDLE_MEMBER_BYTES + 1)
                if len(raw) != member.size or len(raw) > MAX_BUNDLE_MEMBER_BYTES:
                    raise CompleteError(
                        f"parent review bundle member size mismatch: {member.name}"
                    )
                path = directory / member.name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(raw)
                path.chmod(0o600)
                regular_names.add(member.name)
                paths[member.name] = path

            missing = sorted(BUNDLE_REGULAR_FILES - regular_names)
            if missing:
                raise CompleteError(f"parent review bundle is incomplete; missing files: {missing}")
            return paths
    except (OSError, tarfile.TarError) as error:
        raise CompleteError(f"cannot read parent review bundle {bundle}: {error}") from error


def complete(
    *,
    review_pack: Path,
    worksheet: Path,
    bundle_manifest: Path,
    decisions: Path,
    output_dir: Path,
) -> dict[str, object]:
    output_dir, parent = _prepare_output_parent(output_dir)
    temporary = Path(tempfile.mkdtemp(prefix=".r2c-parent-review-", dir=parent))
    published = False
    try:
        completed = temporary / COMPLETED_WORKSHEET
        result = temporary / REVIEW_RESULT
        apply_summary = apply_review.apply(
            worksheet=worksheet,
            bundle_manifest=bundle_manifest,
            decisions=decisions,
            output=completed,
        )
        finalize_summary = finalize_review.finalize(review_pack, completed, result)

        for path in (completed, result):
            text = path.read_text(encoding="utf-8", errors="strict")
            if "source_excerpt" in text:
                raise CompleteError(f"source-rich field leaked into source-free output: {path.name}")

        os.replace(temporary, output_dir)
        published = True
        return {
            "output_dir": str(output_dir),
            "completed_worksheet": str(output_dir / COMPLETED_WORKSHEET),
            "completed_worksheet_sha256": apply_summary["sha256"],
            "parent_review_result": str(output_dir / REVIEW_RESULT),
            "parent_review_result_sha256": finalize_summary["sha256"],
            "selected_sources": finalize_summary["selected_sources"],
            "rejected_sources": apply_summary["rejected_sources"],
            "groups": finalize_summary["groups"],
            "contains_official_source_text": False,
            "production_admitted": False,
        }
    except (
        OSError,
        UnicodeDecodeError,
        apply_review.ApplyError,
        finalize_review.FinalizeError,
    ) as error:
        raise CompleteError(str(error)) from error
    finally:
        if not published and temporary.exists():
            shutil.rmtree(temporary, ignore_errors=True)


def complete_bundle(bundle: Path, decisions: Path, output_dir: Path) -> dict[str, object]:
    """Complete one parent review directly from the canonical source-review bundle."""
    output_dir, parent = _prepare_output_parent(output_dir)
    with tempfile.TemporaryDirectory(prefix=".r2c-parent-bundle-", dir=parent) as temporary:
        paths = _materialize_bundle(bundle, Path(temporary))
        return complete(
            review_pack=paths[REVIEW_PACK],
            worksheet=paths[WORKSHEET],
            bundle_manifest=paths[BUNDLE_MANIFEST],
            decisions=decisions,
            output_dir=output_dir,
        )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--review-pack", type=Path)
    parser.add_argument("--worksheet", type=Path)
    parser.add_argument("--bundle-manifest", type=Path)
    parser.add_argument("--decisions", type=Path, default=apply_review.DEFAULT_DECISIONS)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = _parser()
    args = parser.parse_args(argv)
    explicit = (args.review_pack, args.worksheet, args.bundle_manifest)
    if args.bundle is not None:
        if any(path is not None for path in explicit):
            parser.error("--bundle cannot be combined with --review-pack/--worksheet/--bundle-manifest")
    elif any(path is None for path in explicit):
        parser.error("provide --bundle or all of --review-pack, --worksheet and --bundle-manifest")

    try:
        if args.bundle is not None:
            summary = complete_bundle(args.bundle, args.decisions, args.output_dir)
        else:
            assert args.review_pack is not None
            assert args.worksheet is not None
            assert args.bundle_manifest is not None
            summary = complete(
                review_pack=args.review_pack,
                worksheet=args.worksheet,
                bundle_manifest=args.bundle_manifest,
                decisions=args.decisions,
                output_dir=args.output_dir,
            )
    except CompleteError as error:
        print(f"R2C parent review completion failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
