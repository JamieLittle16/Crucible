#!/usr/bin/env python3
"""Bind target block-state extraction to the pinned official source evidence.

The Mojang source archive and Atlas SQLite database remain local/generated artifacts.
This tool consumes the committed locator specification plus `vanilla.lock.toml`, verifies
that a local Atlas database was built from the exact pinned source archive, resolves every
required source surface unambiguously, and emits a compact fingerprint-only qualification
artifact suitable for committing.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sqlite3
import sys
import tomllib
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1


def canonical_json_bytes(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object in {path}")
    return value


def load_lock(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        value = tomllib.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"expected TOML table in {path}")
    return value


def atlas_meta(conn: sqlite3.Connection) -> dict[str, str]:
    try:
        rows = conn.execute("SELECT key,value FROM meta").fetchall()
    except sqlite3.Error as exc:
        raise ValueError("Atlas database is missing the meta table") from exc
    return {str(key): str(value) for key, value in rows}


def require_equal(label: str, actual: object, expected: object) -> None:
    if actual != expected:
        raise ValueError(f"{label} mismatch: expected {expected!r}, got {actual!r}")


def resolve_type(conn: sqlite3.Connection, owner: str) -> dict[str, Any]:
    rows = conn.execute(
        """
        SELECT sf.path,sf.sha256,t.qualified_name,t.kind,t.normalized_sha256,
               t.start_line,t.end_line
        FROM types t
        JOIN source_files sf ON sf.id=t.file_id
        WHERE t.qualified_name=?
        """,
        (owner,),
    ).fetchall()
    if len(rows) != 1:
        raise ValueError(f"expected exactly one Atlas type {owner}, found {len(rows)}")
    path, file_sha, qualified_name, kind, normalized_sha, start_line, end_line = rows[0]
    return {
        "kind": "type",
        "source_path": str(path),
        "source_file_sha256": str(file_sha),
        "owner": str(qualified_name),
        "declaration_kind": str(kind),
        "normalized_sha256": str(normalized_sha),
        "diagnostic_lines": [int(start_line), int(end_line)],
    }


def resolve_field(conn: sqlite3.Connection, owner: str, name: str) -> dict[str, Any]:
    rows = conn.execute(
        """
        SELECT sf.path,sf.sha256,t.qualified_name,f.name,f.type_name,f.modifiers,f.line
        FROM fields f
        JOIN types t ON t.id=f.type_id
        JOIN source_files sf ON sf.id=t.file_id
        WHERE t.qualified_name=? AND f.name=?
        """,
        (owner, name),
    ).fetchall()
    if len(rows) != 1:
        raise ValueError(
            f"expected exactly one Atlas field {owner}#{name}, found {len(rows)}"
        )
    path, file_sha, qualified_name, field_name, type_name, modifiers, line = rows[0]
    return {
        "kind": "field",
        "source_path": str(path),
        "source_file_sha256": str(file_sha),
        "owner": str(qualified_name),
        "name": str(field_name),
        "type_name": str(type_name),
        "modifiers": str(modifiers),
        "diagnostic_line": int(line),
    }


def resolve_method(
    conn: sqlite3.Connection,
    owner: str,
    name: str,
    param_count: int,
) -> dict[str, Any]:
    rows = conn.execute(
        """
        SELECT sf.path,sf.sha256,t.qualified_name,m.name,m.signature,m.return_type,
               m.modifiers,m.param_count,m.body_sha256,m.normalized_sha256,
               m.start_line,m.end_line
        FROM methods m
        JOIN types t ON t.id=m.type_id
        JOIN source_files sf ON sf.id=t.file_id
        WHERE t.qualified_name=? AND m.name=? AND m.param_count=?
        """,
        (owner, name, param_count),
    ).fetchall()
    if len(rows) != 1:
        raise ValueError(
            f"expected exactly one Atlas method {owner}#{name}/{param_count}, "
            f"found {len(rows)}"
        )
    (
        path,
        file_sha,
        qualified_name,
        method_name,
        signature,
        return_type,
        modifiers,
        actual_param_count,
        body_sha,
        normalized_sha,
        start_line,
        end_line,
    ) = rows[0]
    return {
        "kind": "method",
        "source_path": str(path),
        "source_file_sha256": str(file_sha),
        "owner": str(qualified_name),
        "name": str(method_name),
        "signature": str(signature),
        "return_type": str(return_type),
        "modifiers": str(modifiers),
        "param_count": int(actual_param_count),
        "body_sha256": str(body_sha),
        "normalized_sha256": str(normalized_sha),
        "diagnostic_lines": [int(start_line), int(end_line)],
    }


def resolve_locator(conn: sqlite3.Connection, locator: dict[str, Any]) -> dict[str, Any]:
    kind = locator.get("kind")
    owner = locator.get("owner")
    if not isinstance(owner, str) or not owner:
        raise ValueError("source locator requires non-empty owner")
    if kind == "type":
        return resolve_type(conn, owner)
    if kind == "field":
        name = locator.get("name")
        if not isinstance(name, str) or not name:
            raise ValueError(f"field locator for {owner} requires non-empty name")
        return resolve_field(conn, owner, name)
    if kind == "method":
        name = locator.get("name")
        param_count = locator.get("param_count")
        if not isinstance(name, str) or not name:
            raise ValueError(f"method locator for {owner} requires non-empty name")
        if not isinstance(param_count, int) or param_count < 0:
            raise ValueError(f"method locator {owner}#{name} requires param_count >= 0")
        return resolve_method(conn, owner, name, param_count)
    raise ValueError(f"unsupported source locator kind: {kind!r}")


def qualify(lock_path: Path, spec_path: Path, atlas_path: Path) -> dict[str, Any]:
    lock = load_lock(lock_path)
    spec = load_json(spec_path)
    require_equal("qualification spec schema", spec.get("schema"), SCHEMA_VERSION)

    target = spec.get("target")
    if not isinstance(target, dict):
        raise ValueError("qualification spec requires target object")
    expected_target = {
        "minecraft_version": str(lock["minecraft"]),
        "protocol_version": int(lock["protocol"]),
        "data_version": int(lock["data_version"]),
    }
    require_equal("qualification target", target, expected_target)

    locators = spec.get("locators")
    if not isinstance(locators, list) or not locators:
        raise ValueError("qualification spec requires a non-empty locators array")

    evidence_ids: set[str] = set()
    for locator in locators:
        if not isinstance(locator, dict):
            raise ValueError("every source locator must be an object")
        evidence_id = locator.get("id")
        if not isinstance(evidence_id, str) or not evidence_id:
            raise ValueError("every source locator requires a non-empty id")
        if evidence_id in evidence_ids:
            raise ValueError(f"duplicate source locator id: {evidence_id}")
        evidence_ids.add(evidence_id)

    conn = sqlite3.connect(atlas_path)
    try:
        meta = atlas_meta(conn)
        require_equal(
            "Atlas source archive",
            meta.get("source_archive_sha256"),
            str(lock["source"]["archive_sha256"]),
        )
        require_equal("Atlas Minecraft version", meta.get("minecraft_version"), str(lock["minecraft"]))
        require_equal("Atlas protocol version", meta.get("protocol_version"), str(lock["protocol"]))
        require_equal("Atlas data/world version", meta.get("world_version"), str(lock["data_version"]))
        require_equal("Atlas schema", meta.get("schema_version"), str(lock["atlas"]["schema"]))
        require_equal("Atlas version", meta.get("atlas_version"), str(lock["atlas"]["version"]))
        require_equal(
            "Atlas fingerprint algorithm",
            meta.get("fingerprint_algorithm"),
            str(lock["atlas"]["fingerprint_algorithm"]),
        )

        evidence: list[dict[str, Any]] = []
        for locator in locators:
            resolved = resolve_locator(conn, locator)
            evidence.append(
                {
                    "id": locator["id"],
                    "role": locator.get("role"),
                    "classification": locator.get("classification"),
                    "surface": resolved,
                }
            )
    finally:
        conn.close()

    result: dict[str, Any] = {
        "schema": SCHEMA_VERSION,
        "target": expected_target,
        "source": {
            "archive_sha256": str(lock["source"]["archive_sha256"]),
            "java_files": int(lock["source"]["java_files"]),
        },
        "atlas": {
            "schema": int(lock["atlas"]["schema"]),
            "version": str(lock["atlas"]["version"]),
            "fingerprint_algorithm": str(lock["atlas"]["fingerprint_algorithm"]),
        },
        "spec_sha256": sha256_bytes(canonical_json_bytes(spec)),
        "evidence": evidence,
    }
    result["qualification_digest"] = sha256_bytes(canonical_json_bytes(result))
    return result


def rendered(value: dict[str, Any]) -> str:
    return json.dumps(value, indent=2, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--lock", default="vanilla/vanilla.lock.toml")
    parser.add_argument("--spec", default="vanilla/state-data/source-qualification-spec.json")
    parser.add_argument("--atlas", default=".crucible/vanilla/atlas.sqlite")
    parser.add_argument("--output", required=True)
    parser.add_argument(
        "--verify",
        action="store_true",
        help="fail unless the existing output is byte-identical to regenerated qualification",
    )
    args = parser.parse_args()

    try:
        value = qualify(Path(args.lock), Path(args.spec), Path(args.atlas))
        text = rendered(value)
        output = Path(args.output)
        if args.verify:
            if not output.is_file():
                raise ValueError(f"qualification artifact does not exist: {output}")
            if output.read_text(encoding="utf-8") != text:
                raise ValueError("qualification artifact differs from the pinned Atlas/source evidence")
            print(f"verified state source qualification: {value['qualification_digest']}")
            return 0
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(text, encoding="utf-8")
        print(f"wrote state source qualification: {value['qualification_digest']}")
        return 0
    except (KeyError, OSError, sqlite3.Error, ValueError, json.JSONDecodeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
