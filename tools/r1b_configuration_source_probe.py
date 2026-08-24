#!/usr/bin/env python3
"""Emit the narrow local source evidence needed to finish R1B Configuration admission.

The probe never treats the generated Atlas database as committed evidence. It first binds both the
local source archive and disposable Atlas database to `vanilla.lock.toml`, verifies every explicit
frontier root resolves, prints fingerprint-pinned record templates for uniquely identified
declarations, and extracts only PlayerList.placeNewPlayer from the pinned source archive so the
minimum Play bootstrap can be reviewed without another broad source dump.

An optional machine-readable bundle can be emitted outside the repository. The bundle is deliberately
marked ephemeral because it contains a narrow official-source excerpt and must never be committed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
import zipfile
from pathlib import Path
from typing import Callable, Mapping, Sequence

try:
    from . import vanilla_atlas
except ImportError:  # Direct `python3 tools/...` execution.
    import vanilla_atlas  # type: ignore[no-redef]


DEFAULT_FRONTIER = Path("vanilla/frontiers/r1b-configuration-selected.json")
DEFAULT_DB = Path(".crucible/vanilla/atlas.sqlite")
DEFAULT_SOURCE = Path.home() / "Documents/mc-source/mc-src.zip"
DEFAULT_LOCK = Path("vanilla/vanilla.lock.toml")
PLAYER_LIST_PATH = "src/net/minecraft/server/players/PlayerList.java"
BUNDLE_KIND = "r1b-configuration-source-admission-bundle"

# Stable intended record IDs. A candidate is emitted only when Atlas resolves its query to exactly
# one declaration; ambiguous candidates are printed for refinement rather than guessed.
CANDIDATES: tuple[tuple[str, str], ...] = (
    ("VAR-NET-R1B-CONFIG-REGISTRATION-001", "net.minecraft.network.protocol.configuration.ConfigurationProtocols#<clinit>()"),
    ("VAR-NET-R1B-FINISH-CLIENTBOUND-CODEC-001", "net.minecraft.network.protocol.configuration.ClientboundFinishConfigurationPacket#<clinit>()"),
    ("VAR-NET-R1B-FINISH-SERVERBOUND-CODEC-001", "net.minecraft.network.protocol.configuration.ServerboundFinishConfigurationPacket#<clinit>()"),
    ("VAR-NET-R1B-REGISTRY-DATA-CODEC-001", "net.minecraft.network.protocol.configuration.ClientboundRegistryDataPacket#<clinit>()"),
    ("VAR-NET-R1B-SELECT-KNOWN-CB-CODEC-001", "net.minecraft.network.protocol.configuration.ClientboundSelectKnownPacks#<clinit>()"),
    ("VAR-NET-R1B-SELECT-KNOWN-SB-CODEC-001", "net.minecraft.network.protocol.configuration.ServerboundSelectKnownPacks#<clinit>()"),
    ("VAR-NET-R1B-ENABLED-FEATURES-CODEC-001", "net.minecraft.network.protocol.configuration.ClientboundUpdateEnabledFeaturesPacket#<clinit>()"),
    ("VAR-NET-R1B-UPDATE-TAGS-CODEC-001", "net.minecraft.network.protocol.common.ClientboundUpdateTagsPacket#<clinit>()"),
    ("VAR-NET-R1B-CB-CUSTOM-PAYLOAD-CODEC-001", "net.minecraft.network.protocol.common.ClientboundCustomPayloadPacket#<clinit>()"),
    ("VAR-NET-R1B-SB-CUSTOM-PAYLOAD-CODEC-001", "net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket#<clinit>()"),
    ("VAR-NET-R1B-CLIENT-INFO-PACKET-CODEC-001", "net.minecraft.network.protocol.common.ServerboundClientInformationPacket#<clinit>()"),
    ("VAR-NET-R1B-BRAND-PAYLOAD-CODEC-001", "net.minecraft.network.protocol.common.custom.BrandPayload#<clinit>()"),
    ("VAR-NET-R1B-KNOWN-PACK-CODEC-001", "net.minecraft.server.packs.repository.KnownPack#<clinit>()"),
    ("VAR-NET-R1B-CONFIG-START-001", "net.minecraft.server.network.ServerConfigurationPacketListenerImpl#startConfiguration"),
    ("VAR-NET-R1B-CLIENT-INFO-HANDLER-001", "net.minecraft.server.network.ServerConfigurationPacketListenerImpl#handleClientInformation"),
    ("VAR-NET-R1B-KNOWN-PACK-HANDLER-001", "net.minecraft.server.network.ServerConfigurationPacketListenerImpl#handleSelectKnownPacks"),
    ("VAR-NET-R1B-CONFIG-FINISH-HANDLER-001", "net.minecraft.server.network.ServerConfigurationPacketListenerImpl#handleConfigurationFinished"),
    ("VAR-NET-R1B-CUSTOM-PAYLOAD-HANDLER-001", "net.minecraft.server.network.ServerCommonPacketListenerImpl#handleCustomPayload"),
    ("VAR-NET-R1B-REGISTRY-SYNC-START-001", "net.minecraft.server.network.config.SynchronizeRegistriesTask#start"),
    ("VAR-NET-R1B-REGISTRY-SYNC-SEND-001", "net.minecraft.server.network.config.SynchronizeRegistriesTask#sendRegistries"),
    ("VAR-NET-R1B-REGISTRY-SYNC-RESPONSE-001", "net.minecraft.server.network.config.SynchronizeRegistriesTask#handleResponse"),
    ("VAR-NET-R1B-PREPARE-SPAWN-START-001", "net.minecraft.server.network.config.PrepareSpawnTask#start"),
    ("VAR-NET-R1B-PREPARE-SPAWN-TICK-001", "net.minecraft.server.network.config.PrepareSpawnTask#tick"),
    ("VAR-NET-R1B-JOIN-WORLD-START-001", "net.minecraft.server.network.config.JoinWorldTask#start"),
    ("VAR-NET-R1B-PLACE-NEW-PLAYER-001", "net.minecraft.server.players.PlayerList#placeNewPlayer"),
)


class ProbeError(RuntimeError):
    """Fail-closed local admission-probe error."""


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _required_table(value: object, label: str) -> Mapping[str, object]:
    if not isinstance(value, dict):
        raise ProbeError(f"{label} must be a TOML table")
    return value


def _required_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ProbeError(f"{label} must be a non-empty string")
    return value


def require_pinned_source(
    conn: object,
    source_archive: Path,
    lock_path: Path,
) -> str:
    """Bind the source archive and Atlas metadata to the repository's canonical source lock."""
    try:
        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as error:
        raise ProbeError(f"invalid vanilla lock TOML: {error}") from error

    source_lock = _required_table(lock.get("source"), "vanilla lock source")
    atlas_lock = _required_table(lock.get("atlas"), "vanilla lock atlas")
    expected_sha = _required_string(source_lock.get("archive_sha256"), "source.archive_sha256")
    expected_minecraft = _required_string(lock.get("minecraft"), "minecraft")
    expected_atlas_version = _required_string(atlas_lock.get("version"), "atlas.version")
    expected_fingerprint = _required_string(
        atlas_lock.get("fingerprint_algorithm"), "atlas.fingerprint_algorithm"
    )
    protocol = lock.get("protocol")
    if type(protocol) is not int:
        raise ProbeError("protocol must be an integer")

    actual_sha = _sha256_file(source_archive)
    if actual_sha != expected_sha:
        raise ProbeError(
            "source archive SHA-256 does not match vanilla lock: "
            f"expected {expected_sha}, got {actual_sha}"
        )

    meta = dict(conn.execute("SELECT key,value FROM meta"))  # type: ignore[attr-defined]
    expected_meta = {
        "source_archive_sha256": expected_sha,
        "minecraft_version": expected_minecraft,
        "protocol_version": str(protocol),
        "atlas_version": expected_atlas_version,
        "fingerprint_algorithm": expected_fingerprint,
    }
    mismatches = [
        f"{key}: expected {expected!r}, got {meta.get(key)!r}"
        for key, expected in expected_meta.items()
        if meta.get(key) != expected
    ]
    if mismatches:
        raise ProbeError("Atlas/source lock mismatch: " + "; ".join(mismatches))
    return expected_sha


def require_frontier_roots(
    conn: object,
    root_queries: Sequence[str],
    resolver: Callable[[object, str], Sequence[object]],
) -> list[tuple[str, Sequence[object]]]:
    """Resolve every explicit root independently; one zero-match root fails the whole probe."""
    resolved: list[tuple[str, Sequence[object]]] = []
    for query in root_queries:
        rows = resolver(conn, query)
        if not rows:
            raise ProbeError(f"explicit frontier root resolved zero Atlas methods: {query}")
        resolved.append((query, rows))
    return resolved


def extract_java_method(
    source: str,
    method_name: str,
    *,
    owner_simple_name: str | None = None,
) -> str:
    """Extract exactly one declared Java method using the Atlas structural lexer/parser.

    Textual `methodName(` searching is intentionally insufficient here: a call site preceding the
    declaration could otherwise be mistaken for the evidence root. Reusing Atlas's declaration
    parser means comments, literals, nested blocks and ordinary invocations are already excluded by
    the same structural rules which produced the fingerprint database.
    """
    tokens = vanilla_atlas.tokenize_java(source)
    parens = vanilla_atlas.matching_pairs(tokens, "(", ")")
    braces = vanilla_atlas.matching_pairs(tokens, "{", "}")
    package, _imports = vanilla_atlas.package_and_imports(tokens)
    types = vanilla_atlas.extract_types(tokens, package, braces)

    matches: list[tuple[vanilla_atlas.MethodDecl, str]] = []
    for typ in types:
        if owner_simple_name is not None and typ.simple_name != owner_simple_name:
            continue
        methods, _fields = vanilla_atlas.extract_members(tokens, typ, parens, braces)
        for method in methods:
            if method.name != method_name or method.body_open is None or method.body_close is None:
                continue
            start_token = tokens[method.start_token]
            end_token = tokens[method.body_close]
            line_start = source.rfind("\n", 0, start_token.start) + 1
            matches.append((method, source[line_start : end_token.end]))

    if not matches:
        owner = f" on {owner_simple_name}" if owner_simple_name is not None else ""
        raise ProbeError(f"declared method not found in source: {method_name}{owner}")
    if len(matches) != 1:
        signatures = ", ".join(method.signature for method, _body in matches)
        owner = f" on {owner_simple_name}" if owner_simple_name is not None else ""
        raise ProbeError(
            f"declared method is ambiguous in source: {method_name}{owner}: {signatures}"
        )
    return matches[0][1]


def record_template(conn: object, row: object, var_id: str) -> dict[str, object]:
    method_id = int(row["id"])  # type: ignore[index]
    hashes = conn.execute(  # type: ignore[attr-defined]
        "SELECT normalized_sha256,body_sha256 FROM methods WHERE id=?", (method_id,)
    ).fetchone()
    meta = dict(conn.execute("SELECT key,value FROM meta"))  # type: ignore[attr-defined]
    hazards = [
        item[0]
        for item in conn.execute(  # type: ignore[attr-defined]
            "SELECT DISTINCT kind FROM hazards WHERE method_id=? ORDER BY kind", (method_id,)
        )
    ]
    classifications = [
        item[0]
        for item in conn.execute(  # type: ignore[attr-defined]
            "SELECT label FROM classifications WHERE method_id=? AND source='heuristic' ORDER BY label",
            (method_id,),
        )
    ]
    return {
        "schema": 1,
        "id": var_id,
        "status": "INDEXED",
        "source": {
            "type": row["qualified_name"],  # type: ignore[index]
            "signature": row["signature"],  # type: ignore[index]
            "fingerprint_algorithm": meta.get(
                "fingerprint_algorithm", vanilla_atlas.FINGERPRINT_ALGORITHM
            ),
            "normalized_sha256": hashes[0],
            "body_sha256": hashes[1],
        },
        "classifications": classifications,
        "hazards_reviewed": [],
        "semantic_rules": [],
        "evidence": [],
        "notes": [],
        "atlas_observed_hazards": hazards,
    }


def _source_identity(row: object) -> str:
    return f"{row['qualified_name']}#{row['signature']}"  # type: ignore[index]


def write_admission_bundle(path: Path, bundle: Mapping[str, object]) -> None:
    """Write one deterministic local handoff bundle after all fail-closed admission checks pass."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(bundle, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def run(
    db: Path,
    source_archive: Path,
    frontier_path: Path,
    lock_path: Path,
    bundle_output: Path | None = None,
) -> int:
    frontier = json.loads(frontier_path.read_text(encoding="utf-8"))
    roots = [str(query) for query in frontier.get("root_queries", [])]
    if not roots:
        raise ProbeError("frontier has no explicit root_queries")

    conn = vanilla_atlas.connect_db(db)
    source_sha = require_pinned_source(conn, source_archive, lock_path)
    resolved = require_frontier_roots(conn, roots, vanilla_atlas.resolve_methods)

    print("===== R1B SOURCE IDENTITY =====")
    print(f"source_archive_sha256={source_sha}")

    print("\n===== R1B EXPLICIT ROOT RESOLUTION =====")
    print(f"roots={len(resolved)}")
    root_evidence: list[dict[str, object]] = []
    for query, rows in resolved:
        identities = [_source_identity(row) for row in rows]
        root_evidence.append({"query": query, "matches": identities})
        print(f"ROOT {len(rows):3d} {query}")
        for identity in identities:
            print(f"  {identity}")

    print("\n===== R1B VAR RECORD TEMPLATES =====")
    emitted = 0
    ambiguous = 0
    candidate_evidence: list[dict[str, object]] = []
    for var_id, query in CANDIDATES:
        rows = vanilla_atlas.resolve_methods(conn, query)
        evidence: dict[str, object] = {
            "var_id": var_id,
            "query": query,
            "match_count": len(rows),
        }
        print(f"\n--- {var_id} :: {query} :: matches={len(rows)} ---")
        if len(rows) == 1:
            template = record_template(conn, rows[0], var_id)
            evidence["record_template"] = template
            print(json.dumps(template, indent=2, sort_keys=True))
            emitted += 1
        else:
            identities = [_source_identity(row) for row in rows[:20]]
            evidence["candidates"] = identities
            ambiguous += 1
            for identity in identities:
                print(f"CANDIDATE {identity}")
        candidate_evidence.append(evidence)

    print("\n===== PLAYERLIST.placeNewPlayer EXACT SOURCE =====")
    with zipfile.ZipFile(source_archive) as archive:
        if PLAYER_LIST_PATH not in archive.namelist():
            raise ProbeError(f"source member missing: {PLAYER_LIST_PATH}")
        source = archive.read(PLAYER_LIST_PATH).decode("utf-8", errors="strict")
    place_new_player = extract_java_method(
        source,
        "placeNewPlayer",
        owner_simple_name="PlayerList",
    )
    print(place_new_player)

    summary = {
        "roots_ok": len(resolved),
        "record_templates_emitted": emitted,
        "record_templates_needing_refinement": ambiguous,
    }
    if bundle_output is not None:
        bundle: dict[str, object] = {
            "schema": 1,
            "kind": BUNDLE_KIND,
            "commit_policy": "EPHEMERAL_DO_NOT_COMMIT",
            "contains_official_source_text": True,
            "source_archive_sha256": source_sha,
            "frontier": str(frontier_path),
            "frontier_roots": root_evidence,
            "var_candidates": candidate_evidence,
            "play_bootstrap_source": {
                "path": PLAYER_LIST_PATH,
                "owner": "PlayerList",
                "method": "placeNewPlayer",
                "source": place_new_player,
            },
            "summary": summary,
        }
        write_admission_bundle(bundle_output, bundle)
        print(f"\nlocal_admission_bundle={bundle_output}")
        print("bundle_commit_policy=EPHEMERAL_DO_NOT_COMMIT")

    print("\n===== R1B PROBE RESULT =====")
    print(f"roots_ok={len(resolved)}")
    print(f"record_templates_emitted={emitted}")
    print(f"record_templates_needing_refinement={ambiguous}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-configuration-source-probe")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--frontier", type=Path, default=DEFAULT_FRONTIER)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument(
        "--bundle-output",
        type=Path,
        help="write an ephemeral JSON handoff bundle containing the narrow source witness",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return run(
            args.db,
            args.source,
            args.frontier,
            args.lock,
            args.bundle_output,
        )
    except (OSError, json.JSONDecodeError, zipfile.BadZipFile, ProbeError) as error:
        print(f"R1B Configuration source probe error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
