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
from typing import Mapping, Sequence

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
BUNDLE_MANIFEST_NAME = "bundle-manifest.json"
BUNDLE_MANIFEST_KIND = "r2c-world-state-source-review-bundle-manifest"
BUNDLE_MANIFEST_COMMIT_POLICY = "SOURCE_FREE_UPLOAD_PROVENANCE"
REQUIRED_ARCHIVE_MEMBERS = frozenset(
    {
        BUNDLE_MANIFEST_NAME,
        "discovery/discovery.json",
        "world-state-review/review-pack.json",
        "world-state-review/worksheet.json",
        "world-state-review/manifest.json",
    }
)


class BundleError(RuntimeError):
    """Fail-closed local bundle construction error."""


def _pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _read_json_object(path: Path, label: str) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise BundleError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise BundleError(f"{label} must be a JSON object")
    return value


def _required_object(value: object, label: str) -> Mapping[str, object]:
    if not isinstance(value, dict):
        raise BundleError(f"{label} must be a JSON object")
    return value


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


def _verify_discovery_provenance(discovery_path: Path, plan_path: Path) -> dict[str, str]:
    """Bind generated discovery to the exact plan/frontier still present in this checkout."""
    value = _read_json_object(discovery_path, "R2C discovery provenance")
    inputs = _required_object(value.get("inputs"), "R2C discovery inputs")
    source = _required_object(value.get("source"), "R2C discovery source")

    plan = discovery._load_plan(plan_path)
    expected_plan_sha = _sha256_file(plan_path)
    expected_frontier_sha = _sha256_file(plan.frontier)
    actual_plan_sha = inputs.get("plan_sha256")
    actual_frontier_sha = inputs.get("frontier_sha256")
    if actual_plan_sha != expected_plan_sha:
        raise BundleError(
            "R2C discovery plan changed after discovery or was produced from a different plan: "
            f"expected {expected_plan_sha}, got {actual_plan_sha}"
        )
    if actual_frontier_sha != expected_frontier_sha:
        raise BundleError(
            "R2C discovery frontier changed after discovery or was produced from a different frontier: "
            f"expected {expected_frontier_sha}, got {actual_frontier_sha}"
        )

    source_sha = source.get("archive_sha256")
    if source_sha != packer.EXPECTED_SOURCE_SHA256:
        raise BundleError(
            "R2C discovery source identity drifted before bundle publication: "
            f"expected {packer.EXPECTED_SOURCE_SHA256}, got {source_sha}"
        )
    return {
        "plan_sha256": expected_plan_sha,
        "frontier_sha256": expected_frontier_sha,
        "source_archive_sha256": str(source_sha),
    }


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
        provenance = _verify_discovery_provenance(discovery_dir / "discovery.json", plan)
        review_result = packer.build(discovery_dir / "discovery.json", source, lock, review_dir)
        # Re-check after source-rich extraction as well. A checkout change during extraction must not
        # leave a bundle whose discovery and current review policy silently disagree.
        if _verify_discovery_provenance(discovery_dir / "discovery.json", plan) != provenance:
            raise BundleError("R2C discovery provenance changed during source-review extraction")

        bundle_manifest: dict[str, object] = {
            "schema": 1,
            "kind": BUNDLE_MANIFEST_KIND,
            "commit_policy": BUNDLE_MANIFEST_COMMIT_POLICY,
            "contains_official_source_text": False,
            "production_admitted": False,
            **provenance,
            "discovery_sha256": discovery_result["discovery_sha256"],
            "review_pack_sha256": review_result["review_pack_sha256"],
            "worksheet_sha256": review_result["worksheet_sha256"],
            "unique_candidate_methods": discovery_result["unique_candidate_methods"],
            "unique_source_records": review_result["unique_source_records"],
            "source_excerpt_bytes": review_result["source_excerpt_bytes"],
        }
        manifest_path = root / BUNDLE_MANIFEST_NAME
        manifest_path.write_bytes(_pretty_bytes(bundle_manifest))
        bundle_manifest_sha = _sha256_file(manifest_path)

        # Never open the user-visible destination until a complete archive has been written and
        # reopened successfully. A packaging failure must not leave behind a valid-looking empty
        # gzip/tar that an operator can accidentally upload as evidence.
        with tempfile.TemporaryDirectory(
            prefix=f".{output.name}.staging-", dir=output.parent
        ) as staging:
            staged_archive = Path(staging) / output.name
            with tarfile.open(staged_archive, mode="w:gz") as archive:
                archive.add(manifest_path, arcname=BUNDLE_MANIFEST_NAME)
                archive.add(discovery_dir, arcname="discovery", recursive=True)
                archive.add(review_dir, arcname="world-state-review", recursive=True)
            archive_members = _verify_archive(staged_archive)
            staged_archive.replace(output)

    return {
        "output": str(output),
        "sha256": _sha256_file(output),
        "bundle_manifest_sha256": bundle_manifest_sha,
        "plan_sha256": provenance["plan_sha256"],
        "frontier_sha256": provenance["frontier_sha256"],
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
