#!/usr/bin/env python3
"""Prepare a bounded source-rich discovery review for the R1B fresh-player Play entry."""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sqlite3
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence

try:
    from . import r1b_configuration_source_probe as source_probe
    from . import vanilla_atlas
except ImportError:
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]
    import vanilla_atlas  # type: ignore[no-redef]

SCHEMA = 1
PLAN_ID = "REVIEW-NET-R1B-PLAY-ENTRY-26_2-001"
PREPARED_KIND = "r1b-play-entry-source-review"
WORKSHEET_KIND = "r1b-play-entry-source-review-worksheet"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PLAN = REPO_ROOT / "vanilla/reviews/network/r1b-play-entry-source-review-plan.json"
ROUTE_DISPOSITIONS = (
    "MANDATORY",
    "CONDITIONAL",
    "DEFAULT_EMPTY",
    "INTERNAL_ONLY",
    "DELEGATED_REVIEW_REQUIRED",
)


@dataclass(frozen=True)
class Candidate:
    var_id: str
    type_name: str
    method_name: str
    param_count: int
    review_focus: tuple[str, ...]
    exact_signature: str | None = None


class PlayEntryReviewError(RuntimeError):
    """Fail-closed Play-entry source-review error."""


def _candidate(value: object, index: int) -> Candidate:
    if not isinstance(value, dict):
        raise PlayEntryReviewError(f"plan candidate {index} must be an object")
    required = {"var_id", "type_name", "method_name", "param_count", "review_focus"}
    allowed = required | {"exact_signature"}
    if not required <= set(value) or not set(value) <= allowed:
        raise PlayEntryReviewError(f"plan candidate {index} has unexpected or missing fields")
    focus = value["review_focus"]
    if not isinstance(focus, list) or not focus or any(
        not isinstance(item, str) or not item for item in focus
    ):
        raise PlayEntryReviewError(f"plan candidate {index} review_focus must be non-empty strings")
    param_count = value["param_count"]
    if type(param_count) is not int or param_count < 0:
        raise PlayEntryReviewError(f"plan candidate {index} param_count must be non-negative integer")
    exact_signature = value.get("exact_signature")
    if exact_signature is not None and (
        not isinstance(exact_signature, str) or not exact_signature.strip()
    ):
        raise PlayEntryReviewError(f"plan candidate {index} exact_signature must be non-empty")
    return Candidate(
        var_id=str(value["var_id"]),
        type_name=str(value["type_name"]),
        method_name=str(value["method_name"]),
        param_count=param_count,
        review_focus=tuple(focus),
        exact_signature=exact_signature,
    )


def selector_key(candidate: Candidate) -> tuple[object, ...]:
    if candidate.exact_signature is not None:
        return ("exact", candidate.type_name, candidate.exact_signature)
    return ("arity", candidate.type_name, candidate.method_name, candidate.param_count)


def load_plan(path: Path = DEFAULT_PLAN) -> tuple[Candidate, ...]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise PlayEntryReviewError(f"invalid Play-entry review plan JSON: {error}") from error
    if not isinstance(value, dict) or set(value) != {"schema", "id", "candidates"}:
        raise PlayEntryReviewError("Play-entry review plan has unexpected fields")
    if value["schema"] != SCHEMA or value["id"] != PLAN_ID:
        raise PlayEntryReviewError("Play-entry review plan identity mismatch")
    raw = value["candidates"]
    if not isinstance(raw, list) or not raw:
        raise PlayEntryReviewError("Play-entry review plan candidates must be non-empty")
    candidates = tuple(_candidate(item, index) for index, item in enumerate(raw))
    ids = [candidate.var_id for candidate in candidates]
    selectors = [selector_key(candidate) for candidate in candidates]
    if len(ids) != len(set(ids)):
        raise PlayEntryReviewError("Play-entry review plan contains duplicate candidate ids")
    if len(selectors) != len(set(selectors)):
        raise PlayEntryReviewError("Play-entry review plan contains duplicate effective selectors")
    return candidates


CANDIDATES = load_plan()


def pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _external_fresh_dir(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise PlayEntryReviewError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repo = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repo)
    except ValueError:
        return resolved
    raise PlayEntryReviewError(
        "Play-entry review contains official source text and must live outside the repository"
    )


def _resolve(conn: sqlite3.Connection, candidate: Candidate) -> sqlite3.Row:
    select = """SELECT m.id,t.qualified_name,m.name,m.signature,m.param_count,
                       m.start_line,m.end_line,f.path
                FROM methods m JOIN types t ON t.id=m.type_id
                JOIN source_files f ON f.id=t.file_id"""
    if candidate.exact_signature is not None:
        rows = conn.execute(
            select + " WHERE t.qualified_name=? AND m.signature=? ORDER BY m.start_line",
            (candidate.type_name, candidate.exact_signature),
        ).fetchall()
        if len(rows) != 1:
            identities = [f"{row['qualified_name']}#{row['signature']}" for row in rows]
            raise PlayEntryReviewError(
                f"{candidate.var_id}: exact selector {candidate.type_name}#"
                f"{candidate.exact_signature} resolved {len(rows)} methods: {identities}"
            )
        row = rows[0]
        if row["name"] != candidate.method_name or int(row["param_count"]) != candidate.param_count:
            raise PlayEntryReviewError(
                f"{candidate.var_id}: exact signature disagrees with declared method/arity"
            )
        return row
    rows = conn.execute(
        select + " WHERE t.qualified_name=? AND m.name=? AND m.param_count=? ORDER BY m.start_line",
        (candidate.type_name, candidate.method_name, candidate.param_count),
    ).fetchall()
    if len(rows) != 1:
        identities = [f"{row['qualified_name']}#{row['signature']}" for row in rows]
        raise PlayEntryReviewError(
            f"{candidate.var_id}: {candidate.type_name}#{candidate.method_name}/"
            f"{candidate.param_count} resolved {len(rows)} methods: {identities}"
        )
    return rows[0]


def _resolve_all(conn: sqlite3.Connection) -> list[tuple[Candidate, sqlite3.Row]]:
    resolved: list[tuple[Candidate, sqlite3.Row]] = []
    failures: list[str] = []
    for candidate in CANDIDATES:
        try:
            resolved.append((candidate, _resolve(conn, candidate)))
        except PlayEntryReviewError as error:
            failures.append(str(error))
    if failures:
        details = "\n".join(f"  - {failure}" for failure in failures)
        raise PlayEntryReviewError(f"selector preflight failed:\n{details}")
    return resolved


def _source_excerpt(archive: zipfile.ZipFile, row: sqlite3.Row) -> str:
    path = str(row["path"])
    try:
        text = archive.read(path).decode("utf-8", errors="strict")
    except KeyError as error:
        raise PlayEntryReviewError(f"source member missing: {path}") from error
    lines = text.splitlines()
    start, end = int(row["start_line"]), int(row["end_line"])
    if not (1 <= start <= end <= len(lines)):
        raise PlayEntryReviewError(f"invalid Atlas line range for {path}: {start}-{end}")
    return "\n".join(lines[start - 1 : end]) + "\n"


def prepare(
    output_dir: Path,
    db: Path,
    source: Path,
    lock: Path,
) -> dict[str, object]:
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
                excerpt = _source_excerpt(archive, row)
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
                        "source_excerpt_sha256": sha256_bytes(excerpt.encode("utf-8")),
                    }
                )
                worksheet_candidates.append(
                    {
                        **common,
                        "decision": {
                            "source_inspected": False,
                            "accepted": False,
                            "route_disposition": "",
                            "hazards_reviewed": [],
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
            "route_dispositions": list(ROUTE_DISPOSITIONS),
            "candidates": dossier_candidates,
        }
        worksheet = {
            "schema": SCHEMA,
            "kind": WORKSHEET_KIND,
            "contains_official_source_text": False,
            "source_archive_sha256": source_sha,
            "candidate_count": len(CANDIDATES),
            "route_dispositions": list(ROUTE_DISPOSITIONS),
            "candidates": worksheet_candidates,
        }
        (output / "review-dossier.json").write_bytes(pretty_bytes(dossier))
        (output / "review-worksheet.json").write_bytes(pretty_bytes(worksheet))
        manifest = {
            "schema": SCHEMA,
            "kind": PREPARED_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "candidate_count": len(CANDIDATES),
            "source_archive_sha256": source_sha,
            "artifacts": {
                "review_dossier": "review-dossier.json",
                "review_worksheet": "review-worksheet.json",
            },
            "next_required_step": (
                "Inspect the exact source bodies, classify selected-route publication, and build a "
                "smaller final Play-entry VAR/SEM gate. This discovery worksheet is not admission."
            ),
        }
        (output / "manifest.json").write_bytes(pretty_bytes(manifest))
        return manifest
    except Exception:
        shutil.rmtree(output, ignore_errors=True)
        raise
    finally:
        if conn is not None:
            conn.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-play-entry-source-review")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--db", type=Path, default=source_probe.DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=source_probe.DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=source_probe.DEFAULT_LOCK)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        manifest = prepare(args.output_dir, args.db, args.source, args.lock)
        print(f"play_entry_review={args.output_dir}")
        print(f"candidates={manifest['candidate_count']}")
        print("contains_official_source_text=true")
        print("commit_policy=EPHEMERAL_DO_NOT_COMMIT")
        print(f"worksheet={args.output_dir / 'review-worksheet.json'}")
        return 0
    except (
        OSError,
        json.JSONDecodeError,
        zipfile.BadZipFile,
        PlayEntryReviewError,
        source_probe.ProbeError,
    ) as error:
        print(f"R1B Play-entry source-review error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
