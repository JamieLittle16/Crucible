#!/usr/bin/env python3
"""Build a local, explicitly non-committable source-review dossier for R1B Configuration.

The commit-safe review pack intentionally strips Mojang source text. For manual review, however,
reviewers should not have to navigate 25 declarations by hand. This tool reuses the same pinned
source archive, Atlas database, canonical candidate set and review plan to collect the exact Atlas
indexed source span for every uniquely resolved candidate into one local JSON dossier.

The output contains official source text and is therefore always `EPHEMERAL_DO_NOT_COMMIT`.
Nothing is emitted unless an explicit `--output` path is supplied.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import zipfile
from pathlib import Path, PurePosixPath
from typing import Any, Mapping, Sequence

try:
    from . import r1b_configuration_review as review
    from . import r1b_configuration_source_probe as source_probe
    from . import vanilla_atlas
except ImportError:  # Direct `python3 tools/...` execution.
    import r1b_configuration_review as review  # type: ignore[no-redef]
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]
    import vanilla_atlas  # type: ignore[no-redef]

SCHEMA = 1
DOSSIER_KIND = "r1b-configuration-ephemeral-review-dossier"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"


class DossierError(RuntimeError):
    """Fail-closed local source-dossier error."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _safe_archive_path(value: object) -> str:
    if not isinstance(value, str) or not value:
        raise DossierError("Atlas source path must be a non-empty string")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise DossierError(f"unsafe Atlas source path: {value!r}")
    return value


def extract_indexed_source_span(archive: zipfile.ZipFile, row: Mapping[str, object]) -> str:
    """Extract the exact source lines bound to one Atlas method row."""
    path = _safe_archive_path(row["path"])
    names = archive.namelist()
    if path not in names:
        raise DossierError(f"Atlas source member missing from pinned archive: {path}")
    try:
        source = archive.read(path).decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise DossierError(f"Atlas source member is not strict UTF-8: {path}: {error}") from error
    start = row["start_line"]
    end = row["end_line"]
    if type(start) is not int or type(end) is not int or start < 1 or end < start:
        raise DossierError(f"invalid Atlas source line span for {path}: {start!r}-{end!r}")
    lines = source.splitlines(keepends=True)
    if end > len(lines):
        raise DossierError(
            f"Atlas source line span exceeds member length for {path}: {start}-{end} > {len(lines)}"
        )
    excerpt = "".join(lines[start - 1 : end])
    if not excerpt.strip():
        raise DossierError(f"Atlas source line span is empty for {path}:{start}-{end}")
    return excerpt


def _candidate_plan(plan: Mapping[str, object]) -> list[dict[str, Any]]:
    candidates = plan.get("candidates")
    if not isinstance(candidates, list):
        raise DossierError("review plan candidates must be an array")
    return [dict(item) for item in candidates if isinstance(item, dict)]


def build_dossier(
    *,
    db: Path,
    source_archive: Path,
    lock_path: Path = source_probe.DEFAULT_LOCK,
    plan_path: Path = review.DEFAULT_PLAN,
    semantics_path: Path = review.DEFAULT_SEMANTICS,
) -> dict[str, object]:
    """Resolve and extract every R1B review candidate from the exact pinned source archive."""
    plan, plan_raw = review.load_review_plan(plan_path, semantics_path)
    _semantic_rules, semantics_sha = review.load_semantic_rules(semantics_path)
    plan_candidates = _candidate_plan(plan)
    if len(plan_candidates) != len(source_probe.CANDIDATES):
        raise DossierError("review plan candidate cardinality drifted")

    conn = vanilla_atlas.connect_db(db)
    try:
        source_sha = source_probe.require_pinned_source(conn, source_archive, lock_path)
        meta = dict(conn.execute("SELECT key,value FROM meta"))
        fingerprint_algorithm = str(
            meta.get("fingerprint_algorithm", vanilla_atlas.FINGERPRINT_ALGORITHM)
        )
        dossier_candidates: list[dict[str, object]] = []
        with zipfile.ZipFile(source_archive) as archive:
            for index, ((var_id, query), plan_candidate) in enumerate(
                zip(source_probe.CANDIDATES, plan_candidates)
            ):
                if plan_candidate.get("var_id") != var_id or plan_candidate.get("query") != query:
                    raise DossierError(f"review plan candidate {index} drifted from the source probe")
                rows = vanilla_atlas.resolve_methods(conn, query)
                if len(rows) != 1:
                    identities = [source_probe._source_identity(row) for row in rows[:20]]
                    raise DossierError(
                        f"{var_id} must resolve exactly once for dossier generation; "
                        f"matches={len(rows)} candidates={identities}"
                    )
                row = rows[0]
                excerpt = extract_indexed_source_span(archive, row)
                hashes = conn.execute(
                    "SELECT normalized_sha256,body_sha256 FROM methods WHERE id=?",
                    (int(row["id"]),),
                ).fetchone()
                if hashes is None:
                    raise DossierError(f"Atlas fingerprint row missing for {var_id}")
                hazards = [
                    item[0]
                    for item in conn.execute(
                        "SELECT DISTINCT kind FROM hazards WHERE method_id=? ORDER BY kind",
                        (int(row["id"]),),
                    )
                ]
                dossier_candidates.append(
                    {
                        "var_id": var_id,
                        "query": query,
                        "source_identity": source_probe._source_identity(row),
                        "path": _safe_archive_path(row["path"]),
                        "start_line": int(row["start_line"]),
                        "end_line": int(row["end_line"]),
                        "fingerprint_algorithm": fingerprint_algorithm,
                        "normalized_sha256": str(hashes[0]),
                        "body_sha256": str(hashes[1]),
                        "atlas_observed_hazards": hazards,
                        "semantic_rule_candidates": plan_candidate["semantic_rule_candidates"],
                        "review_focus": plan_candidate["review_focus"],
                        "source_excerpt_sha256": sha256_bytes(excerpt.encode("utf-8")),
                        "source_excerpt": excerpt,
                    }
                )
    finally:
        conn.close()

    return {
        "schema": SCHEMA,
        "kind": DOSSIER_KIND,
        "commit_policy": COMMIT_POLICY,
        "contains_official_source_text": True,
        "source_archive_sha256": source_sha,
        "review_plan_sha256": sha256_bytes(plan_raw),
        "semantic_contract_sha256": semantics_sha,
        "capture_semantic_rules": plan["capture_semantic_rules"],
        "candidates": dossier_candidates,
    }


def write_dossier(path: Path, dossier: Mapping[str, object]) -> None:
    """Write the explicitly local dossier deterministically."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(dossier, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-configuration-review-dossier")
    parser.add_argument("--db", type=Path, default=source_probe.DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=source_probe.DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=source_probe.DEFAULT_LOCK)
    parser.add_argument("--plan", type=Path, default=review.DEFAULT_PLAN)
    parser.add_argument("--semantics", type=Path, default=review.DEFAULT_SEMANTICS)
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="explicit local JSON path; output contains official source text and must not be committed",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        dossier = build_dossier(
            db=args.db,
            source_archive=args.source,
            lock_path=args.lock,
            plan_path=args.plan,
            semantics_path=args.semantics,
        )
        write_dossier(args.output, dossier)
    except (OSError, zipfile.BadZipFile, DossierError, source_probe.ProbeError, review.ReviewError) as error:
        print(f"R1B Configuration review dossier error: {error}", file=sys.stderr)
        return 2
    print(f"review_dossier={args.output}")
    print(f"candidates={len(dossier['candidates'])}")
    print("contains_official_source_text=true")
    print("commit_policy=EPHEMERAL_DO_NOT_COMMIT")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
