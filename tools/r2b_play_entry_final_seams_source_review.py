#!/usr/bin/env python3
"""Prepare the last bounded source-rich review needed by the first R2B Play profile.

The preceding 117-body review deliberately over-selected seven serializer families. Its review result
classifies commands and synchronized recipes as composition-locked immutable publication artifacts,
closes recipe-book/default-empty ItemStack behavior, and leaves only reusable dynamic map/registry
holder and GlobalPos primitives.

This tool hard-pins the exact 117-body source-rich dossier that exposed those seams. It therefore
cannot be used as a generic source browser or quietly expand back into gameplay. Source-rich output
is ephemeral and must remain outside Git.
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
REVIEW_ID = "REVIEW-NET-R2B-PLAY-FINAL-SEAMS-26_2-001"
PRIOR_REVIEW_ID = "REVIEW-NET-R2B-PLAY-WIRE-CLOSURE-26_2-001"
PREPARED_KIND = "r2b-play-entry-final-seams-source-review"
WORKSHEET_KIND = "r2b-play-entry-final-seams-source-review-worksheet"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
EXPECTED_SOURCE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
EXPECTED_PRIOR_SHA256 = "93999fca0a4c69eda607e729af61c74e7ce40c96bf4201516904fabf79bc2e3a"
EXPECTED_PRIOR_COUNT = 117
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PLAN = REPO_ROOT / "vanilla/reviews/network/r2b-play-entry-final-seams-plan.json"
DEFAULT_DB = Path(".crucible/vanilla/atlas.sqlite")
DEFAULT_SOURCE = Path.home() / "Documents/mc-source/mc-src.zip"
DEFAULT_LOCK = REPO_ROOT / "vanilla/vanilla.lock.toml"


class FinalSeamsError(RuntimeError):
    """Fail-closed final-seam review error."""


@dataclass(frozen=True, slots=True)
class Selector:
    mode: str
    required: bool
    type_name: str
    names: tuple[str, ...] = ()
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
        raise FinalSeamsError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repo = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repo)
    except ValueError:
        return resolved
    raise FinalSeamsError("final-seams review contains official source text and must live outside the repository")


def _selector(value: object, group_id: str, index: int) -> Selector:
    if not isinstance(value, dict):
        raise FinalSeamsError(f"{group_id} selector {index} must be an object")
    mode = value.get("mode")
    required = value.get("required")
    type_name = value.get("type_name")
    if mode not in {"type_all", "type_names", "type_name_regex"}:
        raise FinalSeamsError(f"{group_id} selector {index} unsupported mode {mode!r}")
    if type(required) is not bool or not isinstance(type_name, str) or not type_name:
        raise FinalSeamsError(f"{group_id} selector {index} invalid required/type_name")
    names = value.get("names", [])
    name_regex = value.get("name_regex")
    if not isinstance(names, list) or any(not isinstance(item, str) or not item for item in names):
        raise FinalSeamsError(f"{group_id} selector {index} names must be strings")
    if mode == "type_names" and not names:
        raise FinalSeamsError(f"{group_id} selector {index} type_names requires names")
    if mode != "type_names" and names:
        raise FinalSeamsError(f"{group_id} selector {index} unexpected names")
    if mode == "type_name_regex":
        if not isinstance(name_regex, str) or not name_regex:
            raise FinalSeamsError(f"{group_id} selector {index} regex required")
        try:
            re.compile(name_regex)
        except re.error as error:
            raise FinalSeamsError(f"{group_id} selector {index} invalid regex: {error}") from error
    elif name_regex is not None:
        raise FinalSeamsError(f"{group_id} selector {index} unexpected regex")
    expected = {"mode", "required", "type_name"}
    if mode == "type_names":
        expected.add("names")
    if mode == "type_name_regex":
        expected.add("name_regex")
    if set(value) != expected:
        raise FinalSeamsError(f"{group_id} selector {index} unexpected fields")
    return Selector(mode, required, type_name, tuple(names), name_regex)


def load_plan(path: Path = DEFAULT_PLAN) -> tuple[str, tuple[Group, ...]]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise FinalSeamsError(f"cannot read final-seams plan: {error}") from error
    if not isinstance(value, dict):
        raise FinalSeamsError("final-seams plan must be an object")
    expected_keys = {
        "schema",
        "id",
        "prior_review_id",
        "prior_dossier_sha256",
        "source_archive_sha256",
        "scope",
        "groups",
    }
    if set(value) != expected_keys:
        raise FinalSeamsError("final-seams plan has unexpected fields")
    if value["schema"] != SCHEMA or value["id"] != REVIEW_ID:
        raise FinalSeamsError("final-seams plan identity mismatch")
    if value["prior_review_id"] != PRIOR_REVIEW_ID:
        raise FinalSeamsError("final-seams prior review mismatch")
    if value["prior_dossier_sha256"] != EXPECTED_PRIOR_SHA256:
        raise FinalSeamsError("final-seams prior dossier commitment mismatch")
    if value["source_archive_sha256"] != EXPECTED_SOURCE_SHA256:
        raise FinalSeamsError("final-seams source commitment mismatch")
    scope = value["scope"]
    if not isinstance(scope, str) or not scope:
        raise FinalSeamsError("final-seams scope must be non-empty")
    raw_groups = value["groups"]
    if not isinstance(raw_groups, list) or not raw_groups:
        raise FinalSeamsError("final-seams groups must be non-empty")
    groups: list[Group] = []
    seen: set[str] = set()
    for group_index, raw_group in enumerate(raw_groups):
        if not isinstance(raw_group, dict) or set(raw_group) != {"group_id", "review_focus", "selectors"}:
            raise FinalSeamsError(f"group {group_index} has unexpected fields")
        group_id = raw_group["group_id"]
        focus = raw_group["review_focus"]
        selectors = raw_group["selectors"]
        if not isinstance(group_id, str) or not group_id or group_id in seen:
            raise FinalSeamsError(f"group {group_index} invalid/duplicate group_id")
        if not isinstance(focus, str) or not focus:
            raise FinalSeamsError(f"{group_id} review_focus must be non-empty")
        if not isinstance(selectors, list) or not selectors:
            raise FinalSeamsError(f"{group_id} selectors must be non-empty")
        seen.add(group_id)
        groups.append(Group(group_id, focus, tuple(_selector(item, group_id, i) for i, item in enumerate(selectors))))
    return scope, tuple(groups)


SCOPE, GROUPS = load_plan()


def validate_prior_review(path: Path) -> tuple[set[str], str]:
    try:
        raw = path.read_bytes()
        value = json.loads(raw)
    except (OSError, json.JSONDecodeError) as error:
        raise FinalSeamsError(f"cannot read prior 117-body dossier: {error}") from error
    observed_sha = sha256_bytes(raw)
    if observed_sha != EXPECTED_PRIOR_SHA256:
        raise FinalSeamsError(
            f"prior 117-body dossier SHA mismatch: expected {EXPECTED_PRIOR_SHA256}, got {observed_sha}"
        )
    if not isinstance(value, dict):
        raise FinalSeamsError("prior dossier must be an object")
    expected = {
        "id": PRIOR_REVIEW_ID,
        "kind": "r2b-play-entry-wire-closure-source-review",
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
        raise FinalSeamsError(f"prior dossier identity mismatch: {json.dumps(mismatches, sort_keys=True)}")
    candidates = value.get("candidates")
    if not isinstance(candidates, list) or len(candidates) != EXPECTED_PRIOR_COUNT:
        raise FinalSeamsError("prior dossier candidate array mismatch")
    identities: list[str] = []
    for index, candidate in enumerate(candidates):
        if not isinstance(candidate, dict):
            raise FinalSeamsError(f"prior candidate {index} must be an object")
        identity = candidate.get("source_identity")
        source = candidate.get("source")
        excerpt = candidate.get("source_excerpt")
        if not isinstance(identity, str) or not identity:
            raise FinalSeamsError(f"prior candidate {index} source_identity missing")
        if not isinstance(source, dict) or source.get("fingerprint_algorithm") != "java-token-v2-literal-sensitive":
            raise FinalSeamsError(f"prior candidate {index} fingerprint metadata invalid")
        if not isinstance(excerpt, str):
            raise FinalSeamsError(f"prior candidate {index} source excerpt missing")
        identities.append(identity)
    if len(identities) != len(set(identities)):
        raise FinalSeamsError("prior dossier contains duplicate source identities")
    required = {
        "net.minecraft.network.protocol.game.ClientboundSetTimePacket#<clinit>()",
        "net.minecraft.world.clock.WorldClock#<clinit>()",
        "net.minecraft.world.level.dimension.DimensionType#<clinit>()",
        "net.minecraft.world.level.storage.LevelData$RespawnData#<clinit>()",
    }
    missing = required - set(identities)
    if missing:
        raise FinalSeamsError(f"prior dossier missing required dynamic seam roots: {sorted(missing)}")
    return set(identities), observed_sha


_ROW_SELECT = """SELECT m.id,t.qualified_name,m.name,m.signature,m.param_count,
                        m.start_line,m.end_line,f.path
                 FROM methods m
                 JOIN types t ON t.id=m.type_id
                 JOIN source_files f ON f.id=t.file_id"""


def _query_selector(conn: sqlite3.Connection, selector: Selector) -> list[sqlite3.Row]:
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
                raise FinalSeamsError(f"required names missing from {selector.type_name}: {sorted(missing)}")
        return rows
    if selector.mode == "type_name_regex":
        rows = conn.execute(
            _ROW_SELECT + " WHERE t.qualified_name=? ORDER BY m.start_line,m.id",
            (selector.type_name,),
        ).fetchall()
        pattern = re.compile(selector.name_regex or "")
        return [row for row in rows if pattern.search(str(row["name"]))]
    raise AssertionError(selector.mode)


def resolve_groups(
    conn: sqlite3.Connection,
    groups: Sequence[Group],
    prior_identities: set[str],
) -> list[tuple[sqlite3.Row, tuple[str, ...], tuple[str, ...]]]:
    by_id: dict[int, tuple[sqlite3.Row, set[str], set[str]]] = {}
    failures: list[str] = []
    for group in groups:
        newly_for_group = 0
        for selector in group.selectors:
            try:
                rows = _query_selector(conn, selector)
            except FinalSeamsError as error:
                failures.append(f"{group.group_id}: {error}")
                continue
            if selector.required and not rows:
                failures.append(
                    f"{group.group_id}: required selector {selector.mode} {selector.type_name} resolved 0 methods"
                )
                continue
            for row in rows:
                identity = f"{row['qualified_name']}#{row['signature']}"
                if identity in prior_identities:
                    continue
                method_id = int(row["id"])
                if method_id not in by_id:
                    by_id[method_id] = (row, set(), set())
                by_id[method_id][1].add(group.group_id)
                by_id[method_id][2].add(group.review_focus)
                newly_for_group += 1
        if newly_for_group == 0:
            failures.append(f"{group.group_id}: no new source identities remained after prior-review exclusion")
    if failures:
        raise FinalSeamsError("final-seams selector preflight failed:\n  - " + "\n  - ".join(failures))
    ordered = sorted(
        by_id.values(),
        key=lambda item: (str(item[0]["qualified_name"]), int(item[0]["start_line"]), str(item[0]["signature"])),
    )
    return [(row, tuple(sorted(group_ids)), tuple(sorted(focus))) for row, group_ids, focus in ordered]


def _source_excerpt(archive: zipfile.ZipFile, row: sqlite3.Row) -> str:
    path = str(row["path"])
    try:
        text = archive.read(path).decode("utf-8", errors="strict")
    except KeyError as error:
        raise FinalSeamsError(f"source member missing: {path}") from error
    lines = text.splitlines()
    start, end = int(row["start_line"]), int(row["end_line"])
    if not (1 <= start <= end <= len(lines)):
        raise FinalSeamsError(f"invalid Atlas line range for {path}: {start}-{end}")
    return "\n".join(lines[start - 1 : end]) + "\n"


def prepare(output_dir: Path, prior_dossier: Path, db: Path, source: Path, lock: Path) -> dict[str, object]:
    output = _external_fresh_dir(output_dir)
    prior_identities, prior_sha = validate_prior_review(prior_dossier)
    output.mkdir(parents=True)
    conn: sqlite3.Connection | None = None
    try:
        conn = vanilla_atlas.connect_db(db)
        source_sha = source_probe.require_pinned_source(conn, source, lock)
        if source_sha != EXPECTED_SOURCE_SHA256:
            raise FinalSeamsError(f"source pin mismatch: {source_sha}")
        resolved = resolve_groups(conn, GROUPS, prior_identities)
        dossier_candidates: list[dict[str, object]] = []
        worksheet_candidates: list[dict[str, object]] = []
        group_counts: dict[str, int] = {group.group_id: 0 for group in GROUPS}
        with zipfile.ZipFile(source) as archive:
            for index, (row, group_ids, focus) in enumerate(resolved, start=1):
                record = source_probe.record_template(conn, row, f"DISC-NET-R2B-PLAY-FINAL-SEAM-{index:03d}")
                source_record = dict(record["source"])
                hazards = sorted(set(record.get("atlas_observed_hazards", [])))
                identity = f"{source_record['type']}#{source_record['signature']}"
                excerpt = _source_excerpt(archive, row)
                for group_id in group_ids:
                    group_counts[group_id] += 1
                common = {
                    "candidate_id": f"DISC-NET-R2B-PLAY-FINAL-SEAM-{index:03d}",
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
        dossier = {
            "schema": SCHEMA,
            "id": REVIEW_ID,
            "kind": PREPARED_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "source_archive_sha256": source_sha,
            "prior_review": {
                "id": PRIOR_REVIEW_ID,
                "candidate_count": EXPECTED_PRIOR_COUNT,
                "dossier_sha256": prior_sha,
            },
            "scope": SCOPE,
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
            "prior_review": dossier["prior_review"],
            "scope": SCOPE,
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
            "prior_review": dossier["prior_review"],
            "scope": SCOPE,
            "candidate_count": len(dossier_candidates),
            "group_counts": group_counts,
            "artifacts": {
                "review_dossier": "review-dossier.json",
                "review_worksheet": "review-worksheet.json",
            },
            "next_required_step": (
                "Inspect every exact body. If these reusable primitives contain no escaping outbound delegate, "
                "canonicalize R2B VAR/SEM/gate plus the named commands/update_recipes composition artifacts."
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
    parser = argparse.ArgumentParser(prog="r2b-play-entry-final-seams-source-review")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--prior-dossier", type=Path, required=True)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        manifest = prepare(args.output_dir, args.prior_dossier, args.db, args.source, args.lock)
    except FinalSeamsError as error:
        print(f"R2B final-seams review error: {error}", file=sys.stderr)
        return 2
    print(f"r2b_play_entry_final_seams_review={args.output_dir}")
    print(f"candidates={manifest['candidate_count']}")
    print(f"groups={json.dumps(manifest['group_counts'], sort_keys=True)}")
    print("contains_official_source_text=true")
    print(f"commit_policy={COMMIT_POLICY}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
