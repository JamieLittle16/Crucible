#!/usr/bin/env python3
"""Collect the last local evidence bundle for the selected R2B Play-entry profile.

This wrapper deliberately combines two different evidence classes without conflating their trust:

* a source-rich, ephemeral review of the tiny reusable dynamic-codec seam; and
* a source-free black-box oracle for the two composition-stable Play publications.

The output directory is source-rich as a whole and must remain outside the repository. Nothing in
this tool performs semantic review or production admission; it only prepares the bounded evidence
that the R2B finalizer consumes after the source bodies have been inspected and the worksheet has
been completed.
"""
from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path
from typing import Sequence

try:
    from . import r2b_play_bootstrap_oracle_extract as oracle_extract
    from . import r2b_play_entry_final_seams_source_review as final_seams
except ImportError:  # Direct `python3 tools/...` execution.
    import r2b_play_bootstrap_oracle_extract as oracle_extract  # type: ignore[no-redef]
    import r2b_play_entry_final_seams_source_review as final_seams  # type: ignore[no-redef]

SCHEMA = 1
KIND = "r2b-play-entry-final-evidence-bundle-v1"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DB = Path(".crucible/vanilla/atlas.sqlite")
DEFAULT_SOURCE = Path.home() / "Documents/mc-source/mc-src.zip"
DEFAULT_LOCK = REPO_ROOT / "vanilla/vanilla.lock.toml"


class CollectError(ValueError):
    """Raised when the combined evidence collection cannot be performed safely."""


def _external_fresh_dir(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise CollectError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repo = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repo)
    except ValueError:
        return resolved
    raise CollectError(
        "combined evidence contains official source text and must live outside the repository"
    )


def collect(
    *,
    output_dir: Path,
    prior_117_dossier: Path,
    r1x_replay: Path,
    db: Path,
    source: Path,
    lock: Path,
) -> dict[str, object]:
    output = _external_fresh_dir(output_dir)
    seams_dir = output / "final-seams"
    oracle_path = output / "composition-oracle.json"
    output.mkdir(parents=True)
    try:
        seams_manifest = final_seams.prepare(
            seams_dir,
            prior_117_dossier,
            db,
            source,
            lock,
        )
        oracle_value = oracle_extract.extract(oracle_extract._read(r1x_replay))
        oracle_extract.write(oracle_path, oracle_value)

        manifest = {
            "schema": SCHEMA,
            "kind": KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "production_admitted": False,
            "source_review": {
                "id": seams_manifest["id"],
                "candidate_count": seams_manifest["candidate_count"],
                "group_counts": seams_manifest["group_counts"],
                "directory": "final-seams",
                "review_dossier": "final-seams/review-dossier.json",
                "review_worksheet": "final-seams/review-worksheet.json",
                "trust": "SOURCE_RICH_REQUIRES_HUMAN_REVIEW",
            },
            "composition_oracle": {
                "path": "composition-oracle.json",
                "artifact_count": len(oracle_value["artifacts"]),
                "capture_sha256": oracle_value["target"]["capture_sha256"],
                "trust": "BLACK_BOX_CONFIRMATION_ONLY",
                "production_admitted": False,
            },
            "next_required_step": (
                "Inspect every final-seam source body and complete review-worksheet.json; then run "
                "r2b_play_entry_finalize.py with the reviewed source dossiers and composition oracle."
            ),
        }
        (output / "manifest.json").write_text(
            json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
        return manifest
    except Exception:
        shutil.rmtree(output, ignore_errors=True)
        raise


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--prior-117-dossier", type=Path, required=True)
    parser.add_argument("--r1x-replay", type=Path, required=True)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        manifest = collect(
            output_dir=args.output_dir,
            prior_117_dossier=args.prior_117_dossier,
            r1x_replay=args.r1x_replay,
            db=args.db,
            source=args.source,
            lock=args.lock,
        )
    except (
        CollectError,
        final_seams.FinalSeamsError,
        oracle_extract.OracleExtractError,
        OSError,
        KeyError,
        TypeError,
        ValueError,
    ) as error:
        print(f"R2B final evidence collection error: {error}", file=sys.stderr)
        return 2

    source_review = manifest["source_review"]
    oracle = manifest["composition_oracle"]
    assert isinstance(source_review, dict) and isinstance(oracle, dict)
    print(f"r2b_final_evidence={args.output_dir}")
    print(f"final_seam_candidates={source_review['candidate_count']}")
    print(f"oracle_artifacts={oracle['artifact_count']}")
    print("contains_official_source_text=true")
    print(f"commit_policy={COMMIT_POLICY}")
    print("production_admitted=false")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
