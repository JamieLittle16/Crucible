#!/usr/bin/env python3
"""Resolve one canonical Crucible milestone tag to its versioned release-notes file."""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
NOTES_ROOT = REPO_ROOT / "docs/milestones/releases"
TAG_RE = re.compile(r"^milestone-[a-z0-9][a-z0-9-]*$")


class MilestoneReleaseError(ValueError):
    """Raised when a milestone tag cannot safely resolve release metadata."""


def resolve_notes(tag: str, *, notes_root: Path = NOTES_ROOT) -> Path:
    if TAG_RE.fullmatch(tag) is None:
        raise MilestoneReleaseError(f"unsupported milestone tag syntax: {tag}")
    path = notes_root / f"{tag}.md"
    if path.is_symlink() or not path.is_file():
        raise MilestoneReleaseError(f"missing or unsafe milestone release notes: {path}")
    return path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    args = parser.parse_args()
    try:
        path = resolve_notes(args.tag)
    except MilestoneReleaseError as error:
        print(f"milestone release metadata error: {error}", file=sys.stderr)
        return 2
    print(path.relative_to(REPO_ROOT))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
