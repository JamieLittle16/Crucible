#!/usr/bin/env python3
"""Prepare the final bounded R2B Play-entry wire-only source review.

The 67-body Play-entry review closes selected-route control flow. This tool deliberately does not
reopen gameplay discovery. It validates that exact prior source-rich review, resolves only the
serializer/codec families named by the committed wire-closure plan, excludes source identities
already present in the 67-body dossier, preflights every rule, and only then emits source excerpts.

All source-rich output is ephemeral and must remain outside the repository.
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
from typing import Any, Sequence

try:
    from . import r1b_configuration_source_probe as source_probe
    from . import vanilla_atlas
except ImportError:
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]
    import vanilla_atlas  # type: ignore[no-redef]

SCHEMA = 1
REVIEW_ID = "REVIEW-NET-R2B-PLAY-WIRE-CLOSURE-26_2-001"
PRIOR_REVIEW_ID = "REVIEW-NET-R2B-PLAY-ENTRY-FINAL-26_2-001"
PREPARED_KIND = "r2b-play-entry-wire-closure-source-review"
WORKSHEET_KIND = "r2b-play-entry-wire-closure-source-review-worksheet"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
EXPECTED_SOURCE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
EXPECTED_PRIOR_COUNT = 67
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PLAN = REPO_ROOT / "vanilla/reviews/network/r2b-play-entry-wire-closure-plan.json"
DEFAULT_DB = Path(".crucible/vanilla/atlas.sqlite")
DEFAULT_SOURCE = Path.home() / "Documents/mc-source/mc-src.zip"
DEFAULT_LOCK = REPO_ROOT / "vanilla/vanilla.lock.toml"


class WireClosureError(RuntimeError):
    """Fail-closed wire-closure review error."""


@dataclass(frozen=True, slots=True)
class Selector:
    mode: str
    required: bool
    type_name: str | None = None
    type_prefix: str | None = None
    names: tuple[str, ...] = ()
    signature: str | None = None
    name_regex: str | None = None


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
        raise WireClosureError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repo = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repo)
    except ValueError:
        return resolved
    raise WireClosureError("wire-closure review contains official source text and must live outside the repository")


def _selector(value: object, group_id: str, index: int) -> Selector:
    if not isinstance(value, dict):
        raise WireClosureError(f"{group_id} selector {index} must be an object")
    mode = value.get("mode")
    required = value.get("required")
    if not isinstance(mode, str) or type(required) is not bool:
        raise WireClosureError(f"{group_id} selector {index} requires string mode and bool required")
    type_name = value.get("type_name")
    type_prefix = value.get("type_prefix")
    names = value.get("names", [])
    signature = value.get("signature")
    name_regex = value.get("name_regex")
    if type_name is not None and (not isinstance(type_name, str) or not type_name):
        raise WireClosureError(f"{group_id} selector {index} invalid type_name")
    if type_prefix is not None and (not isinstance(type_prefix, str) or not type_prefix):
        raise WireClosureError(f"{group_id} selector {index} invalid type_prefix")
    if not isinstance(names, list) or any(not isinstance(item, str) or not item for item in names):
        raise WireClosureError(f"{group_id} selector {index} names must be strings")
    if signature is not None and (not isinstance(signature, str) or not signature):
        raise WireClosureError(f"{group_id} selector {index} invalid signature")
    if name_regex is not None:
        if not isinstance(name_regex, str) or not name_regex:
            raise WireClosureError(f"{group_id} selector {index} invalid name_regex")
        try:
            re.compile(name_regex)
        except re.error as error:
            raise WireClosureError(f"{group_id} selector {index} invalid regex: {error}") from error

    allowed = {
        "exact_signature": {"type_name", "signature"},
        "type_all": {"type_name"},
        "type_names": {"type_name", "names"},
        "prefix_all": {"type_prefix"},
        "type_name_regex": {"type_name", "name_regex"},
        "prefix_name_regex": {"type_prefix", "name_regex"},
    }
    if mode not in allowed:
        raise WireClosureError(f"{group_id} selector {index} unknown mode {mode!r}")
    present = {
        key
        for key, item in {
            "type_name": type_name,
            "type_prefix": type_prefix,
            "names": names if names else None,
            "signature": signature,
            "name_regex": name_regex,
        }.items()
        if item is not None
    }
    if present != allowed[mode]:
        raise WireClosureError(
            f"{group_id} selector {index} mode {mode} requires {sorted(allowed[mode])}, got {sorted(present)}"
        )
    return Selector(
        mode=mode,
        required=required,
        type_name=type_name,
        type_prefix=type_prefix,
        names=tuple(names),
        signature=signature,
        name_regex=name_regex,
    )


def load_plan(path: Path = DEFAULT_PLAN) -> tuple[Group, ...]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise WireClosureError(f"cannot read wire-closure plan: {error}") from error
    if not isinstance(value, dict):
        raise WireClosureError("wire-closure plan must be an object")
    expected_keys = {"schema", "id", "prior_review_id", "source_archive_sha256", "groups"}
    if set(value) != expected_keys:
        raise WireClosureError("wire-closure plan has unexpected fields")
    if value["schema"] != SCHEMA or value["id"] != REVIEW_ID:
        raise WireClosureError("wire-closure plan identity mismatch")
    if value["prior_review_id"] != PRIOR_REVIEW_ID:
        raise WireClosureError("wire-closure plan prior review mismatch")
    if value["source_archive_sha256"] != EXPECTED_SOURCE_SHA256:
        raise WireClosureError("wire-closure plan source pin mismatch")
    raw_groups = value["groups"]
    if not isinstance(raw_groups, list) or not raw_groups:
        raise WireClosureError("wire-closure plan groups must be non-empty")
    groups: list[Group] = []
    seen: set[str] = set()
    for group_index, raw_group in enumerate(raw_groups):
        if not isinstance(raw_group, dict) or set(raw_group) != {"group_id", "review_focus", "selectors"}:
            raise WireClosureError(f"group {group_index} has unexpected fields")
        group_id = raw_group["group_id"]
        focus = raw_group["review_focus"]
        selectors = raw_group["selectors"]
        if not isinstance(group_id, str) or not group_id or group_id in seen:
            raise WireClosureError(f"group {group_index} invalid/duplicate group_id")
        if not isinstance(focus, str) or not focus:
            raise WireClosureError(f"{group_id} review_focus must be non-empty")
        if not isinstance(selectors, list) or not selectors:
            raise WireClosureError(f"{group_id} selectors must be non-empty")
        seen.add(group_id)
        groups.append(
            Group(
                group_id=group_id,
                review_focus=focus,
                selectors=tuple(_selector(item, group_id, i) for i, item in enumerate(selectors)),
            )
        )
    return tuple(groups)


GROUPS = load_plan()


def validate_prior_review(path: Path) -> tuple[set[str], str]:
    try:
        raw = path.read_bytes()
        value = json.loads(raw)
    except (OSError, json.JSONDecodeError) as error:
        raise WireClosureError(f"cannot read prior 67-body dossier: {error}") from error
    if not isinstance(value, dict):
        raise WireClosureError("prior dossier must be an object")
    expected = {
        "id": PRIOR_REVIEW_ID,
        "kind": "r2b-play-entry-final-source-review",
        "contains_official_source_text": True,
        "candidate_count": EXPECTED_PRIOR_COUNT,
        "source_archive_sha256": EXPECTED_SOURCE_SHA256,
    }
    mismatches = {
        key: {"expected": expected_value, "actual": value.get(key)}
        for key, expected_value in expected.items()
        if value.get(key) != expected_value
    }
    if mismatches:
        raise WireClosureError(f"prior dossier identity mismatch: {json.dumps(mismatches, sort_keys=True)}")
    candidates = value.get("candidates")
    if not isinstance(candidates, list) or len(candidates) != EXPECTED_PRIOR_COUNT:
        raise WireClosureError("prior dossier candidate array mismatch")
    identities: list[str] = []
    for index, candidate in enumerate(candidates):
        if not isinstance(candidate, dict):
            raise WireClosureError(f"prior candidate {index} must be an object")
        identity = candidate.get("source_identity")
        source = candidate.get("source")
        excerpt = candidate.get("source_excerpt")
        if not isinstance(identity, str) or not identity:
            raise WireClosureError(f"prior candidate {index} source_identity missing")
        if not isinstance(source, dict) or source.get("fingerprint_algorithm") != "java-token-v2-literal-sensitive":
            raise WireClosureError(f"prior candidate {index} fingerprint metadata invalid")
        if not isinstance(excerpt, str):
            raise WireClosureError(f"prior candidate {index} source excerpt missing")
        identities.append(identity)
    if len(identities) != len(set(identities)):
        raise WireClosureError("prior dossier contains duplicate source identities")
    required_identity = "net.minecraft.server.level.ServerPlayer#<fieldinit:containerSynchronizer>()"
    if required_identity not in identities:
        raise WireClosureError("prior dossier is missing hardened inventory synchronizer evidence")
    return set(identities), sha256_bytes(raw)


_ROW_SELECT = """SELECT m.id,t.qualified_name,m.name,m.signature,m.param_count,
                        m.start_line,m.end_line,f.path
                 FROM methods m
                 JOIN types t ON t.id=m.type_id
                 JOIN source_files f ON f.id=t.file_id"""


def _query_selector(conn: sqlite3.Connection, selector: Selector) -> list[sqlite3.Row]:
    if selector.mode == "exact_signature":
        return conn.execute(
            _ROW_SELECT + " WHERE t.qualified_name=? AND m.signature=? ORDER BY m.start_line,m.id",
            (selector.type_name, selector.signature),
        ).fetchall()
    if selector.mode == "type_all":
        return conn.execute(
            _ROW_SELECT + " WHERE t.qualified_name=? ORDER BY m.start_line,m.id",
            (selector.type_name,),
        ).fetchall()
    if selector.mode == "type_names":
        placeholders = ",".join("?" for _ in selector.names)
        rows = conn.execute(
            _ROW_SELECT
            + f" WHERE t.qualified_name=? AND m.name IN ({placeholders}) ORDER BY m.start_line,m.id",
            (selector.type_name, *selector.names),
        ).fetchall()
        if selector.required:
            found = {str(row["name"]) for row in rows}
            missing = set(selector.names) - found
            if missing:
                raise WireClosureError(
                    f"required names missing from {selector.type_name}: {sorted(missing)}"
                )
        return rows
    if selector.mode == "prefix_all":
        return conn.execute(
            _ROW_SELECT + " WHERE t.qualified_name LIKE ? ORDER BY t.qualified_name,m.start_line,m.id",
            (f"{selector.type_prefix}%",),
        ).fetchall()
    if selector.mode in {"type_name_regex", "prefix_name_regex"}:
        if selector.mode == "type_name_regex":
            rows = conn.execute(
                _ROW_SELECT + " WHERE t.qualified_name=? ORDER BY m.start_line,m.id",
                (selector.type_name,),
            ).fetchall()
        else:
            rows = conn.execute(
                _ROW_SELECT + " WHERE t.qualified_name LIKE ? ORDER BY t.qualified_name,m.start_line,m.id",
                (f"{selector.type_prefix}%",),
            ).fetchall()
        pattern = re.compile(selector.name_regex or "")
        return [row for row in rows if pattern.search(str(row["name"]))]
    raise AssertionError(selector.mode)


def resolve_groups(
    conn: sqlite3.Connection,
    groups: Sequence[Group],
    prior_identities: set[str],
) -> list[tuple[sqlite3.Row, tuple[str, ...], tuple[str, ...]]]:
    """Resolve every plan selector, fail closed, deduplicate, and exclude already-reviewed identities."""
    by_id: dict[int, tuple[sqlite3.Row, set[str], set[str]]] = {}
    failures: list[str] = []
    for group in groups:
        group_rows = 0
        for selector in group.selectors:
            try:
                rows = _query_selector(conn, selector)
            except WireClosureError as error:
                failures.append(f"{group.group_id}: {error}")
                continue
            if selector.required and not rows:
                descriptor = selector.type_name or selector.type_prefix
                failures.append(f"{group.group_id}: required selector {selector.mode} {descriptor} resolved 0 methods")
                continue
            group_rows += len(rows)
            for row in rows:
                identity = f"{row['qualified_name']}#{row['signature']}"
                if identity in prior_identities:
                    continue
                method_id = int(row["id"])
                if method_id not in by_id:
                    by_id[method_id] = (row, set(), set())
                by_id[method_id][1].add(group.group_id)
                by_id[method_id][2].add(group.review_focus)
        newly = sum(1 for _row, group_ids, _focus in by_id.values() if group.group_id in group_ids)
        if group_rows and newly == 0:
            failures.append(f"{group.group_id}: all resolved rows were already present in the 67-body dossier")
    if failures:
        raise WireClosureError("wire-closure selector preflight failed:\n  - " + "\n  - ".join(failures))
    ordered = sorted(
        by_id.values(),
        key=lambda item: (str(item[0]["qualified_name"]), int(item[0]["start_line"]), str(item[0]["signature"])),
    )
    return [
        (row, tuple(sorted(group_ids)), tuple(sorted(focus)))
        for row, group_ids, focus in ordered
    ]


def _source_excerpt(archive: zipfile.ZipFile, row: sqlite3.Row) -> str:
    path = str(row["path"])
    try:
        text = archive.read(path).decode("utf-8", errors="strict")
    except KeyError as error:
        raise WireClosureError(f"source member missing: {path}") from error
    lines = text.splitlines()
    start, end = int(row["start_line"]), int(row["end_line"])
    if not (1 <= start <= end <= len(lines)):
        raise WireClosureError(f"invalid Atlas line range for {path}: {start}-{end}")
    return "\n".join(lines[start - 1 : end]) + "\n"


def prepare(
    output_dir: Path,
    db: Path,
    source: Path,
    lock: Path,
    prior_dossier: Path,
    plan: Path = DEFAULT_PLAN,
) -> dict[str, object]:
    output = _external_fresh_dir(output_dir)
    prior_identities, prior_sha = validate_prior_review(prior_dossier)
    groups = load_plan(plan)

    conn: sqlite3.Connection | None = None
    output.mkdir(parents=True)
    try:
        conn = vanilla_atlas.connect_db(db)
        source_sha = source_probe.require_pinned_source(conn, source, lock)
        if source_sha != EXPECTED_SOURCE_SHA256:
            raise WireClosureError("pinned source helper returned unexpected source commitment")
        rows = resolve_groups(conn, groups, prior_identities)

        dossier_candidates: list[dict[str, object]] = []
        worksheet_candidates: list[dict[str, object]] = []
        with zipfile.ZipFile(source) as archive:
            for index, (row, group_ids, focus) in enumerate(rows, start=1):
                candidate_id = f"DISC-NET-R2B-PLAY-WIRE-CLOSURE-{index:03d}"
                template = source_probe.record_template(conn, row, candidate_id)
                source_record = dict(template["source"])
                identity = f"{source_record['type']}#{source_record['signature']}"
                excerpt = _source_excerpt(archive, row)
                hazards = sorted(set(template.get("atlas_observed_hazards", [])))
                common = {
                    "candidate_id": candidate_id,
                    "group_ids": list(group_ids),
                    "source_identity": identity,
                    "source": source_record,
                    "atlas_observed_hazards": hazards,
                    "review_focus": list(focus),
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
                            "hazards_reviewed": [],
                            "followup_dependencies": [],
                            "semantic_observations": [],
                            "note": "",
                        },
                    }
                )

        base = {
            "schema": SCHEMA,
            "id": REVIEW_ID,
            "source_archive_sha256": source_sha,
            "prior_review": {
                "id": PRIOR_REVIEW_ID,
                "candidate_count": EXPECTED_PRIOR_COUNT,
                "dossier_sha256": prior_sha,
            },
            "plan_sha256": sha256_bytes(plan.read_bytes()),
            "candidate_count": len(dossier_candidates),
            "group_counts": {
                group.group_id: sum(
                    1 for candidate in dossier_candidates if group.group_id in candidate["group_ids"]
                )
                for group in groups
            },
        }
        dossier = {
            **base,
            "kind": PREPARED_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "candidates": dossier_candidates,
        }
        worksheet = {
            **base,
            "kind": WORKSHEET_KIND,
            "contains_official_source_text": False,
            "candidates": worksheet_candidates,
        }
        manifest = {
            **base,
            "kind": PREPARED_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "artifacts": {
                "review_dossier": "review-dossier.json",
                "review_worksheet": "review-worksheet.json",
            },
            "scope": (
                "Wire/serializer closure only. The 67-body selected-route control-flow frontier is "
                "closed; world/chunk/light/movement/general gameplay are forbidden from this review."
            ),
            "next_required_step": (
                "Inspect every exact serializer body. If no genuinely material outbound delegate "
                "escapes these seven families, canonicalize directly into R2B VAR/SEM records and "
                "GATE-NET-PLAY-ENTRY-26_2-001."
            ),
        }
        (output / "review-dossier.json").write_bytes(pretty_bytes(dossier))
        (output / "review-worksheet.json").write_bytes(pretty_bytes(worksheet))
        (output / "manifest.json").write_bytes(pretty_bytes(manifest))
        return manifest
    except Exception:
        shutil.rmtree(output, ignore_errors=True)
        raise
    finally:
        if conn is not None:
            conn.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r2b-play-entry-wire-closure-source-review")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--prior-dossier", type=Path, required=True)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        manifest = prepare(
            args.output_dir,
            args.db,
            args.source,
            args.lock,
            args.prior_dossier,
            args.plan,
        )
    except WireClosureError as error:
        print(f"R2B Play-entry wire-closure review error: {error}", file=sys.stderr)
        return 2
    except Exception as error:
        print(f"R2B Play-entry wire-closure review failed closed: {error}", file=sys.stderr)
        return 2
    print(f"r2b_play_entry_wire_closure_review={args.output_dir}")
    print(f"candidates={manifest['candidate_count']}")
    print(f"groups={json.dumps(manifest['group_counts'], sort_keys=True)}")
    print(f"contains_official_source_text={str(manifest['contains_official_source_text']).lower()}")
    print(f"commit_policy={manifest['commit_policy']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
