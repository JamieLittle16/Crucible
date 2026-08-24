#!/usr/bin/env python3
"""Create one bounded local R1B Configuration source-review session.

This is orchestration only. It deliberately delegates every evidence decision to the existing
source probe, source-text firewall, ephemeral dossier builder and manual-review workflow. The
session directory contains official source text, must live outside the repository, and is removed
if any component fails so a partial run cannot be mistaken for review-ready evidence.
"""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import shutil
import sys
from pathlib import Path
from typing import Mapping, Sequence

try:
    from . import r1b_configuration_bundle_review as bundle_review
    from . import r1b_configuration_review as review
    from . import r1b_configuration_review_dossier as dossier
    from . import r1b_configuration_source_probe as source_probe
except ImportError:  # Direct `python3 tools/...` execution.
    import r1b_configuration_bundle_review as bundle_review  # type: ignore[no-redef]
    import r1b_configuration_review as review  # type: ignore[no-redef]
    import r1b_configuration_review_dossier as dossier  # type: ignore[no-redef]
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]

SCHEMA = 1
SESSION_KIND = "r1b-configuration-local-review-session"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
REPO_ROOT = Path(__file__).resolve().parents[1]


class ReviewSessionError(RuntimeError):
    """Fail-closed local review-session orchestration error."""


def sha256_file(path: Path) -> str:
    """Return the SHA-256 of one generated session file."""
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _is_within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
    except ValueError:
        return False
    return True


def _validated_session_dir(path: Path, *, repo_root: Path = REPO_ROOT) -> Path:
    """Return a fresh external session path or fail before creating anything."""
    if path.exists() or path.is_symlink():
        raise ReviewSessionError(f"session output must not already exist: {path}")
    resolved = path.resolve(strict=False)
    resolved_repo = repo_root.resolve(strict=True)
    if _is_within(resolved, resolved_repo):
        raise ReviewSessionError(
            "review session contains official source text and must be outside the Git repository"
        )
    return resolved


def _write_manifest(path: Path, manifest: Mapping[str, object]) -> None:
    path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def build_session(
    *,
    output_dir: Path,
    db: Path = source_probe.DEFAULT_DB,
    source_archive: Path = source_probe.DEFAULT_SOURCE,
    lock_path: Path = source_probe.DEFAULT_LOCK,
    frontier_path: Path = source_probe.DEFAULT_FRONTIER,
    plan_path: Path = review.DEFAULT_PLAN,
    semantics_path: Path = review.DEFAULT_SEMANTICS,
    repo_root: Path = REPO_ROOT,
) -> dict[str, object]:
    """Run the existing R1B evidence tools into one fresh external review directory."""
    session = _validated_session_dir(output_dir, repo_root=repo_root)
    transcript = session / "source-probe.txt"
    bundle = session / "admission-bundle.json"
    review_pack = session / "review-pack"
    review_dossier = session / "review-dossier.json"
    worksheet = session / "review-worksheet.json"
    manifest_path = session / "session-manifest.json"

    session.mkdir(parents=True)
    try:
        with transcript.open("x", encoding="utf-8") as sink, contextlib.redirect_stdout(sink):
            result = source_probe.run(db, source_archive, frontier_path, lock_path, bundle)
        if result != 0:
            raise ReviewSessionError(f"source probe returned unexpected status {result}")
        if not bundle.is_file():
            raise ReviewSessionError("source probe did not create the required admission bundle")

        bundle_review.materialize_review_pack(
            bundle_path=bundle,
            output_dir=review_pack,
            lock_path=lock_path,
            frontier_path=frontier_path,
        )

        dossier_value = dossier.build_dossier(
            db=db,
            source_archive=source_archive,
            lock_path=lock_path,
            plan_path=plan_path,
            semantics_path=semantics_path,
        )
        dossier.write_dossier(review_dossier, dossier_value)

        worksheet_value = review.prepare_worksheet(
            review_pack=review_pack,
            output=worksheet,
            plan_path=plan_path,
            semantics_path=semantics_path,
        )

        candidates = worksheet_value.get("candidates")
        if not isinstance(candidates, list) or len(candidates) != len(source_probe.CANDIDATES):
            raise ReviewSessionError("prepared worksheet candidate cardinality drifted")

        manifest: dict[str, object] = {
            "schema": SCHEMA,
            "kind": SESSION_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "candidate_count": len(candidates),
            "source_rich_artifacts": {
                "source_probe_transcript": {
                    "path": transcript.name,
                    "sha256": sha256_file(transcript),
                },
                "admission_bundle": {
                    "path": bundle.name,
                    "sha256": sha256_file(bundle),
                },
                "review_dossier": {
                    "path": review_dossier.name,
                    "sha256": sha256_file(review_dossier),
                },
            },
            "source_free_artifacts": {
                "review_pack": review_pack.name,
                "worksheet": {
                    "path": worksheet.name,
                    "sha256": sha256_file(worksheet),
                },
            },
            "next_required_steps": [
                "Inspect every review-dossier candidate against its review_focus and Atlas hazards.",
                "Record only explicit human dispositions in review-worksheet.json; keep it source-text-free.",
                "Finalize the worksheet with tools/r1b_configuration_review.py finalize.",
                "Run tools/vanilla_source_gate.py against the finalized records and pinned Atlas database.",
                "Do not admit Configuration unless the source gate reports admitted=true with no failures.",
            ],
        }
        _write_manifest(manifest_path, manifest)
        return manifest
    except Exception:
        shutil.rmtree(session, ignore_errors=True)
        raise


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-configuration-review-session")
    parser.add_argument(
        "--output-dir",
        required=True,
        type=Path,
        help="fresh directory outside the repository; contains official source text",
    )
    parser.add_argument("--db", type=Path, default=source_probe.DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=source_probe.DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=source_probe.DEFAULT_LOCK)
    parser.add_argument("--frontier", type=Path, default=source_probe.DEFAULT_FRONTIER)
    parser.add_argument("--plan", type=Path, default=review.DEFAULT_PLAN)
    parser.add_argument("--semantics", type=Path, default=review.DEFAULT_SEMANTICS)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        manifest = build_session(
            output_dir=args.output_dir,
            db=args.db,
            source_archive=args.source,
            lock_path=args.lock,
            frontier_path=args.frontier,
            plan_path=args.plan,
            semantics_path=args.semantics,
        )
    except (
        OSError,
        ReviewSessionError,
        source_probe.ProbeError,
        bundle_review.ReviewPackError,
        dossier.DossierError,
        review.ReviewError,
    ) as error:
        print(f"R1B Configuration review session error: {error}", file=sys.stderr)
        return 2

    session = args.output_dir.resolve(strict=False)
    review_pack = session / "review-pack"
    worksheet = session / "review-worksheet.json"
    reviewed = session / "reviewed"
    gate = reviewed / "gate" / f"{bundle_review.GATE_ID}.json"
    records = reviewed / "records"
    source_gate_output = session / f"{bundle_review.GATE_ID}-source-admission.json"

    print(f"review_session={session}")
    print(f"candidates={manifest['candidate_count']}")
    print("contains_official_source_text=true")
    print("commit_policy=EPHEMERAL_DO_NOT_COMMIT")
    print(f"review_dossier={session / 'review-dossier.json'}")
    print(f"worksheet={worksheet}")
    print("next_finalize_command=")
    print(
        "python3 tools/r1b_configuration_review.py finalize "
        f"--review-pack {review_pack} --worksheet {worksheet} --output-dir {reviewed}"
    )
    print("next_source_gate_command=")
    print(
        "python3 tools/vanilla_source_gate.py "
        f"--db {args.db} --gate {gate} --records {records} --output {source_gate_output}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
