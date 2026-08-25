#!/usr/bin/env python3
"""Prepare the bounded source review needed to production-admit R2 Play liveness."""
from __future__ import annotations

import argparse
import json
import shutil
import sqlite3
import sys
import zipfile
from pathlib import Path
from typing import Sequence

try:
    from . import r1b_configuration_source_probe as source_probe
    from . import r1b_play_entry_source_review as review_support
    from . import vanilla_atlas
except ImportError:  # Direct `python3 tools/...` execution.
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]
    import r1b_play_entry_source_review as review_support  # type: ignore[no-redef]
    import vanilla_atlas  # type: ignore[no-redef]

SCHEMA = 1
PLAN_ID = "REVIEW-NET-R2-PLAY-LIVENESS-26_2-001"
PREPARED_KIND = "r2-play-liveness-source-review"
WORKSHEET_KIND = "r2-play-liveness-source-review-worksheet"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PLAN = REPO_ROOT / "vanilla/reviews/network/r2-play-liveness-source-review-plan.json"

Candidate = review_support.Candidate


class PlayLivenessReviewError(RuntimeError):
    """Fail-closed R2 Play-liveness review error."""


def load_plan(path: Path = DEFAULT_PLAN) -> tuple[Candidate, ...]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise PlayLivenessReviewError(f"invalid Play-liveness review plan JSON: {error}") from error
    if not isinstance(value, dict) or set(value) != {"schema", "id", "candidates"}:
        raise PlayLivenessReviewError("Play-liveness review plan has unexpected fields")
    if value["schema"] != SCHEMA or value["id"] != PLAN_ID:
        raise PlayLivenessReviewError("Play-liveness review plan identity mismatch")
    raw = value["candidates"]
    if not isinstance(raw, list) or not raw:
        raise PlayLivenessReviewError("Play-liveness review plan candidates must be non-empty")
    try:
        candidates = tuple(
            review_support._candidate(item, index) for index, item in enumerate(raw)
        )
    except review_support.PlayEntryReviewError as error:
        raise PlayLivenessReviewError(str(error)) from error
    ids = [candidate.var_id for candidate in candidates]
    selectors = [review_support.selector_key(candidate) for candidate in candidates]
    if len(ids) != len(set(ids)):
        raise PlayLivenessReviewError("Play-liveness review plan contains duplicate candidate ids")
    if len(selectors) != len(set(selectors)):
        raise PlayLivenessReviewError(
            "Play-liveness review plan contains duplicate effective selectors"
        )
    return candidates


CANDIDATES = load_plan()


def _external_fresh_dir(path: Path) -> Path:
    try:
        return review_support._external_fresh_dir(path)
    except review_support.PlayEntryReviewError as error:
        raise PlayLivenessReviewError(str(error)) from error


def _resolve_all(conn: sqlite3.Connection) -> list[tuple[Candidate, sqlite3.Row]]:
    resolved: list[tuple[Candidate, sqlite3.Row]] = []
    failures: list[str] = []
    for candidate in CANDIDATES:
        try:
            resolved.append((candidate, review_support._resolve(conn, candidate)))
        except review_support.PlayEntryReviewError as error:
            failures.append(str(error))
    if failures:
        details = "\n".join(f"  - {failure}" for failure in failures)
        raise PlayLivenessReviewError(f"selector preflight failed:\n{details}")
    return resolved


def prepare(output_dir: Path, db: Path, source: Path, lock: Path) -> dict[str, object]:
    """Create source-rich and source-free review artifacts outside the repository."""
    output = _external_fresh_dir(output_dir)
    output.mkdir(parents=True)
    conn: sqlite3.Connection | None = None
    try:
        conn = vanilla_atlas.connect_db(db)
        source_sha = source_probe.require_pinned_source(conn, source, lock)
        resolved = _resolve_all(conn)
        dossier_candidates: list[dict[str, object]] = []
        worksheet_candidates: list[dict[str, object]] = []
        with zipfile.ZipFile(source) as archive:
            for candidate, row in resolved:
                template = source_probe.record_template(conn, row, candidate.var_id)
                source_record = dict(template["source"])
                hazards = sorted(set(template.get("atlas_observed_hazards", [])))
                identity = f"{source_record['type']}#{source_record['signature']}"
                excerpt = review_support._source_excerpt(archive, row)
                common = {
                    "candidate_id": candidate.var_id,
                    "source_identity": identity,
                    "source": source_record,
                    "atlas_observed_hazards": hazards,
                    "review_focus": list(candidate.review_focus),
                }
                dossier_candidates.append(
                    {
                        **common,
                        "path": str(row["path"]),
                        "start_line": int(row["start_line"]),
                        "end_line": int(row["end_line"]),
                        "source_excerpt": excerpt,
                        "source_excerpt_sha256": review_support.sha256_bytes(
                            excerpt.encode("utf-8")
                        ),
                    }
                )
                worksheet_candidates.append(
                    {
                        **common,
                        "decision": {
                            "source_inspected": False,
                            "accepted": False,
                            "hazards_reviewed": [],
                            "semantic_rules": [],
                            "followup_dependencies": [],
                            "note": "",
                        },
                    }
                )

        dossier = {
            "schema": SCHEMA,
            "kind": PREPARED_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "source_archive_sha256": source_sha,
            "candidate_count": len(CANDIDATES),
            "candidates": dossier_candidates,
        }
        worksheet = {
            "schema": SCHEMA,
            "kind": WORKSHEET_KIND,
            "contains_official_source_text": False,
            "source_archive_sha256": source_sha,
            "candidate_count": len(CANDIDATES),
            "candidates": worksheet_candidates,
        }
        (output / "review-dossier.json").write_bytes(review_support.pretty_bytes(dossier))
        (output / "review-worksheet.json").write_bytes(review_support.pretty_bytes(worksheet))
        manifest = {
            "schema": SCHEMA,
            "kind": PREPARED_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "source_archive_sha256": source_sha,
            "candidate_count": len(CANDIDATES),
            "artifacts": {
                "review_dossier": "review-dossier.json",
                "review_worksheet": "review-worksheet.json",
            },
            "next_required_step": (
                "Inspect all twelve exact bodies, fill only the source-free worksheet, then finalize "
                "the R2 Play-liveness VAR/protocol contract. The dossier must not be committed."
            ),
        }
        (output / "manifest.json").write_bytes(review_support.pretty_bytes(manifest))
        return manifest
    except Exception:
        shutil.rmtree(output, ignore_errors=True)
        raise
    finally:
        if conn is not None:
            conn.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r2-play-liveness-source-review")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--db", type=Path, default=source_probe.DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=source_probe.DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=source_probe.DEFAULT_LOCK)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        manifest = prepare(args.output_dir, args.db, args.source, args.lock)
        print(f"play_liveness_review={args.output_dir}")
        print(f"candidates={manifest['candidate_count']}")
        print("contains_official_source_text=true")
        print("commit_policy=EPHEMERAL_DO_NOT_COMMIT")
        print(f"worksheet={args.output_dir / 'review-worksheet.json'}")
        return 0
    except (
        OSError,
        json.JSONDecodeError,
        zipfile.BadZipFile,
        PlayLivenessReviewError,
        source_probe.ProbeError,
    ) as error:
        print(f"R2 Play-liveness source-review error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
