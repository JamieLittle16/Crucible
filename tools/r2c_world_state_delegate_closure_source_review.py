#!/usr/bin/env python3
"""Build the bounded second-order R2C world-state source-review closure bundle.

The first R2C world-state review intentionally stopped rather than inferring two delegated semantic
seams: biome PalettedContainer wire representation and light DataLayer extraction. This tool closes
only those named delegates against the exact pinned Minecraft 26.2 source/Vanilla Atlas identity.

Source-rich output is written only into one external tar.gz. The companion worksheet and manifest
inside that archive are source-free. This tool discovers evidence; it does not admit production
semantics.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import re
import sqlite3
import sys
import tarfile
import zipfile
from collections import Counter
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Mapping, Sequence

try:
    from . import r1b_configuration_source_probe as source_probe
    from . import vanilla_atlas
except ImportError:
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]
    import vanilla_atlas  # type: ignore[no-redef]

SCHEMA = 1
PLAN_ID = "REVIEW-NET-R2C-WORLD-STATE-DELEGATE-CLOSURE-26_2-001"
PARENT_REVIEW_ID = "REVIEW-NET-R2C-WORLD-PROJECTION-DISCOVERY-26_2-001"
PACK_KIND = "r2c-world-state-delegate-closure-source-review-pack"
WORKSHEET_KIND = "r2c-world-state-delegate-closure-source-review-worksheet"
MANIFEST_KIND = "r2c-world-state-delegate-closure-source-review-manifest"
PACK_COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
WORKSHEET_COMMIT_POLICY = "SOURCE_FREE_REVIEW_ONLY_NOT_ADMISSION"
MANIFEST_COMMIT_POLICY = "SOURCE_FREE_UPLOAD_PROVENANCE"
EXPECTED_SOURCE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
EXPECTED_GROUPS = (
    ("R2C-BIOME-PALETTE-WIRE", "R2C-BIOMES"),
    ("R2C-LIGHT-DATA-LAYER", "R2C-LIGHT"),
)

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PLAN = REPO_ROOT / "vanilla/reviews/network/r2c-world-state-delegate-closure-plan.json"
DEFAULT_PARENT_PLAN = REPO_ROOT / "vanilla/reviews/network/r2c-world-projection-discovery-plan.json"
DEFAULT_FRONTIER = REPO_ROOT / "vanilla/frontiers/r2c-world-projection.json"
DEFAULT_DB = Path(".crucible/vanilla/atlas.sqlite")
DEFAULT_SOURCE = Path.home() / "Documents/mc-source/mc-src.zip"
DEFAULT_LOCK = REPO_ROOT / "vanilla/vanilla.lock.toml"


class ClosureError(RuntimeError):
    """Fail-closed delegate-closure review error."""


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
    parent_group_id: str
    review_focus: str
    selectors: tuple[Selector, ...]


@dataclass(frozen=True, slots=True)
class Plan:
    max_candidate_methods: int
    max_candidate_lines: int
    max_source_bytes: int
    groups: tuple[Group, ...]


_ROW_SELECT = """SELECT m.id,t.qualified_name,m.name,m.signature,m.param_count,
                        m.start_line,m.end_line,f.path
                 FROM methods m
                 JOIN types t ON t.id=m.type_id
                 JOIN source_files f ON f.id=t.file_id"""


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


def _external_output(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise ClosureError(f"output archive must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repository = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repository)
    except ValueError:
        return resolved
    raise ClosureError("source-rich delegate-closure bundle must live outside the repository")


def _read_json_object(path: Path, label: str) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ClosureError(f"cannot read {label}: {error}") from error
    if not isinstance(value, dict):
        raise ClosureError(f"{label} must be a JSON object")
    return value


def _selector(value: object, group_id: str, index: int) -> Selector:
    if not isinstance(value, dict):
        raise ClosureError(f"{group_id} selector {index} must be an object")
    if set(value) - {
        "mode",
        "required",
        "type_name",
        "type_prefix",
        "names",
        "signature",
        "name_regex",
    }:
        raise ClosureError(f"{group_id} selector {index} has unknown fields")
    mode = value.get("mode")
    required = value.get("required")
    if not isinstance(mode, str) or type(required) is not bool:
        raise ClosureError(f"{group_id} selector {index} requires string mode and bool required")

    type_name = value.get("type_name")
    type_prefix = value.get("type_prefix")
    raw_names = value.get("names", [])
    signature = value.get("signature")
    name_regex = value.get("name_regex")
    if type_name is not None and (
        not isinstance(type_name, str) or not type_name.startswith("net.minecraft.")
    ):
        raise ClosureError(f"{group_id} selector {index} has invalid type_name")
    if type_prefix is not None and (
        not isinstance(type_prefix, str) or not type_prefix.startswith("net.minecraft.")
    ):
        raise ClosureError(f"{group_id} selector {index} has invalid type_prefix")
    if not isinstance(raw_names, list) or any(
        not isinstance(item, str) or not item for item in raw_names
    ):
        raise ClosureError(f"{group_id} selector {index} names must be strings")
    if signature is not None and (not isinstance(signature, str) or not signature):
        raise ClosureError(f"{group_id} selector {index} has invalid signature")
    if name_regex is not None:
        if not isinstance(name_regex, str) or not name_regex:
            raise ClosureError(f"{group_id} selector {index} has invalid name_regex")
        try:
            re.compile(name_regex)
        except re.error as error:
            raise ClosureError(f"{group_id} selector {index} has invalid regex: {error}") from error

    allowed = {
        "exact_signature": {"type_name", "signature"},
        "type_all": {"type_name"},
        "type_names": {"type_name", "names"},
        "prefix_name_regex": {"type_prefix", "name_regex"},
    }
    if mode not in allowed:
        raise ClosureError(f"{group_id} selector {index} has unknown mode {mode!r}")
    present = {
        key
        for key, item in {
            "type_name": type_name,
            "type_prefix": type_prefix,
            "names": raw_names if raw_names else None,
            "signature": signature,
            "name_regex": name_regex,
        }.items()
        if item is not None
    }
    if present != allowed[mode]:
        raise ClosureError(
            f"{group_id} selector {index} mode {mode} requires "
            f"{sorted(allowed[mode])}, got {sorted(present)}"
        )
    return Selector(
        mode=mode,
        required=required,
        type_name=type_name,
        type_prefix=type_prefix,
        names=tuple(raw_names),
        signature=signature,
        name_regex=name_regex,
    )


def _load_plan(path: Path = DEFAULT_PLAN) -> Plan:
    value = _read_json_object(path, "R2C delegate-closure plan")
    expected = {
        "schema",
        "id",
        "parent_review_id",
        "source_archive_sha256",
        "max_candidate_methods",
        "max_candidate_lines",
        "max_source_bytes",
        "groups",
    }
    if set(value) != expected:
        raise ClosureError("R2C delegate-closure plan has unexpected fields")
    if value["schema"] != SCHEMA or value["id"] != PLAN_ID:
        raise ClosureError("R2C delegate-closure plan identity mismatch")
    if value["parent_review_id"] != PARENT_REVIEW_ID:
        raise ClosureError("R2C delegate-closure parent review mismatch")
    if value["source_archive_sha256"] != EXPECTED_SOURCE_SHA256:
        raise ClosureError("R2C delegate-closure source pin mismatch")

    max_methods = value["max_candidate_methods"]
    max_lines = value["max_candidate_lines"]
    max_bytes = value["max_source_bytes"]
    if type(max_methods) is not int or not 1 <= max_methods <= 512:
        raise ClosureError("max_candidate_methods must be in 1..512")
    if type(max_lines) is not int or not 1 <= max_lines <= 8192:
        raise ClosureError("max_candidate_lines must be in 1..8192")
    if type(max_bytes) is not int or not 1 <= max_bytes <= 16 * 1024 * 1024:
        raise ClosureError("max_source_bytes must be in 1..16777216")

    raw_groups = value["groups"]
    if not isinstance(raw_groups, list) or len(raw_groups) != len(EXPECTED_GROUPS):
        raise ClosureError("R2C delegate-closure plan must contain exactly two groups")
    groups: list[Group] = []
    for index, ((expected_group, expected_parent), raw_group) in enumerate(
        zip(EXPECTED_GROUPS, raw_groups, strict=True)
    ):
        if not isinstance(raw_group, dict) or set(raw_group) != {
            "group_id",
            "parent_group_id",
            "review_focus",
            "selectors",
        }:
            raise ClosureError(f"delegate-closure group {index} has unexpected fields")
        if raw_group["group_id"] != expected_group or raw_group["parent_group_id"] != expected_parent:
            raise ClosureError("delegate-closure groups are missing or out of canonical order")
        focus = raw_group["review_focus"]
        selectors = raw_group["selectors"]
        if not isinstance(focus, str) or not focus:
            raise ClosureError(f"{expected_group} review_focus must be non-empty")
        if not isinstance(selectors, list) or not selectors:
            raise ClosureError(f"{expected_group} selectors must be non-empty")
        groups.append(
            Group(
                expected_group,
                expected_parent,
                focus,
                tuple(_selector(item, expected_group, i) for i, item in enumerate(selectors)),
            )
        )
    return Plan(max_methods, max_lines, max_bytes, tuple(groups))


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
                raise ClosureError(
                    f"required names missing from {selector.type_name}: {sorted(missing)}"
                )
        return rows
    if selector.mode == "prefix_name_regex":
        rows = conn.execute(
            _ROW_SELECT + " WHERE t.qualified_name LIKE ? ORDER BY t.qualified_name,m.start_line,m.id",
            (f"{selector.type_prefix}%",),
        ).fetchall()
        pattern = re.compile(selector.name_regex or "")
        return [row for row in rows if pattern.search(str(row["name"]))]
    raise AssertionError(selector.mode)


def _resolve_groups(
    conn: sqlite3.Connection, plan: Plan
) -> list[tuple[sqlite3.Row, tuple[str, ...], tuple[str, ...]]]:
    by_id: dict[int, tuple[sqlite3.Row, set[str], set[str]]] = {}
    failures: list[str] = []
    for group in plan.groups:
        group_rows = 0
        for selector in group.selectors:
            try:
                rows = _query_selector(conn, selector)
            except ClosureError as error:
                failures.append(f"{group.group_id}: {error}")
                continue
            if selector.required and not rows:
                descriptor = selector.type_name or selector.type_prefix
                failures.append(
                    f"{group.group_id}: required selector {selector.mode} {descriptor} resolved zero methods"
                )
                continue
            group_rows += len(rows)
            for row in rows:
                method_id = int(row["id"])
                if method_id not in by_id:
                    by_id[method_id] = (row, set(), set())
                by_id[method_id][1].add(group.group_id)
                by_id[method_id][2].add(group.review_focus)
        if group_rows == 0:
            failures.append(f"{group.group_id}: no methods resolved")
    if failures:
        raise ClosureError("delegate-closure selector preflight failed:\n  - " + "\n  - ".join(failures))
    if len(by_id) > plan.max_candidate_methods:
        raise ClosureError(
            f"delegate-closure candidate count {len(by_id)} exceeds cap {plan.max_candidate_methods}"
        )
    ordered = sorted(
        by_id.values(),
        key=lambda item: (
            str(item[0]["qualified_name"]),
            int(item[0]["start_line"]),
            str(item[0]["signature"]),
        ),
    )
    return [
        (row, tuple(sorted(group_ids)), tuple(sorted(focus)))
        for row, group_ids, focus in ordered
    ]


def _safe_member(value: object) -> str:
    if not isinstance(value, str) or not value or "\\" in value:
        raise ClosureError("candidate source path is invalid")
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts or "." in path.parts:
        raise ClosureError(f"unsafe candidate source path: {value}")
    return path.as_posix()


def _source_excerpt(
    archive: zipfile.ZipFile,
    row: Mapping[str, object],
    *,
    max_lines: int,
    cache: dict[str, list[str]],
) -> bytes:
    member = _safe_member(row["path"])
    start = int(row["start_line"])
    end = int(row["end_line"])
    if start < 1 or end < start or end - start + 1 > max_lines:
        raise ClosureError(
            f"candidate line range exceeds review bound: {row['qualified_name']}#{row['signature']}"
        )
    if member not in cache:
        try:
            raw = archive.read(member)
        except KeyError as error:
            raise ClosureError(f"source archive member is missing: {member}") from error
        try:
            cache[member] = raw.decode("utf-8", errors="strict").splitlines(keepends=True)
        except UnicodeDecodeError as error:
            raise ClosureError(f"source archive member is not UTF-8: {member}") from error
    lines = cache[member]
    if end > len(lines):
        raise ClosureError(f"candidate line range exceeds source member: {member}:{start}-{end}")
    return "".join(lines[start - 1 : end]).encode("utf-8")


def _call_inventory(conn: sqlite3.Connection, method_id: int) -> dict[str, object]:
    rows = conn.execute(
        """SELECT c.owner_text,c.callee_name,c.arg_count,c.line,
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
        if row["target_type"] is not None and row["target_signature"] is not None:
            resolved.append(f"{row['target_type']}#{row['target_signature']}")
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


def _candidate_record(
    conn: sqlite3.Connection,
    row: sqlite3.Row,
    candidate_id: str,
    group_ids: Sequence[str],
    review_focus: Sequence[str],
    excerpt: bytes,
) -> dict[str, object]:
    template = source_probe.record_template(conn, row, candidate_id)
    source = template.get("source")
    if not isinstance(source, dict):
        raise ClosureError(f"invalid Atlas source metadata for {candidate_id}")
    return {
        "candidate_id": candidate_id,
        "source_identity": f"{row['qualified_name']}#{row['signature']}",
        "source": source,
        "source_location": {
            "path": str(row["path"]),
            "start_line": int(row["start_line"]),
            "end_line": int(row["end_line"]),
        },
        "atlas_observed_hazards": list(template.get("atlas_observed_hazards", [])),
        "atlas_classifications": list(template.get("classifications", [])),
        "calls": _call_inventory(conn, int(row["id"])),
        "group_ids": list(group_ids),
        "review_focus": list(review_focus),
        "source_excerpt": excerpt.decode("utf-8"),
        "source_excerpt_sha256": _sha256_bytes(excerpt),
    }


def _source_free_candidate(record: Mapping[str, object]) -> dict[str, object]:
    return {
        key: record[key]
        for key in (
            "candidate_id",
            "source_identity",
            "source",
            "source_location",
            "atlas_observed_hazards",
            "atlas_classifications",
            "calls",
            "group_ids",
            "review_focus",
        )
    }


def _payloads(
    *,
    plan: Plan,
    plan_sha256: str,
    parent_plan_sha256: str,
    frontier_sha256: str,
    source_sha256: str,
    records: Sequence[Mapping[str, object]],
) -> dict[str, bytes]:
    source_free = [_source_free_candidate(record) for record in records]
    by_identity = {str(record["source_identity"]): record for record in source_free}
    groups: list[dict[str, object]] = []
    for group in plan.groups:
        identities = [
            str(record["source_identity"])
            for record in source_free
            if group.group_id in record["group_ids"]  # type: ignore[operator]
        ]
        groups.append(
            {
                "group_id": group.group_id,
                "parent_group_id": group.parent_group_id,
                "review_focus": group.review_focus,
                "candidates": [by_identity[identity] for identity in identities],
                "source_inspected": False,
                "selected_source_identities": [],
                "rejected_source_identities": [],
                "hazards_reviewed": [],
                "followup_dependencies": [],
                "semantic_observations": [],
                "review_complete": False,
            }
        )

    pack: dict[str, object] = {
        "schema": SCHEMA,
        "kind": PACK_KIND,
        "review_id": PLAN_ID,
        "parent_review_id": PARENT_REVIEW_ID,
        "commit_policy": PACK_COMMIT_POLICY,
        "contains_official_source_text": True,
        "production_admitted": False,
        "source_archive_sha256": source_sha256,
        "plan_sha256": plan_sha256,
        "parent_discovery_plan_sha256": parent_plan_sha256,
        "frontier_sha256": frontier_sha256,
        "groups": [
            {
                "group_id": group.group_id,
                "parent_group_id": group.parent_group_id,
                "review_focus": group.review_focus,
            }
            for group in plan.groups
        ],
        "source_records": list(records),
        "unique_source_records": len(records),
        "source_excerpt_bytes": sum(
            len(str(record["source_excerpt"]).encode("utf-8")) for record in records
        ),
    }
    pack_bytes = _pretty_bytes(pack)
    pack_sha = _sha256_bytes(pack_bytes)

    worksheet: dict[str, object] = {
        "schema": SCHEMA,
        "kind": WORKSHEET_KIND,
        "review_id": PLAN_ID,
        "parent_review_id": PARENT_REVIEW_ID,
        "commit_policy": WORKSHEET_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": source_sha256,
        "plan_sha256": plan_sha256,
        "review_pack_sha256": pack_sha,
        "groups": groups,
    }
    worksheet_bytes = _pretty_bytes(worksheet)
    worksheet_sha = _sha256_bytes(worksheet_bytes)

    manifest: dict[str, object] = {
        "schema": SCHEMA,
        "kind": MANIFEST_KIND,
        "review_id": PLAN_ID,
        "commit_policy": MANIFEST_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": source_sha256,
        "plan_sha256": plan_sha256,
        "parent_discovery_plan_sha256": parent_plan_sha256,
        "frontier_sha256": frontier_sha256,
        "files": [
            {
                "path": "review-pack.json",
                "sha256": pack_sha,
                "size": len(pack_bytes),
                "source_rich": True,
            },
            {
                "path": "worksheet.json",
                "sha256": worksheet_sha,
                "size": len(worksheet_bytes),
                "source_rich": False,
            },
        ],
    }
    manifest_bytes = _pretty_bytes(manifest)
    return {
        "review-pack.json": pack_bytes,
        "worksheet.json": worksheet_bytes,
        "manifest.json": manifest_bytes,
    }


def _add_bytes(archive: tarfile.TarFile, name: str, data: bytes) -> None:
    info = tarfile.TarInfo(name=name)
    info.size = len(data)
    info.mtime = 0
    info.mode = 0o600
    info.uid = 0
    info.gid = 0
    archive.addfile(info, io.BytesIO(data))


def _write_archive(path: Path, payloads: Mapping[str, bytes]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(path, mode="w:gz") as archive:
        for name in sorted(payloads):
            _add_bytes(archive, name, payloads[name])
    with tarfile.open(path, mode="r:gz") as archive:
        members = archive.getmembers()
        names = {member.name for member in members}
        if names != set(payloads) or any(
            not member.isfile() or member.issym() or member.islnk() for member in members
        ):
            raise ClosureError("delegate-closure archive member set/type mismatch")
        for member in members:
            stream = archive.extractfile(member)
            if stream is None or stream.read() != payloads[member.name]:
                raise ClosureError(f"delegate-closure archive verification failed: {member.name}")


def build_bundle(
    *,
    output: Path,
    plan_path: Path = DEFAULT_PLAN,
    db_path: Path = DEFAULT_DB,
    source_archive: Path = DEFAULT_SOURCE,
    lock_path: Path = DEFAULT_LOCK,
) -> dict[str, object]:
    output = _external_output(output)
    plan = _load_plan(plan_path)

    conn = vanilla_atlas.connect_db(db_path)
    try:
        source_sha = source_probe.require_pinned_source(conn, source_archive, lock_path)
        if source_sha != EXPECTED_SOURCE_SHA256:
            raise ClosureError(f"delegate-closure source pin mismatch: {source_sha}")
        resolved = _resolve_groups(conn, plan)

        records: list[dict[str, object]] = []
        total_source_bytes = 0
        with zipfile.ZipFile(source_archive) as source_zip:
            cache: dict[str, list[str]] = {}
            for index, (row, group_ids, focus) in enumerate(resolved, start=1):
                excerpt = _source_excerpt(
                    source_zip,
                    row,
                    max_lines=plan.max_candidate_lines,
                    cache=cache,
                )
                total_source_bytes += len(excerpt)
                if total_source_bytes > plan.max_source_bytes:
                    raise ClosureError(
                        f"delegate-closure source bytes {total_source_bytes} exceed cap "
                        f"{plan.max_source_bytes}"
                    )
                records.append(
                    _candidate_record(
                        conn,
                        row,
                        f"DISC-NET-R2C-WORLD-DELEGATE-{index:04d}",
                        group_ids,
                        focus,
                        excerpt,
                    )
                )
    finally:
        conn.close()

    payloads = _payloads(
        plan=plan,
        plan_sha256=_sha256_file(plan_path),
        parent_plan_sha256=_sha256_file(DEFAULT_PARENT_PLAN),
        frontier_sha256=_sha256_file(DEFAULT_FRONTIER),
        source_sha256=source_sha,
        records=records,
    )
    _write_archive(output, payloads)
    return {
        "output": str(output),
        "sha256": _sha256_file(output),
        "review_pack_sha256": _sha256_bytes(payloads["review-pack.json"]),
        "worksheet_sha256": _sha256_bytes(payloads["worksheet.json"]),
        "manifest_sha256": _sha256_bytes(payloads["manifest.json"]),
        "candidate_methods": len(records),
        "source_excerpt_bytes": total_source_bytes,
        "groups": len(plan.groups),
        "contains_official_source_text": True,
        "production_admitted": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = build_bundle(
            output=args.output,
            plan_path=args.plan,
            db_path=args.db,
            source_archive=args.source,
            lock_path=args.lock,
        )
    except (
        ClosureError,
        OSError,
        sqlite3.Error,
        source_probe.ProbeError,
        zipfile.BadZipFile,
        tarfile.TarError,
    ) as error:
        print(f"R2C world-state delegate closure failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
