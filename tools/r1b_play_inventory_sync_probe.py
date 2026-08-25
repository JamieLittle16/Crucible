#!/usr/bin/env python3
"""Source-free Atlas probe for the last R1B inventory synchronization edge."""
from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from pathlib import Path
from typing import Any, Sequence

try:
    from . import vanilla_atlas
except ImportError:
    import vanilla_atlas  # type: ignore[no-redef]

SCHEMA = 1
KIND = "r1b-play-inventory-sync-atlas-probe"
EXPECTED_SOURCE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
EXPECTED_FINGERPRINT = "java-token-v2-literal-sensitive"
EXPECTED_ATLAS_VERSION = "0.1.1"

ROOTS = (
    ("net.minecraft.world.inventory.AbstractContainerMenu", "addSlotListener", 1),
    ("net.minecraft.world.inventory.AbstractContainerMenu", "setSynchronizer", 1),
    ("net.minecraft.world.inventory.AbstractContainerMenu", "sendAllDataToRemote", 0),
)
SEARCH_NAMES = (
    "sendInitialData",
    "sendSlotChange",
    "sendCarriedChange",
    "sendDataChange",
)


class ProbeError(ValueError):
    """Fail-closed structural probe error."""


def _meta(conn: sqlite3.Connection) -> dict[str, str]:
    return {str(key): str(value) for key, value in conn.execute("SELECT key,value FROM meta")}


def _identity(row: sqlite3.Row) -> str:
    return f"{row['qualified_name']}#{row['signature']}"


def _method_rows(
    conn: sqlite3.Connection, type_name: str, method_name: str, param_count: int
) -> list[sqlite3.Row]:
    return conn.execute(
        """SELECT m.id,t.qualified_name,m.name,m.signature,m.param_count,m.start_line,m.end_line
           FROM methods m JOIN types t ON t.id=m.type_id
           WHERE t.qualified_name=? AND m.name=? AND m.param_count=? ORDER BY m.start_line""",
        (type_name, method_name, param_count),
    ).fetchall()


def _calls(conn: sqlite3.Connection, method_id: int) -> list[dict[str, Any]]:
    rows = conn.execute(
        """SELECT c.line,c.owner_text,c.callee_name,c.arg_count,c.resolution,
                  rt.qualified_name AS resolved_type,rm.signature AS resolved_signature
           FROM method_calls c
           LEFT JOIN methods rm ON rm.id=c.resolved_method_id
           LEFT JOIN types rt ON rt.id=rm.type_id
           WHERE c.caller_method_id=? ORDER BY c.line,c.id""",
        (method_id,),
    ).fetchall()
    result = []
    for row in rows:
        resolved = None
        if row["resolved_type"] is not None:
            resolved = f"{row['resolved_type']}#{row['resolved_signature']}"
        result.append(
            {
                "line": int(row["line"]),
                "owner_text": row["owner_text"],
                "callee_name": row["callee_name"],
                "arg_count": int(row["arg_count"]),
                "resolution": row["resolution"],
                "resolved_identity": resolved,
            }
        )
    return result


def probe(db: Path) -> dict[str, Any]:
    conn = vanilla_atlas.connect_db(db)
    try:
        meta = _meta(conn)
        if meta.get("source_archive_sha256") != EXPECTED_SOURCE_SHA256:
            raise ProbeError("Atlas source archive SHA-256 mismatch")
        if meta.get("fingerprint_algorithm") != EXPECTED_FINGERPRINT:
            raise ProbeError("Atlas fingerprint algorithm mismatch")
        if meta.get("atlas_version") != EXPECTED_ATLAS_VERSION:
            raise ProbeError("Atlas version mismatch")

        roots = []
        for type_name, method_name, param_count in ROOTS:
            rows = _method_rows(conn, type_name, method_name, param_count)
            roots.append(
                {
                    "selector": {
                        "type_name": type_name,
                        "method_name": method_name,
                        "param_count": param_count,
                    },
                    "matches": [
                        {"identity": _identity(row), "calls": _calls(conn, int(row["id"]))}
                        for row in rows
                    ],
                }
            )

        placeholders = ",".join("?" for _ in SEARCH_NAMES)
        searched = conn.execute(
            f"""SELECT m.id,t.qualified_name,m.name,m.signature,m.param_count,m.start_line
                FROM methods m JOIN types t ON t.id=m.type_id
                WHERE m.name IN ({placeholders})
                ORDER BY t.qualified_name,m.start_line""",
            SEARCH_NAMES,
        ).fetchall()
        named = [
            {
                "identity": _identity(row),
                "name": row["name"],
                "param_count": int(row["param_count"]),
                "calls": _calls(conn, int(row["id"])),
            }
            for row in searched
        ]
        return {
            "schema": SCHEMA,
            "kind": KIND,
            "contains_official_source_text": False,
            "source": {
                "minecraft_version": meta.get("minecraft_version"),
                "protocol_version": int(meta.get("protocol_version", "0")),
                "world_version": int(meta.get("world_version", "0")),
                "source_archive_sha256": meta.get("source_archive_sha256"),
                "fingerprint_algorithm": meta.get("fingerprint_algorithm"),
                "atlas_version": meta.get("atlas_version"),
            },
            "roots": roots,
            "named_synchronizer_methods": named,
            "note": (
                "Structural discovery only. Exact source-body review is required before any newly resolved "
                "inventory synchronization method enters GATE-NET-PLAY-ENTRY-26_2-001."
            ),
        }
    finally:
        conn.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-play-inventory-sync-probe")
    parser.add_argument("--db", type=Path, default=vanilla_atlas.default_db())
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        result = probe(args.db)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    except (OSError, sqlite3.Error, ProbeError) as error:
        print(f"R1B inventory-sync probe error: {error}", file=sys.stderr)
        return 2
    print(f"inventory_sync_probe={args.output}")
    print(f"root_selectors={len(result['roots'])}")
    print(f"named_synchronizer_methods={len(result['named_synchronizer_methods'])}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
