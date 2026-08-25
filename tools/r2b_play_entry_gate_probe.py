#!/usr/bin/env python3
"""Emit source-free Atlas metadata needed to harden the finite R2B Play-entry source gate.

This probe deliberately reads only the local Atlas database. It never opens the official source
archive and never emits source bodies. Its purpose is to resolve the last delegate seams before one
bounded source-rich review is prepared: inventory synchronization and packet codec roots whose wire
law may live in constructors/writers rather than the static codec declaration itself.
"""
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
KIND = "r2b-play-entry-gate-atlas-probe"
DEFAULT_DB = Path(".crucible/vanilla/atlas.sqlite")
EXPECTED_META = {
    "atlas_version": "0.1.1",
    "fingerprint_algorithm": "java-token-v2-literal-sensitive",
    "minecraft_version": "26.2",
    "protocol_version": "776",
    "world_version": "4903",
    "source_archive_sha256": "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750",
}

# These identities were already resolved by the first/follow-up Play-entry reviews. The new probe
# traces their calls without treating any call edge as semantic admission.
ANCHORS = (
    "net.minecraft.server.level.ServerPlayer#initMenu(final AbstractContainerMenu container)",
    "net.minecraft.world.inventory.AbstractContainerMenu#sendAllDataToRemote()",
)

# setSynchronizer is intentionally discovered by name because the probe's job is to report the exact
# signature before the source-review frontier is frozen.
SYMBOL_NAMES = (
    "setSynchronizer",
    "sendAllDataToRemote",
    "sendInitialData",
    "sendSlotChange",
    "sendCarriedChange",
    "sendDataChange",
)

# Packet classes that occur in the selected fresh/default Play-entry route and whose codec roots or
# writers must be checked for delegated payload law. The output lists exact Atlas signatures only.
PACKET_TYPES = (
    "net.minecraft.network.protocol.game.ClientboundLoginPacket",
    "net.minecraft.network.protocol.game.ClientboundChangeDifficultyPacket",
    "net.minecraft.network.protocol.game.ClientboundPlayerAbilitiesPacket",
    "net.minecraft.network.protocol.game.ClientboundSetHeldSlotPacket",
    "net.minecraft.network.protocol.game.ClientboundUpdateRecipesPacket",
    "net.minecraft.network.protocol.game.ClientboundEntityEventPacket",
    "net.minecraft.network.protocol.game.ClientboundCommandsPacket",
    "net.minecraft.network.protocol.game.ClientboundRecipeBookSettingsPacket",
    "net.minecraft.network.protocol.game.ClientboundRecipeBookAddPacket",
    "net.minecraft.network.protocol.game.ClientboundInitializeBorderPacket",
    "net.minecraft.network.protocol.game.ClientboundSetDefaultSpawnPositionPacket",
    "net.minecraft.network.protocol.game.ClientboundGameEventPacket",
    "net.minecraft.network.protocol.game.ClientboundTickingStatePacket",
    "net.minecraft.network.protocol.game.ClientboundTickingStepPacket",
    "net.minecraft.network.protocol.game.ClientboundContainerSetContentPacket",
    "net.minecraft.network.protocol.game.ClientboundPlayerInfoUpdatePacket",
    "net.minecraft.network.protocol.game.ClientboundPlayerPositionPacket",
    "net.minecraft.network.protocol.game.ClientboundServerDataPacket",
)


class ProbeError(RuntimeError):
    """Fail-closed source-free probe error."""


def _meta(conn: sqlite3.Connection) -> dict[str, str]:
    return {str(key): str(value) for key, value in conn.execute("SELECT key,value FROM meta")}


def _verify_meta(conn: sqlite3.Connection) -> dict[str, str]:
    meta = _meta(conn)
    mismatches = {
        key: {"expected": expected, "actual": meta.get(key)}
        for key, expected in EXPECTED_META.items()
        if meta.get(key) != expected
    }
    if mismatches:
        raise ProbeError(f"Atlas source pin mismatch: {json.dumps(mismatches, sort_keys=True)}")
    return meta


def _method_row(conn: sqlite3.Connection, identity: str) -> sqlite3.Row:
    if "#" not in identity:
        raise ProbeError(f"invalid method identity: {identity}")
    qname, signature = identity.split("#", 1)
    rows = conn.execute(
        """SELECT m.id,t.qualified_name,m.signature
           FROM methods m JOIN types t ON t.id=m.type_id
           WHERE t.qualified_name=? AND m.signature=?""",
        (qname, signature),
    ).fetchall()
    if len(rows) != 1:
        raise ProbeError(f"anchor must resolve exactly once: {identity}; got {len(rows)}")
    return rows[0]


def _calls(conn: sqlite3.Connection, method_id: int) -> list[dict[str, object]]:
    rows = conn.execute(
        """SELECT c.owner_text,c.callee_name,c.arg_count,c.line,c.resolution,
                  t.qualified_name AS resolved_type,m.signature AS resolved_signature
           FROM method_calls c
           LEFT JOIN methods m ON m.id=c.resolved_method_id
           LEFT JOIN types t ON t.id=m.type_id
           WHERE c.caller_method_id=? ORDER BY c.line,c.id""",
        (method_id,),
    ).fetchall()
    return [
        {
            "owner_text": row["owner_text"],
            "callee_name": row["callee_name"],
            "arg_count": int(row["arg_count"]),
            "line": int(row["line"]),
            "resolution": row["resolution"],
            "resolved_identity": (
                f"{row['resolved_type']}#{row['resolved_signature']}"
                if row["resolved_type"] is not None
                else None
            ),
        }
        for row in rows
    ]


def _symbol_matches(conn: sqlite3.Connection, name: str) -> list[str]:
    rows = conn.execute(
        """SELECT t.qualified_name,m.signature
           FROM methods m JOIN types t ON t.id=m.type_id
           WHERE m.name=? ORDER BY t.qualified_name,m.start_line""",
        (name,),
    ).fetchall()
    return [f"{row['qualified_name']}#{row['signature']}" for row in rows]


def _type_methods(conn: sqlite3.Connection, qname: str) -> list[str]:
    rows = conn.execute(
        """SELECT t.qualified_name,m.signature
           FROM methods m JOIN types t ON t.id=m.type_id
           WHERE t.qualified_name=? ORDER BY m.start_line""",
        (qname,),
    ).fetchall()
    return [f"{row['qualified_name']}#{row['signature']}" for row in rows]


def build_report(conn: sqlite3.Connection) -> dict[str, object]:
    meta = _verify_meta(conn)
    anchors: list[dict[str, object]] = []
    for identity in ANCHORS:
        row = _method_row(conn, identity)
        anchors.append({"identity": identity, "calls": _calls(conn, int(row["id"]))})
    return {
        "schema": SCHEMA,
        "kind": KIND,
        "contains_official_source_text": False,
        "source": {key: meta[key] for key in EXPECTED_META},
        "anchors": anchors,
        "symbol_matches": {name: _symbol_matches(conn, name) for name in SYMBOL_NAMES},
        "packet_type_methods": {qname: _type_methods(conn, qname) for qname in PACKET_TYPES},
        "note": (
            "Structural discovery only. Exact signatures and call edges are review leads, not "
            "semantic admission. Freeze the final source-rich frontier only after inspecting this report."
        ),
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r2b-play-entry-gate-probe")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--output", type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    conn = vanilla_atlas.connect_db(args.db)
    try:
        report = build_report(conn)
    except ProbeError as error:
        print(f"R2B Play-entry gate probe error: {error}", file=sys.stderr)
        return 2
    finally:
        conn.close()
    raw = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output is None:
        print(raw, end="")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(raw, encoding="utf-8")
        print(f"r2b_play_entry_gate_probe={args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
