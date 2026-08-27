#!/usr/bin/env python3
"""Prepare the final implementation-discovered R2B runtime-seam source review.

The independently admitted 206-method Play-entry gate remains immutable. During compact codec
implementation a handful of wrapper-level delegates were found whose terminal outbound bodies were
not members of that gate. This tool creates one bounded supplement review for those terminals only.

Source-rich output is ephemeral and must remain outside the repository.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sqlite3
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

try:
    from . import r1b_configuration_source_probe as source_probe
    from . import vanilla_atlas
except ImportError:
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]
    import vanilla_atlas  # type: ignore[no-redef]

SCHEMA = 1
REVIEW_ID = "REVIEW-NET-R2B-PLAY-RUNTIME-SEAMS-26_2-001"
PREPARED_KIND = "r2b-play-entry-runtime-seams-source-review"
WORKSHEET_KIND = "r2b-play-entry-runtime-seams-source-review-worksheet"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
BASE_GATE_ID = "GATE-NET-PLAY-ENTRY-26_2-001"
BASE_GATE_SHA256 = "a304f8f35a411b2d14300c5cf1bbe8097afe9eadd1059ae180e4129ef8d781cb"
BASE_REQUIRED_METHODS = 206
EXPECTED_SOURCE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PLAN = REPO_ROOT / "vanilla/reviews/network/r2b-play-entry-runtime-seams-plan.json"
DEFAULT_DB = Path(".crucible/vanilla/atlas.sqlite")
DEFAULT_SOURCE = Path.home() / "Documents/mc-source/mc-src.zip"
DEFAULT_LOCK = REPO_ROOT / "vanilla/vanilla.lock.toml"


class RuntimeSeamsError(RuntimeError):
    """Fail-closed runtime-seam review error."""


@dataclass(frozen=True, slots=True)
class Selector:
    mode: str
    required: bool
    type_name: str
    names: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class Group:
    group_id: str
    review_focus: str
    selectors: tuple[Selector, ...]


def pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _external_fresh_dir(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise RuntimeSeamsError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repo = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repo)
    except ValueError:
        return resolved
    raise RuntimeSeamsError("runtime-seams review contains official source text and must live outside the repository")


def _load_plan(path: Path) -> tuple[str, tuple[str, ...], tuple[Group, ...]]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeSeamsError(f"cannot read runtime-seams plan: {error}") from error
    expected = {"schema", "id", "base_gate", "source_archive_sha256", "scope", "reused_source_records", "groups"}
    if not isinstance(value, dict) or set(value) != expected:
        raise RuntimeSeamsError("runtime-seams plan has unexpected fields")
    if value["schema"] != SCHEMA or value["id"] != REVIEW_ID:
        raise RuntimeSeamsError("runtime-seams plan identity mismatch")
    if value["source_archive_sha256"] != EXPECTED_SOURCE_SHA256:
        raise RuntimeSeamsError("runtime-seams source commitment mismatch")
    base = value["base_gate"]
    if base != {"id": BASE_GATE_ID, "sha256": BASE_GATE_SHA256, "required_methods": BASE_REQUIRED_METHODS}:
        raise RuntimeSeamsError("runtime-seams base gate commitment mismatch")
    scope = value["scope"]
    reused = value["reused_source_records"]
    raw_groups = value["groups"]
    if not isinstance(scope, str) or not scope:
        raise RuntimeSeamsError("runtime-seams scope must be non-empty")
    if not isinstance(reused, list) or not reused or any(not isinstance(item, str) or not item for item in reused):
        raise RuntimeSeamsError("reused_source_records must be a non-empty string array")
    if len(reused) != len(set(reused)):
        raise RuntimeSeamsError("reused_source_records contains duplicates")
    if not isinstance(raw_groups, list) or not raw_groups:
        raise RuntimeSeamsError("runtime-seams groups must be non-empty")

    groups: list[Group] = []
    seen_groups: set[str] = set()
    for group_index, raw_group in enumerate(raw_groups):
        if not isinstance(raw_group, dict) or set(raw_group) != {"group_id", "review_focus", "selectors"}:
            raise RuntimeSeamsError(f"group {group_index} has unexpected fields")
        group_id = raw_group["group_id"]
        focus = raw_group["review_focus"]
        raw_selectors = raw_group["selectors"]
        if not isinstance(group_id, str) or not group_id or group_id in seen_groups:
            raise RuntimeSeamsError(f"group {group_index} invalid/duplicate group_id")
        if not isinstance(focus, str) or not focus:
            raise RuntimeSeamsError(f"{group_id} review_focus must be non-empty")
        if not isinstance(raw_selectors, list) or not raw_selectors:
            raise RuntimeSeamsError(f"{group_id} selectors must be non-empty")
        selectors: list[Selector] = []
        for selector_index, raw in enumerate(raw_selectors):
            if not isinstance(raw, dict) or set(raw) != {"mode", "required", "type_name", "names"}:
                raise RuntimeSeamsError(f"{group_id} selector {selector_index} has unexpected fields")
            if raw["mode"] != "type_names" or raw["required"] is not True:
                raise RuntimeSeamsError(f"{group_id} selector {selector_index} must be required type_names")
            type_name = raw["type_name"]
            names = raw["names"]
            if not isinstance(type_name, str) or not type_name:
                raise RuntimeSeamsError(f"{group_id} selector {selector_index} invalid type_name")
            if not isinstance(names, list) or not names or any(not isinstance(name, str) or not name for name in names):
                raise RuntimeSeamsError(f"{group_id} selector {selector_index} invalid names")
            if len(names) != len(set(names)):
                raise RuntimeSeamsError(f"{group_id} selector {selector_index} duplicate names")
            selectors.append(Selector("type_names", True, type_name, tuple(names)))
        seen_groups.add(group_id)
        groups.append(Group(group_id, focus, tuple(selectors)))
    return scope, tuple(reused), tuple(groups)


def _validate_base_report(path: Path) -> tuple[set[str], dict[str, object]]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeSeamsError(f"cannot read base source-gate report: {error}") from error
    if not isinstance(value, dict):
        raise RuntimeSeamsError("base source-gate report must be an object")
    if value.get("gate_id") != BASE_GATE_ID or value.get("gate_sha256") != BASE_GATE_SHA256:
        raise RuntimeSeamsError("base source-gate identity/hash mismatch")
    if value.get("admitted") is not True or value.get("failures") != []:
        raise RuntimeSeamsError("base source gate is not cleanly admitted")
    required = value.get("required_methods")
    if not isinstance(required, list) or len(required) != BASE_REQUIRED_METHODS:
        raise RuntimeSeamsError("base source-gate required-method count mismatch")
    source = value.get("source")
    if not isinstance(source, dict) or source.get("archive_sha256") != EXPECTED_SOURCE_SHA256:
        raise RuntimeSeamsError("base source archive commitment mismatch")
    identities: set[str] = set()
    for index, method in enumerate(required):
        if not isinstance(method, dict):
            raise RuntimeSeamsError(f"base required method {index} must be an object")
        identity = method.get("source")
        if not isinstance(identity, str) or not identity:
            raise RuntimeSeamsError(f"base required method {index} missing source identity")
        identities.add(identity)
    if len(identities) != BASE_REQUIRED_METHODS:
        raise RuntimeSeamsError("base source gate contains duplicate source identities")
    return identities, {
        "id": BASE_GATE_ID,
        "sha256": BASE_GATE_SHA256,
        "required_methods": BASE_REQUIRED_METHODS,
    }


def _validate_reused_records(paths: Sequence[str]) -> list[dict[str, object]]:
    result: list[dict[str, object]] = []
    for relative in paths:
        path = REPO_ROOT / relative
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise RuntimeSeamsError(f"cannot read reused source record {relative}: {error}") from error
        if not isinstance(value, dict) or value.get("status") != "VAR_REVIEWED":
            raise RuntimeSeamsError(f"reused source record is not VAR_REVIEWED: {relative}")
        source = value.get("source")
        if not isinstance(source, dict) or source.get("fingerprint_algorithm") != "java-token-v2-literal-sensitive":
            raise RuntimeSeamsError(f"reused source record has invalid fingerprint metadata: {relative}")
        result.append({
            "path": relative,
            "id": value.get("id"),
            "source_identity": f"{source.get('type')}#{source.get('signature')}",
            "normalized_sha256": source.get("normalized_sha256"),
        })
    return result


_ROW_SELECT = """SELECT m.id,t.qualified_name,m.name,m.signature,m.param_count,
                        m.start_line,m.end_line,f.path
                 FROM methods m
                 JOIN types t ON t.id=m.type_id
                 JOIN source_files f ON f.id=t.file_id"""


def _resolve_groups(
    conn: sqlite3.Connection,
    groups: Sequence[Group],
    base_identities: set[str],
) -> list[tuple[sqlite3.Row, tuple[str, ...], tuple[str, ...]]]:
    by_id: dict[int, tuple[sqlite3.Row, set[str], set[str]]] = {}
    failures: list[str] = []
    for group in groups:
        selected_for_group = 0
        for selector in group.selectors:
            placeholders = ",".join("?" for _ in selector.names)
            rows = conn.execute(
                _ROW_SELECT
                + f" WHERE t.qualified_name=? AND m.name IN ({placeholders}) ORDER BY m.start_line,m.id",
                (selector.type_name, *selector.names),
            ).fetchall()
            found = {str(row["name"]) for row in rows}
            missing = set(selector.names) - found
            if missing:
                failures.append(f"{group.group_id}: required names missing from {selector.type_name}: {sorted(missing)}")
                continue
            for row in rows:
                identity = f"{row['qualified_name']}#{row['signature']}"
                if identity in base_identities:
                    failures.append(f"{group.group_id}: selector redundantly resolves base-gate identity {identity}")
                    continue
                method_id = int(row["id"])
                if method_id not in by_id:
                    by_id[method_id] = (row, set(), set())
                by_id[method_id][1].add(group.group_id)
                by_id[method_id][2].add(group.review_focus)
                selected_for_group += 1
        if selected_for_group == 0:
            failures.append(f"{group.group_id}: no new source identities selected")
    if failures:
        raise RuntimeSeamsError("runtime-seams selector preflight failed:\n  - " + "\n  - ".join(failures))
    ordered = sorted(
        by_id.values(),
        key=lambda item: (str(item[0]["qualified_name"]), int(item[0]["start_line"]), str(item[0]["signature"])),
    )
    return [(row, tuple(sorted(groups)), tuple(sorted(focus))) for row, groups, focus in ordered]


def _source_excerpt(archive: zipfile.ZipFile, row: sqlite3.Row) -> str:
    path = str(row["path"])
    try:
        text = archive.read(path).decode("utf-8", errors="strict")
    except KeyError as error:
        raise RuntimeSeamsError(f"source member missing: {path}") from error
    lines = text.splitlines()
    start, end = int(row["start_line"]), int(row["end_line"])
    if not (1 <= start <= end <= len(lines)):
        raise RuntimeSeamsError(f"invalid Atlas line range for {path}: {start}-{end}")
    return "\n".join(lines[start - 1 : end]) + "\n"


def prepare(
    output_dir: Path,
    base_gate_report: Path,
    plan: Path,
    db: Path,
    source: Path,
    lock: Path,
) -> dict[str, object]:
    output = _external_fresh_dir(output_dir)
    scope, reused_paths, groups = _load_plan(plan)
    base_identities, base_gate = _validate_base_report(base_gate_report)
    reused = _validate_reused_records(reused_paths)
    output.mkdir(parents=True)
    conn: sqlite3.Connection | None = None
    try:
        conn = vanilla_atlas.connect_db(db)
        source_sha = source_probe.require_pinned_source(conn, source, lock)
        if source_sha != EXPECTED_SOURCE_SHA256:
            raise RuntimeSeamsError(f"source pin mismatch: {source_sha}")
        resolved = _resolve_groups(conn, groups, base_identities)
        dossier_candidates: list[dict[str, object]] = []
        worksheet_candidates: list[dict[str, object]] = []
        group_counts: dict[str, int] = {group.group_id: 0 for group in groups}
        with zipfile.ZipFile(source) as archive:
            for index, (row, group_ids, focus) in enumerate(resolved, start=1):
                candidate_id = f"DISC-NET-R2B-PLAY-RUNTIME-SEAM-{index:03d}"
                record = source_probe.record_template(conn, row, candidate_id)
                source_record = dict(record["source"])
                hazards = sorted(set(record.get("atlas_observed_hazards", [])))
                identity = f"{source_record['type']}#{source_record['signature']}"
                excerpt = _source_excerpt(archive, row)
                for group_id in group_ids:
                    group_counts[group_id] += 1
                common = {
                    "candidate_id": candidate_id,
                    "group_ids": list(group_ids),
                    "source_identity": identity,
                    "source": source_record,
                    "atlas_observed_hazards": hazards,
                    "review_focus": list(focus),
                }
                dossier_candidates.append({
                    **common,
                    "path": str(row["path"]),
                    "start_line": int(row["start_line"]),
                    "end_line": int(row["end_line"]),
                    "source_excerpt": excerpt,
                    "source_excerpt_sha256": sha256_bytes(excerpt.encode("utf-8")),
                })
                worksheet_candidates.append({
                    **common,
                    "decision": {
                        "source_inspected": False,
                        "accepted": False,
                        "hazards_reviewed": [],
                        "followup_dependencies": [],
                        "semantic_observations": [],
                        "note": "",
                    },
                })
        prior = {"base_gate": base_gate, "reused_source_records": reused}
        dossier = {
            "schema": SCHEMA,
            "id": REVIEW_ID,
            "kind": PREPARED_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "source_archive_sha256": source_sha,
            "prior": prior,
            "scope": scope,
            "candidate_count": len(dossier_candidates),
            "group_counts": group_counts,
            "candidates": dossier_candidates,
        }
        worksheet = {
            "schema": SCHEMA,
            "id": REVIEW_ID,
            "kind": WORKSHEET_KIND,
            "contains_official_source_text": False,
            "source_archive_sha256": source_sha,
            "prior": prior,
            "scope": scope,
            "candidate_count": len(worksheet_candidates),
            "group_counts": group_counts,
            "candidates": worksheet_candidates,
        }
        (output / "review-dossier.json").write_bytes(pretty_bytes(dossier))
        (output / "review-worksheet.json").write_bytes(pretty_bytes(worksheet))
        manifest = {
            "schema": SCHEMA,
            "id": REVIEW_ID,
            "kind": PREPARED_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "source_archive_sha256": source_sha,
            "prior": prior,
            "scope": scope,
            "candidate_count": len(dossier_candidates),
            "group_counts": group_counts,
            "artifacts": {
                "review_dossier": "review-dossier.json",
                "review_worksheet": "review-worksheet.json",
            },
            "next_required_step": (
                "Inspect every exact runtime-seam body. Any escaping outbound delegate must be added to this same bounded supplement before a supplement source gate may be emitted."
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
    parser = argparse.ArgumentParser(prog="r2b-play-entry-runtime-seams-source-review")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--base-gate-report", type=Path, required=True)
    parser.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        manifest = prepare(args.output_dir, args.base_gate_report, args.plan, args.db, args.source, args.lock)
    except RuntimeSeamsError as error:
        print(f"R2B runtime-seams review error: {error}", file=sys.stderr)
        return 2
    print(f"r2b_play_entry_runtime_seams_review={args.output_dir}")
    print(f"candidates={manifest['candidate_count']}")
    print(f"groups={json.dumps(manifest['group_counts'], sort_keys=True)}")
    print("contains_official_source_text=true")
    print(f"commit_policy={COMMIT_POLICY}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
