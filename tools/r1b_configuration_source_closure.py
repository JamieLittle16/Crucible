#!/usr/bin/env python3
"""Prepare/finalize the supplemental R1B Configuration source-closure review."""
from __future__ import annotations

import argparse
import hashlib
import json
import shlex
import shutil
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
GATE_ID = "GATE-NET-CONFIG-CLOSURE-26_2-001"
PREPARED_KIND = "r1b-configuration-source-closure-review"
WORKSHEET_KIND = "r1b-configuration-source-closure-worksheet"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PLAN = REPO_ROOT / "vanilla/reviews/network/r1b-configuration-source-closure-plan.json"


@dataclass(frozen=True)
class Candidate:
    var_id: str
    type_name: str
    method_name: str
    param_count: int
    semantic_rules: tuple[str, ...]
    review_focus: tuple[str, ...]
    exact_signature: str | None = None


class ClosureError(RuntimeError):
    """Fail-closed supplemental review error."""


def _candidate(value: object, index: int) -> Candidate:
    if not isinstance(value, dict):
        raise ClosureError(f"plan candidate {index} must be an object")
    required = {
        "var_id",
        "type_name",
        "method_name",
        "param_count",
        "semantic_rules",
        "review_focus",
    }
    allowed = required | {"exact_signature"}
    keys = set(value)
    if not required <= keys or not keys <= allowed:
        raise ClosureError(f"plan candidate {index} has unexpected or missing fields")
    rules = value["semantic_rules"]
    focus = value["review_focus"]
    if not isinstance(rules, list) or not rules or any(
        not isinstance(item, str) or not item for item in rules
    ):
        raise ClosureError(f"plan candidate {index} semantic_rules must be non-empty strings")
    if not isinstance(focus, list) or not focus or any(
        not isinstance(item, str) or not item for item in focus
    ):
        raise ClosureError(f"plan candidate {index} review_focus must be non-empty strings")
    param_count = value["param_count"]
    if type(param_count) is not int or param_count < 0:
        raise ClosureError(f"plan candidate {index} param_count must be non-negative integer")
    exact_signature = value.get("exact_signature")
    if exact_signature is not None and (
        not isinstance(exact_signature, str) or not exact_signature.strip()
    ):
        raise ClosureError(f"plan candidate {index} exact_signature must be a non-empty string")
    return Candidate(
        var_id=str(value["var_id"]),
        type_name=str(value["type_name"]),
        method_name=str(value["method_name"]),
        param_count=param_count,
        semantic_rules=tuple(rules),
        review_focus=tuple(focus),
        exact_signature=exact_signature,
    )


def _selector_key(candidate: Candidate) -> tuple[object, ...]:
    if candidate.exact_signature is not None:
        return ("exact", candidate.type_name, candidate.exact_signature)
    return ("arity", candidate.type_name, candidate.method_name, candidate.param_count)


def load_plan(path: Path = DEFAULT_PLAN) -> tuple[Candidate, ...]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise ClosureError(f"invalid closure plan JSON: {error}") from error
    if not isinstance(value, dict) or set(value) != {"schema", "gate_id", "candidates"}:
        raise ClosureError("closure plan has unexpected fields")
    if value["schema"] != SCHEMA or value["gate_id"] != GATE_ID:
        raise ClosureError("closure plan identity mismatch")
    raw_candidates = value["candidates"]
    if not isinstance(raw_candidates, list) or not raw_candidates:
        raise ClosureError("closure plan candidates must be non-empty")
    candidates = tuple(_candidate(item, index) for index, item in enumerate(raw_candidates))
    ids = [item.var_id for item in candidates]
    selectors = [_selector_key(item) for item in candidates]
    if len(ids) != len(set(ids)):
        raise ClosureError("closure plan contains duplicate VAR ids")
    if len(selectors) != len(set(selectors)):
        raise ClosureError("closure plan contains duplicate selectors")
    return candidates


CANDIDATES = load_plan()


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode(
        "utf-8"
    )


def _external_fresh_dir(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise ClosureError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repo = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repo)
    except ValueError:
        return resolved
    raise ClosureError(
        "source-closure review contains official source text and must live outside the repository"
    )


def _resolve(conn: Any, candidate: Candidate) -> Any:
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
            raise ClosureError(
                f"{candidate.var_id}: exact selector {candidate.type_name}#"
                f"{candidate.exact_signature} resolved {len(rows)} methods: {identities}"
            )
        row = rows[0]
        if row["name"] != candidate.method_name or int(row["param_count"]) != candidate.param_count:
            raise ClosureError(
                f"{candidate.var_id}: exact signature disagrees with declared method/arity "
                f"{candidate.method_name}/{candidate.param_count}: "
                f"{row['qualified_name']}#{row['signature']}"
            )
        return row

    rows = conn.execute(
        select
        + " WHERE t.qualified_name=? AND m.name=? AND m.param_count=? ORDER BY m.start_line",
        (candidate.type_name, candidate.method_name, candidate.param_count),
    ).fetchall()
    if len(rows) != 1:
        identities = [f"{row['qualified_name']}#{row['signature']}" for row in rows]
        raise ClosureError(
            f"{candidate.var_id}: {candidate.type_name}#{candidate.method_name}/"
            f"{candidate.param_count} resolved {len(rows)} methods: {identities}"
        )
    return rows[0]


def _resolve_all(conn: Any) -> list[tuple[Candidate, Any]]:
    """Resolve every selector before source-rich output so ambiguity is reported in one pass."""
    resolved: list[tuple[Candidate, Any]] = []
    failures: list[str] = []
    for candidate in CANDIDATES:
        try:
            resolved.append((candidate, _resolve(conn, candidate)))
        except ClosureError as error:
            failures.append(str(error))
    if failures:
        details = "\n".join(f"  - {failure}" for failure in failures)
        raise ClosureError(f"selector preflight failed:\n{details}")
    return resolved


def _source_excerpt(archive: zipfile.ZipFile, row: Any) -> str:
    path = str(row["path"])
    try:
        text = archive.read(path).decode("utf-8", errors="strict")
    except KeyError as error:
        raise ClosureError(f"source member missing: {path}") from error
    lines = text.splitlines()
    start, end = int(row["start_line"]), int(row["end_line"])
    if not (1 <= start <= end <= len(lines)):
        raise ClosureError(f"invalid Atlas line range for {path}: {start}-{end}")
    return "\n".join(lines[start - 1 : end]) + "\n"


def prepare(output_dir: Path, db: Path, source: Path, lock: Path) -> dict[str, object]:
    output = _external_fresh_dir(output_dir)
    output.mkdir(parents=True)
    conn = None
    try:
        conn = vanilla_atlas.connect_db(db)
        source_sha = source_probe.require_pinned_source(conn, source, lock)
        resolved_candidates = _resolve_all(conn)
        dossier_candidates: list[dict[str, object]] = []
        worksheet_candidates: list[dict[str, object]] = []
        records: list[dict[str, object]] = []
        methods: list[dict[str, str]] = []
        with zipfile.ZipFile(source) as archive:
            for candidate, row in resolved_candidates:
                record = source_probe.record_template(conn, row, candidate.var_id)
                source_record = dict(record["source"])
                identity = f"{source_record['type']}#{source_record['signature']}"
                hazards = sorted(set(record.get("atlas_observed_hazards", [])))
                excerpt = _source_excerpt(archive, row)
                common = {
                    "var_id": candidate.var_id,
                    "source_identity": identity,
                    "source": source_record,
                    "atlas_observed_hazards": hazards,
                    "semantic_rule_candidates": list(candidate.semantic_rules),
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
                            "reviewer": "",
                            "note": "",
                            "hazards_reviewed": [],
                            "semantic_rules": [],
                        },
                    }
                )
                record.pop("atlas_observed_hazards", None)
                records.append(record)
                methods.append({"query": identity, "var_id": candidate.var_id})

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
        gate = {
            "schema": SCHEMA,
            "id": GATE_ID,
            "minimum_status": "VAR_REVIEWED",
            "require_semantic_rules": True,
            "require_hazards_reviewed": True,
            "methods": methods,
        }
        (output / "records").mkdir()
        (output / "gate").mkdir()
        for record in records:
            (output / "records" / f"{record['id']}.json").write_bytes(pretty_bytes(record))
        (output / "review-dossier.json").write_bytes(pretty_bytes(dossier))
        (output / "review-worksheet.json").write_bytes(pretty_bytes(worksheet))
        (output / "gate" / f"{GATE_ID}.json").write_bytes(pretty_bytes(gate))
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
                "indexed_records": "records",
                "gate": f"gate/{GATE_ID}.json",
            },
        }
        (output / "manifest.json").write_bytes(pretty_bytes(manifest))
        return manifest
    except Exception:
        shutil.rmtree(output, ignore_errors=True)
        raise
    finally:
        if conn is not None:
            conn.close()


def _indexed_records(review_dir: Path) -> dict[str, dict[str, object]]:
    indexed: dict[str, dict[str, object]] = {}
    for path in sorted((review_dir / "records").glob("*.json")):
        record = json.loads(path.read_text(encoding="utf-8"))
        var_id = str(record.get("id", ""))
        if not var_id or var_id in indexed:
            raise ClosureError(f"duplicate or missing INDEXED record id: {var_id!r}")
        indexed[var_id] = record
    expected = {candidate.var_id for candidate in CANDIDATES}
    if set(indexed) != expected:
        raise ClosureError("INDEXED record set does not exactly match closure plan")
    return indexed


def _reviewed_records(review_dir: Path) -> list[dict[str, object]]:
    worksheet = json.loads((review_dir / "review-worksheet.json").read_text(encoding="utf-8"))
    if (
        worksheet.get("kind") != WORKSHEET_KIND
        or worksheet.get("contains_official_source_text") is not False
        or worksheet.get("candidate_count") != len(CANDIDATES)
    ):
        raise ClosureError("worksheet identity/cardinality mismatch")
    entries = worksheet.get("candidates")
    if not isinstance(entries, list) or len(entries) != len(CANDIDATES):
        raise ClosureError("worksheet candidates mismatch")
    indexed = _indexed_records(review_dir)
    reviewed_records: list[dict[str, object]] = []
    for candidate, entry in zip(CANDIDATES, entries):
        if not isinstance(entry, dict) or entry.get("var_id") != candidate.var_id:
            raise ClosureError(f"worksheet order/id mismatch at {candidate.var_id}")
        decision = entry.get("decision")
        if not isinstance(decision, dict):
            raise ClosureError(f"{candidate.var_id}: decision object missing")
        if decision.get("source_inspected") is not True or decision.get("accepted") is not True:
            raise ClosureError(f"{candidate.var_id}: source must be explicitly inspected and accepted")
        reviewer, note = decision.get("reviewer"), decision.get("note")
        if (
            not isinstance(reviewer, str)
            or not reviewer.strip()
            or not isinstance(note, str)
            or not note.strip()
        ):
            raise ClosureError(f"{candidate.var_id}: reviewer and note are required")
        observed_raw = entry.get("atlas_observed_hazards")
        reviewed = decision.get("hazards_reviewed")
        if not isinstance(observed_raw, list) or any(
            not isinstance(item, str) or not item for item in observed_raw
        ):
            raise ClosureError(f"{candidate.var_id}: observed hazards must be a string array")
        if not isinstance(reviewed, list) or any(
            not isinstance(item, str) or not item for item in reviewed
        ):
            raise ClosureError(f"{candidate.var_id}: hazards_reviewed must be a string array")
        missing = set(observed_raw) - set(reviewed)
        if missing:
            raise ClosureError(f"{candidate.var_id}: undispositioned hazards: {sorted(missing)}")
        semantic_rules = decision.get("semantic_rules")
        allowed = set(candidate.semantic_rules)
        if (
            not isinstance(semantic_rules, list)
            or not semantic_rules
            or any(not isinstance(rule, str) or rule not in allowed for rule in semantic_rules)
        ):
            raise ClosureError(
                f"{candidate.var_id}: semantic_rules must be a non-empty subset of "
                f"{sorted(allowed)}"
            )
        record = indexed[candidate.var_id]
        if record.get("status") != "INDEXED":
            raise ClosureError(f"{candidate.var_id}: bound source record must remain INDEXED")
        if entry.get("source") != record.get("source"):
            raise ClosureError(f"{candidate.var_id}: worksheet/record source identity drift")
        reviewed_record = dict(record)
        reviewed_record["status"] = "VAR_REVIEWED"
        reviewed_record["hazards_reviewed"] = sorted(set(reviewed))
        reviewed_record["semantic_rules"] = sorted(set(semantic_rules))
        reviewed_record["evidence"] = ["R1B supplemental source-closure review"]
        reviewed_record["notes"] = [f"Reviewer: {reviewer.strip()}", note.strip()]
        reviewed_records.append(reviewed_record)
    return reviewed_records


def finalize(review_dir: Path, output_dir: Path) -> None:
    if output_dir.exists() or output_dir.is_symlink():
        raise ClosureError(f"finalized output must not already exist: {output_dir}")

    # Validate the entire review before creating any final output. A rejected worksheet must
    # leave no partial reviewed record set that could be mistaken for a successful finalization.
    reviewed_records = _reviewed_records(review_dir)
    gate_source = review_dir / "gate" / f"{GATE_ID}.json"
    gate = json.loads(gate_source.read_text(encoding="utf-8"))
    if (
        gate.get("id") != GATE_ID
        or gate.get("minimum_status") != "VAR_REVIEWED"
        or gate.get("require_semantic_rules") is not True
        or gate.get("require_hazards_reviewed") is not True
    ):
        raise ClosureError("closure gate no longer enforces the review boundary")

    staging = output_dir.with_name(f".{output_dir.name}.staging")
    if staging.exists() or staging.is_symlink():
        raise ClosureError(f"staging output already exists: {staging}")
    staging.mkdir(parents=True)
    try:
        (staging / "records").mkdir()
        (staging / "gate").mkdir()
        for record in reviewed_records:
            (staging / "records" / f"{record['id']}.json").write_bytes(pretty_bytes(record))
        shutil.copyfile(gate_source, staging / "gate" / f"{GATE_ID}.json")
        staging.replace(output_dir)
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise


def _shell_command(parts: Sequence[object]) -> str:
    return " ".join(shlex.quote(str(part)) for part in parts)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-configuration-source-closure")
    sub = parser.add_subparsers(dest="command", required=True)
    prepare_parser = sub.add_parser("prepare")
    prepare_parser.add_argument("--output-dir", type=Path, required=True)
    prepare_parser.add_argument("--db", type=Path, default=source_probe.DEFAULT_DB)
    prepare_parser.add_argument("--source", type=Path, default=source_probe.DEFAULT_SOURCE)
    prepare_parser.add_argument("--lock", type=Path, default=source_probe.DEFAULT_LOCK)
    finalize_parser = sub.add_parser("finalize")
    finalize_parser.add_argument("--review-dir", type=Path, required=True)
    finalize_parser.add_argument("--output-dir", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "prepare":
            manifest = prepare(args.output_dir, args.db, args.source, args.lock)
            print(f"closure_review={args.output_dir}")
            print(f"candidates={manifest['candidate_count']}")
            print("contains_official_source_text=true")
            print("commit_policy=EPHEMERAL_DO_NOT_COMMIT")
            print(f"worksheet={args.output_dir / 'review-worksheet.json'}")
        else:
            finalize(args.review_dir, args.output_dir)
            gate = args.output_dir / "gate" / f"{GATE_ID}.json"
            records = args.output_dir / "records"
            admission = args.output_dir / f"{GATE_ID}-source-admission.json"
            print(f"reviewed_closure={args.output_dir}")
            print(
                "next_source_gate_command="
                + _shell_command(
                    [
                        "python3",
                        "tools/vanilla_source_gate.py",
                        "--db",
                        source_probe.DEFAULT_DB,
                        "--gate",
                        gate,
                        "--records",
                        records,
                        "--output",
                        admission,
                    ]
                )
            )
        return 0
    except (
        OSError,
        json.JSONDecodeError,
        zipfile.BadZipFile,
        ClosureError,
        source_probe.ProbeError,
    ) as error:
        print(f"R1B Configuration source-closure error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
