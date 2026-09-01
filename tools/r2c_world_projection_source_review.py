#!/usr/bin/env python3
"""Prepare bounded, source-free R2C world-projection discovery from the pinned Vanilla Atlas.

This is the first mechanical step of R2C.1. It deliberately does not promote source discovery into
semantic admission. The tool binds the disposable local Atlas and official source archive to
`vanilla.lock.toml`, resolves the exact type roots named by the committed review plan, and emits a
content-addressed inventory/worksheet outside the repository.

No official source body or excerpt is written. The output contains only source-free Atlas metadata,
fingerprints, locations, hazards and call structure. A later explicit review step must inspect the
pinned source, select exact declarations, close delegates/hazards, materialize VAR/SEM records and
pass the independent source gate before `helve-target-26-2` may rely on any R2C wire fact.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sqlite3
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Sequence

try:
    from . import r1b_configuration_source_probe as source_probe
    from . import vanilla_atlas
except ImportError:  # Direct `python3 tools/...` execution.
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]
    import vanilla_atlas  # type: ignore[no-redef]

SCHEMA = 1
PLAN_ID = "REVIEW-NET-R2C-WORLD-PROJECTION-DISCOVERY-26_2-001"
DISCOVERY_KIND = "r2c-world-projection-source-discovery"
WORKSHEET_KIND = "r2c-world-projection-source-discovery-worksheet"
MANIFEST_KIND = "r2c-world-projection-source-discovery-manifest"
COMMIT_POLICY = "SOURCE_FREE_LOCAL_DISCOVERY_DO_NOT_COMMIT_WITHOUT_REVIEW"
EXPECTED_SOURCE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
EXPECTED_TARGET = {
    "minecraft_version": "26.2",
    "protocol_version": 776,
    "world_version": 4903,
}
EXPECTED_GROUPS = (
    "R2C-WORLD-ENTRY",
    "R2C-CHUNK-SPAN",
    "R2C-BLOCK-SECTIONS",
    "R2C-BIOMES",
    "R2C-HEIGHTMAPS",
    "R2C-LIGHT",
    "R2C-BLOCK-ENTITIES",
    "R2C-PACING",
    "R2C-PACKET-IDS",
)

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PLAN = REPO_ROOT / "vanilla/reviews/network/r2c-world-projection-discovery-plan.json"
DEFAULT_DB = Path(".crucible/vanilla/atlas.sqlite")
DEFAULT_SOURCE = Path.home() / "Documents/mc-source/mc-src.zip"
DEFAULT_LOCK = REPO_ROOT / "vanilla/vanilla.lock.toml"


class DiscoveryError(RuntimeError):
    """Fail-closed R2C source-discovery error."""


@dataclass(frozen=True, slots=True)
class ReviewGroup:
    """One semantic review group and its exact Atlas type roots."""

    group_id: str
    review_focus: str
    root_types: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class DiscoveryPlan:
    """Validated committed discovery plan."""

    scope: str
    frontier: Path
    max_methods_per_type: int
    groups: tuple[ReviewGroup, ...]


def _pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _fresh_external_dir(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise DiscoveryError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repository = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repository)
    except ValueError:
        return resolved
    raise DiscoveryError("R2C source-discovery output must live outside the repository")


def _read_json_object(path: Path, label: str) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise DiscoveryError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise DiscoveryError(f"{label} must be a JSON object")
    return value


def _load_plan(path: Path) -> DiscoveryPlan:
    value = _read_json_object(path, "R2C discovery plan")
    expected_fields = {
        "schema",
        "id",
        "source_archive_sha256",
        "frontier",
        "scope",
        "max_methods_per_type",
        "groups",
    }
    if set(value) != expected_fields:
        raise DiscoveryError("R2C discovery plan has unexpected fields")
    if value["schema"] != SCHEMA or value["id"] != PLAN_ID:
        raise DiscoveryError("R2C discovery plan identity mismatch")
    if value["source_archive_sha256"] != EXPECTED_SOURCE_SHA256:
        raise DiscoveryError("R2C discovery plan source commitment mismatch")

    scope = value["scope"]
    frontier_value = value["frontier"]
    max_methods = value["max_methods_per_type"]
    raw_groups = value["groups"]
    if not isinstance(scope, str) or not scope:
        raise DiscoveryError("R2C discovery plan scope must be non-empty")
    if not isinstance(frontier_value, str) or not frontier_value:
        raise DiscoveryError("R2C discovery plan frontier must be a non-empty path")
    if type(max_methods) is not int or not 1 <= max_methods <= 4096:
        raise DiscoveryError("max_methods_per_type must be an integer in 1..4096")
    if not isinstance(raw_groups, list) or not raw_groups:
        raise DiscoveryError("R2C discovery plan groups must be non-empty")

    groups: list[ReviewGroup] = []
    seen_groups: set[str] = set()
    for index, raw_group in enumerate(raw_groups):
        if not isinstance(raw_group, dict) or set(raw_group) != {
            "group_id",
            "review_focus",
            "root_types",
        }:
            raise DiscoveryError(f"R2C discovery group {index} has unexpected fields")
        group_id = raw_group["group_id"]
        review_focus = raw_group["review_focus"]
        root_types = raw_group["root_types"]
        if not isinstance(group_id, str) or not group_id or group_id in seen_groups:
            raise DiscoveryError(f"R2C discovery group {index} has invalid/duplicate group_id")
        if not isinstance(review_focus, str) or not review_focus:
            raise DiscoveryError(f"{group_id} review_focus must be non-empty")
        if (
            not isinstance(root_types, list)
            or not root_types
            or any(not isinstance(root, str) or not root for root in root_types)
        ):
            raise DiscoveryError(f"{group_id} root_types must be a non-empty string array")
        if len(root_types) != len(set(root_types)):
            raise DiscoveryError(f"{group_id} root_types contains duplicates")
        if any(not root.startswith("net.minecraft.") for root in root_types):
            raise DiscoveryError(f"{group_id} contains a non-Minecraft root type")
        groups.append(ReviewGroup(group_id, review_focus, tuple(root_types)))
        seen_groups.add(group_id)

    if tuple(group.group_id for group in groups) != EXPECTED_GROUPS:
        raise DiscoveryError("R2C discovery plan must contain the nine semantic groups in canonical order")

    frontier = (REPO_ROOT / frontier_value).resolve(strict=False)
    return DiscoveryPlan(scope, frontier, max_methods, tuple(groups))


def _load_frontier(path: Path, plan: DiscoveryPlan) -> dict[str, object]:
    value = _read_json_object(path, "R2C source frontier")
    if value.get("schema") != SCHEMA:
        raise DiscoveryError("R2C source frontier schema mismatch")
    if value.get("target") != EXPECTED_TARGET:
        raise DiscoveryError("R2C source frontier target mismatch")
    if value.get("semantic_groups") != list(EXPECTED_GROUPS):
        raise DiscoveryError("R2C source frontier semantic-group mismatch")

    roots = value.get("root_queries")
    if (
        not isinstance(roots, list)
        or not roots
        or any(not isinstance(root, str) or not root for root in roots)
        or len(roots) != len(set(roots))
    ):
        raise DiscoveryError("R2C source frontier root_queries must be unique non-empty strings")

    planned_roots = {root for group in plan.groups for root in group.root_types}
    if set(roots) != planned_roots:
        missing = sorted(set(roots) - planned_roots)
        extra = sorted(planned_roots - set(roots))
        raise DiscoveryError(
            f"R2C discovery plan/frontier root mismatch: unplanned={missing} non_frontier={extra}"
        )
    return value


_TYPE_SELECT = """SELECT t.id,t.qualified_name,f.path
                  FROM types t
                  JOIN source_files f ON f.id=t.file_id
                  WHERE t.qualified_name=?
                  ORDER BY t.id"""

_METHOD_SELECT = """SELECT m.id,t.qualified_name,m.name,m.signature,m.param_count,
                           m.start_line,m.end_line,f.path
                    FROM methods m
                    JOIN types t ON t.id=m.type_id
                    JOIN source_files f ON f.id=t.file_id
                    WHERE t.qualified_name=?
                    ORDER BY m.start_line,m.id"""


def _exact_type_methods(
    conn: sqlite3.Connection,
    qualified_name: str,
    max_methods: int,
) -> list[sqlite3.Row]:
    types = conn.execute(_TYPE_SELECT, (qualified_name,)).fetchall()
    if not types:
        raise DiscoveryError(f"R2C root type resolved zero Atlas types: {qualified_name}")
    if len(types) != 1:
        raise DiscoveryError(
            f"R2C root type must resolve exactly once in Atlas: {qualified_name} matched {len(types)}"
        )

    rows = conn.execute(_METHOD_SELECT, (qualified_name,)).fetchall()
    if len(rows) > max_methods:
        raise DiscoveryError(
            f"R2C root type is too broad for bounded discovery: {qualified_name} "
            f"resolved {len(rows)} methods > cap {max_methods}"
        )
    return rows


def _source_identity(row: sqlite3.Row) -> str:
    return f"{row['qualified_name']}#{row['signature']}"


def _call_inventory(conn: sqlite3.Connection, method_id: int) -> dict[str, object]:
    rows = conn.execute(
        """SELECT c.owner_text,c.callee_name,c.arg_count,c.line,c.resolution,
                  target_type.qualified_name AS target_type,target.signature AS target_signature
           FROM method_calls c
           LEFT JOIN methods target ON target.id=c.resolved_method_id
           LEFT JOIN types target_type ON target_type.id=target.type_id
           WHERE c.caller_method_id=?
           ORDER BY c.line,c.id""",
        (method_id,),
    ).fetchall()
    resolved: list[str] = []
    unresolved: Counter[str] = Counter()
    for row in rows:
        target_type = row["target_type"]
        target_signature = row["target_signature"]
        if target_type is not None and target_signature is not None:
            resolved.append(f"{target_type}#{target_signature}")
        else:
            owner = f"{row['owner_text']}." if row["owner_text"] else ""
            unresolved[f"{owner}{row['callee_name']}/{row['arg_count']}"] += 1
    return {
        "call_sites": len(rows),
        "resolved_targets": sorted(set(resolved)),
        "unresolved_call_sites": sum(unresolved.values()),
        "top_unresolved_callees": [
            {"callee": callee, "sites": sites}
            for callee, sites in unresolved.most_common(24)
        ],
    }


def _method_inventory(
    conn: sqlite3.Connection,
    row: sqlite3.Row,
    candidate_id: str,
) -> dict[str, object]:
    template = source_probe.record_template(conn, row, candidate_id)
    source = template["source"]
    if not isinstance(source, dict):
        raise DiscoveryError(f"Atlas record template has invalid source object for {candidate_id}")
    hazards = template.get("atlas_observed_hazards", [])
    classifications = template.get("classifications", [])
    return {
        "candidate_id": candidate_id,
        "source_identity": _source_identity(row),
        "source": source,
        "source_location": {
            "path": str(row["path"]),
            "start_line": int(row["start_line"]),
            "end_line": int(row["end_line"]),
        },
        "atlas_observed_hazards": list(hazards) if isinstance(hazards, list) else [],
        "atlas_classifications": list(classifications) if isinstance(classifications, list) else [],
        "calls": _call_inventory(conn, int(row["id"])),
    }


def _group_inventory(
    conn: sqlite3.Connection,
    group: ReviewGroup,
    max_methods: int,
    method_cache: dict[int, dict[str, object]],
    candidate_ids: dict[int, str],
) -> dict[str, object]:
    method_ids: set[int] = set()
    roots: list[dict[str, object]] = []
    for root_type in group.root_types:
        rows = _exact_type_methods(conn, root_type, max_methods)
        ids = [int(row["id"]) for row in rows]
        method_ids.update(ids)
        roots.append({
            "type": root_type,
            "method_count": len(rows),
            "declaration_only": not rows,
            "source_identities": [_source_identity(row) for row in rows],
        })
        for row in rows:
            method_id = int(row["id"])
            if method_id not in candidate_ids:
                candidate_ids[method_id] = f"DISC-NET-R2C-WORLD-{len(candidate_ids) + 1:04d}"
            if method_id not in method_cache:
                method_cache[method_id] = _method_inventory(
                    conn,
                    row,
                    candidate_ids[method_id],
                )

    methods = [method_cache[method_id] for method_id in sorted(method_ids)]
    hazard_counts: Counter[str] = Counter()
    unresolved = 0
    for method in methods:
        for hazard in method["atlas_observed_hazards"]:
            hazard_counts[str(hazard)] += 1
        calls = method["calls"]
        if isinstance(calls, dict):
            unresolved += int(calls.get("unresolved_call_sites", 0))
    return {
        "group_id": group.group_id,
        "review_focus": group.review_focus,
        "roots": roots,
        "candidate_methods": methods,
        "candidate_method_count": len(methods),
        "atlas_hazard_method_counts": dict(sorted(hazard_counts.items())),
        "unresolved_call_sites": unresolved,
        "production_admitted": False,
    }


def prepare(
    output_dir: Path,
    plan_path: Path,
    db_path: Path,
    source_archive: Path,
    lock_path: Path,
) -> dict[str, object]:
    output = _fresh_external_dir(output_dir)
    plan = _load_plan(plan_path)
    frontier = _load_frontier(plan.frontier, plan)

    conn: sqlite3.Connection | None = None
    output.mkdir(parents=True)
    try:
        conn = vanilla_atlas.connect_db(db_path)
        source_sha = source_probe.require_pinned_source(conn, source_archive, lock_path)
        if source_sha != EXPECTED_SOURCE_SHA256:
            raise DiscoveryError(f"R2C source pin mismatch: {source_sha}")

        meta = dict(conn.execute("SELECT key,value FROM meta"))
        method_cache: dict[int, dict[str, object]] = {}
        candidate_ids: dict[int, str] = {}
        groups = [
            _group_inventory(
                conn,
                group,
                plan.max_methods_per_type,
                method_cache,
                candidate_ids,
            )
            for group in plan.groups
        ]

        discovery: dict[str, object] = {
            "schema": SCHEMA,
            "kind": DISCOVERY_KIND,
            "review_id": PLAN_ID,
            "commit_policy": COMMIT_POLICY,
            "scope": plan.scope,
            "source": {
                "minecraft_version": meta.get("minecraft_version"),
                "protocol_version": int(meta.get("protocol_version", "-1")),
                "world_version": int(meta.get("world_version", "-1")),
                "archive_sha256": source_sha,
                "atlas_version": meta.get("atlas_version"),
                "fingerprint_algorithm": meta.get("fingerprint_algorithm"),
            },
            "inputs": {
                "plan": str(plan_path),
                "plan_sha256": _sha256_file(plan_path),
                "frontier": str(plan.frontier),
                "frontier_sha256": _sha256_file(plan.frontier),
            },
            "frontier": {
                "root_queries": frontier["root_queries"],
                "semantic_groups": frontier["semantic_groups"],
                "max_depth": frontier.get("max_depth"),
            },
            "groups": groups,
            "unique_candidate_methods": len(method_cache),
            "source_text_included": False,
            "production_admitted": False,
            "next_step": (
                "Inspect pinned official source for selected exact candidate identities; close "
                "delegates/hazards; then materialize source-free VAR/SEM and run the independent gate."
            ),
        }
        discovery_bytes = _pretty_bytes(discovery)
        discovery_sha = _sha256_bytes(discovery_bytes)

        worksheet: dict[str, object] = {
            "schema": SCHEMA,
            "kind": WORKSHEET_KIND,
            "review_id": PLAN_ID,
            "discovery_sha256": discovery_sha,
            "groups": [
                {
                    "group_id": group.group_id,
                    "source_inspected": False,
                    "selected_source_identities": [],
                    "rejected_source_identities": [],
                    "hazards_reviewed": [],
                    "followup_dependencies": [],
                    "semantic_observations": [],
                    "review_complete": False,
                }
                for group in plan.groups
            ],
            "production_admitted": False,
        }
        worksheet_bytes = _pretty_bytes(worksheet)
        worksheet_sha = _sha256_bytes(worksheet_bytes)

        manifest: dict[str, object] = {
            "schema": SCHEMA,
            "kind": MANIFEST_KIND,
            "review_id": PLAN_ID,
            "files": [
                {
                    "path": "discovery.json",
                    "size": len(discovery_bytes),
                    "sha256": discovery_sha,
                },
                {
                    "path": "worksheet.json",
                    "size": len(worksheet_bytes),
                    "sha256": worksheet_sha,
                },
            ],
            "source_text_included": False,
            "production_admitted": False,
        }
        manifest_bytes = _pretty_bytes(manifest)

        (output / "discovery.json").write_bytes(discovery_bytes)
        (output / "worksheet.json").write_bytes(worksheet_bytes)
        (output / "manifest.json").write_bytes(manifest_bytes)
        return {
            "output_dir": str(output),
            "discovery_sha256": discovery_sha,
            "worksheet_sha256": worksheet_sha,
            "unique_candidate_methods": len(method_cache),
            "groups": len(groups),
            "production_admitted": False,
        }
    except (OSError, sqlite3.Error, source_probe.ProbeError) as error:
        raise DiscoveryError(str(error)) from error
    finally:
        if conn is not None:
            conn.close()


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = prepare(args.output_dir, args.plan, args.db, args.source, args.lock)
    except DiscoveryError as error:
        print(f"R2C source discovery failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
