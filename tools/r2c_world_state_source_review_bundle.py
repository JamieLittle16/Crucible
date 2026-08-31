#!/usr/bin/env python3
"""Produce one uploadable local R2C biome/heightmap/light source-review bundle.

This is an operator convenience wrapper around the existing fail-closed R2C source-discovery and
focused source-review-pack tools. It does not infer or admit semantics. The generated archive is
source-rich, ephemeral and must remain outside the repository.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tarfile
import tempfile
from pathlib import Path
from typing import Sequence

try:
    from . import r2c_world_projection_source_review as discovery
    from . import r2c_world_state_source_review_pack as packer
except ImportError:  # Direct `python3 tools/...` execution.
    import r2c_world_projection_source_review as discovery  # type: ignore[no-redef]
    import r2c_world_state_source_review_pack as packer  # type: ignore[no-redef]

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DB = discovery.DEFAULT_DB
DEFAULT_SOURCE = discovery.DEFAULT_SOURCE
DEFAULT_LOCK = discovery.DEFAULT_LOCK
REQUIRED_ARCHIVE_MEMBERS = frozenset(
    {
        "discovery/discovery.json",
        "world-state-review/review-pack.json",
        "world-state-review/worksheet.json",
        "world-state-review/manifest.json",
    }
)


class BundleError(RuntimeError):
    """Fail-closed local bundle construction error."""


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _external_output(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise BundleError(f"output archive must not already exist: {path}")
    resolved = path.expanduser().resolve(strict=False)
    repository = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repository)
    except ValueError:
        return resolved
    raise BundleError("source-rich R2C review bundle must live outside the repository")


def _verify_archive(path: Path) -> int:
    """Require the upload artifact to contain every review handoff before publication."""
    try:
        with tarfile.open(path, mode="r:gz") as archive:
            regular_files = {member.name for member in archive.getmembers() if member.isfile()}
    except (OSError, tarfile.TarError) as error:
        raise BundleError(f"cannot reopen staged R2C review bundle: {error}") from error

    missing = sorted(REQUIRED_ARCHIVE_MEMBERS - regular_files)
    if missing:
        raise BundleError(f"staged R2C review bundle is incomplete; missing members: {missing}")
    return len(regular_files)


def build_bundle(
    *,
    output: Path,
    db: Path,
    source: Path,
    lock: Path,
    plan: Path = discovery.DEFAULT_PLAN,
) -> dict[str, object]:
    """Run bounded discovery + focused review-pack extraction and archive both outputs."""
    output = _external_output(output)
    output.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="helve-r2c-source-review-") as temporary:
        root = Path(temporary)
        discovery_dir = root / "discovery"
        review_dir = root / "world-state-review"

        discovery_result = discovery.prepare(discovery_dir, plan, db, source, lock)
        review_result = packer.build(discovery_dir / "discovery.json", source, lock, review_dir)

        # Never open the user-visible destination until a complete archive has been written and
        # reopened successfully. A packaging failure must not leave behind a valid-looking empty
        # gzip/tar that an operator can accidentally upload as evidence.
        with tempfile.TemporaryDirectory(
            prefix=f".{output.name}.staging-", dir=output.parent
        ) as staging:
            staged_archive = Path(staging) / output.name
            with tarfile.open(staged_archive, mode="w:gz") as archive:
                archive.add(discovery_dir, arcname="discovery", recursive=True)
                archive.add(review_dir, arcname="world-state-review", recursive=True)
            archive_members = _verify_archive(staged_archive)
            staged_archive.replace(output)

    return {
        "output": str(output),
        "sha256": _sha256_file(output),
        "archive_regular_files": archive_members,
        "discovery_sha256": discovery_result["discovery_sha256"],
        "review_pack_sha256": review_result["review_pack_sha256"],
        "worksheet_sha256": review_result["worksheet_sha256"],
        "unique_candidate_methods": discovery_result["unique_candidate_methods"],
        "unique_source_records": review_result["unique_source_records"],
        "source_excerpt_bytes": review_result["source_excerpt_bytes"],
        "production_admitted": False,
        "contains_official_source_text": True,
        "commit_policy": "EPHEMERAL_DO_NOT_COMMIT",
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--plan", type=Path, default=discovery.DEFAULT_PLAN)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = build_bundle(
            output=args.output,
            db=args.db,
            source=args.source,
            lock=args.lock,
            plan=args.plan,
        )
    except (BundleError, OSError, discovery.DiscoveryError, packer.ReviewPackError) as error:
        print(f"R2C world-state source-review bundle failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
