#!/usr/bin/env python3
"""Complete the reviewed R2C world-state delegate closure in one source-safe operation.

This is operator composition only. It applies the committed source-free human decisions to the exact
untouched worksheet and then runs the independent delegate finalizer against the source-rich review
pack plus immutable upload manifest. The preferred operator path consumes the generated tar.gz
bundle directly, extracts its exact three canonical members only into ephemeral storage, and
atomically publishes a directory containing only source-free artifacts.
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
    from . import r2c_world_state_delegate_review_apply as apply_review
    from . import r2c_world_state_source_review_delegate_finalize as finalize_review
except ImportError:  # Direct `python3 tools/...` execution.
    import r2c_world_state_delegate_review_apply as apply_review  # type: ignore[no-redef]
    import r2c_world_state_source_review_delegate_finalize as finalize_review  # type: ignore[no-redef]

COMPLETED_WORKSHEET = "completed-worksheet.json"
REVIEW_RESULT = "delegate-review-result.json"
BUNDLE_MEMBERS = ("manifest.json", "review-pack.json", "worksheet.json")
MAX_BUNDLE_MEMBER_BYTES = 8 * 1024 * 1024


class CompleteError(RuntimeError):
    """Fail-closed one-shot delegate review completion error."""


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
        raise CompleteError(f"delegate review bundle must be a real non-symlink file: {bundle}")
    try:
        with tarfile.open(bundle, mode="r:gz") as archive:
            members = archive.getmembers()
            names = [member.name for member in members]
            if len(names) != len(BUNDLE_MEMBERS) or set(names) != set(BUNDLE_MEMBERS):
                raise CompleteError(
                    "delegate review bundle must contain exactly " + ", ".join(BUNDLE_MEMBERS)
                )
            paths: dict[str, Path] = {}
            for member in members:
                if not member.isfile() or member.issym() or member.islnk():
                    raise CompleteError(
                        f"delegate review bundle member must be a regular file: {member.name}"
                    )
                if member.size < 0 or member.size > MAX_BUNDLE_MEMBER_BYTES:
                    raise CompleteError(
                        f"delegate review bundle member exceeds bounded size: {member.name}"
                    )
                stream = archive.extractfile(member)
                if stream is None:
                    raise CompleteError(
                        f"delegate review bundle member cannot be read: {member.name}"
                    )
                raw = stream.read(MAX_BUNDLE_MEMBER_BYTES + 1)
                if len(raw) != member.size or len(raw) > MAX_BUNDLE_MEMBER_BYTES:
                    raise CompleteError(
                        f"delegate review bundle member size mismatch: {member.name}"
                    )
                path = directory / member.name
                path.write_bytes(raw)
                path.chmod(0o600)
                paths[member.name] = path
            return paths
    except (OSError, tarfile.TarError) as error:
        raise CompleteError(f"cannot read delegate review bundle {bundle}: {error}") from error


def complete(
    review_pack: Path,
    worksheet: Path,
    manifest: Path,
    decisions: Path,
    output_dir: Path,
) -> dict[str, object]:
    output_dir, parent = _prepare_output_parent(output_dir)
    temporary = Path(tempfile.mkdtemp(prefix=".r2c-delegate-review-", dir=parent))
    published = False
    try:
        completed = temporary / COMPLETED_WORKSHEET
        result = temporary / REVIEW_RESULT
        apply_summary = apply_review.apply(worksheet, decisions, completed)
        finalize_summary = finalize_review.finalize(review_pack, completed, manifest, result)

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
            "delegate_review_result": str(output_dir / REVIEW_RESULT),
            "delegate_review_result_sha256": finalize_summary["sha256"],
            "selected_sources": finalize_summary["selected_sources"],
            "rejected_sources": apply_summary["rejected_sources"],
            "groups": finalize_summary["groups"],
            "contains_official_source_text": False,
            "production_admitted": False,
        }
    except (OSError, UnicodeDecodeError, apply_review.ApplyError, finalize_review.FinalizeError) as error:
        raise CompleteError(str(error)) from error
    finally:
        if not published and temporary.exists():
            shutil.rmtree(temporary, ignore_errors=True)


def complete_bundle(bundle: Path, decisions: Path, output_dir: Path) -> dict[str, object]:
    """Complete one delegate review directly from the canonical source-review bundle."""
    output_dir, parent = _prepare_output_parent(output_dir)
    with tempfile.TemporaryDirectory(prefix=".r2c-delegate-bundle-", dir=parent) as temporary:
        paths = _materialize_bundle(bundle, Path(temporary))
        return complete(
            paths["review-pack.json"],
            paths["worksheet.json"],
            paths["manifest.json"],
            decisions,
            output_dir,
        )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--review-pack", type=Path)
    parser.add_argument("--worksheet", type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--decisions", type=Path, default=apply_review.DEFAULT_DECISIONS)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = _parser()
    args = parser.parse_args(argv)
    explicit = (args.review_pack, args.worksheet, args.manifest)
    if args.bundle is not None:
        if any(path is not None for path in explicit):
            parser.error("--bundle cannot be combined with --review-pack/--worksheet/--manifest")
    elif any(path is None for path in explicit):
        parser.error("provide --bundle or all of --review-pack, --worksheet and --manifest")

    try:
        if args.bundle is not None:
            summary = complete_bundle(args.bundle, args.decisions, args.output_dir)
        else:
            assert args.review_pack is not None
            assert args.worksheet is not None
            assert args.manifest is not None
            summary = complete(
                args.review_pack,
                args.worksheet,
                args.manifest,
                args.decisions,
                args.output_dir,
            )
    except CompleteError as error:
        print(f"R2C delegate review completion failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
