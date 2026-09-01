#!/usr/bin/env python3
"""Complete the reviewed R2C world-state delegate closure in one source-safe operation.

This is operator composition only. It applies the committed source-free human decisions to the exact
untouched worksheet and then runs the independent delegate finalizer against the source-rich review
pack plus immutable upload manifest. The output directory contains only source-free artifacts and is
published atomically after both stages succeed.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
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


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--review-pack", type=Path, required=True)
    parser.add_argument("--worksheet", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--decisions", type=Path, default=apply_review.DEFAULT_DECISIONS)
    parser.add_argument("--output-dir", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
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
