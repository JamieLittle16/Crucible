#!/usr/bin/env python3
"""Index source-backed Java class initialization as reviewable Atlas evidence.

Vanilla Atlas intentionally models source methods conservatively. Modern Minecraft protocol law,
however, also lives in static field initializers: packet registrations, codecs and packet-type
constants are commonly declared there. This pass augments a generated Atlas database with one
synthetic ``<clinit>()`` method per type that has static initialization.

The synthetic node is evidence tooling only. It is the ordered token/raw-source projection of the
actual static field initializers and static initializer blocks for that source type, mirroring the
single class-initialization unit Java exposes at runtime. No Mojang source body is written to Git.
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

import vanilla_atlas as atlas

DECLARATION_INDEX_VERSION = "0.1.0"
SYNTHETIC_NAME = "<clinit>"
SYNTHETIC_SIGNATURE = "<clinit>()"
SYNTHETIC_MODIFIERS = "static synthetic atlas-declaration-v1"


class DeclarationIndexError(RuntimeError):
    """Raised when declaration evidence cannot be bound safely to the Atlas source identity."""


@dataclass(frozen=True, slots=True)
class StaticInitialization:
    """Canonical source projection for one type's static initialization."""

    start_line: int
    end_line: int
    normalized_sha256: str
    body_sha256: str
    tokens: tuple[atlas.Token, ...]


def _token_texts(tokens: Sequence[atlas.Token]) -> set[str]:
    return {token.text for token in tokens}


def _is_static_field(tokens: Sequence[atlas.Token]) -> bool:
    """Return whether a semicolon-terminated member is an initialized static field."""
    texts = [token.text for token in tokens]
    try:
        static_index = texts.index("static")
        equals_index = texts.index("=")
    except ValueError:
        return False
    # `static` must be part of the declaration, not appear inside an initializer expression.
    return static_index < equals_index


def _is_static_block_prefix(tokens: Sequence[atlas.Token]) -> bool:
    texts = [token.text for token in tokens]
    if "static" not in texts or "=" in texts or "(" in texts:
        return False
    return not any(kind in texts for kind in atlas.TYPE_DECL_KINDS)


def _static_spans(
    text: str,
    tokens: Sequence[atlas.Token],
    typ: atlas.TypeDecl,
    braces: dict[int, int],
) -> tuple[tuple[atlas.Token, ...], tuple[str, ...]]:
    """Extract top-level static initialization from one Java type in source order."""
    token_spans: list[tuple[atlas.Token, ...]] = []
    raw_spans: list[str] = []
    member_start = typ.body_open + 1
    index = member_start

    while index < typ.body_close:
        token = tokens[index]

        if token.text == ";":
            member = tuple(tokens[member_start : index + 1])
            if member and _is_static_field(member):
                token_spans.append(member)
                raw_spans.append(text[member[0].start : member[-1].end])
            member_start = index + 1
            index += 1
            continue

        if token.text != "{" or index not in braces:
            index += 1
            continue

        close = braces[index]
        if close > typ.body_close:
            raise DeclarationIndexError(
                f"initializer brace escapes type {typ.qualified_name} at line {token.line}"
            )

        prefix = tuple(tokens[member_start:index])
        prefix_texts = _token_texts(prefix)

        # A field initializer may itself contain an array/lambda/anonymous-class body. Keep the
        # original member start and jump over that nested body; the terminating semicolon will
        # capture the complete declaration.
        if "=" in prefix_texts:
            index = close + 1
            continue

        if _is_static_block_prefix(prefix):
            block = tuple(tokens[member_start : close + 1])
            if block:
                token_spans.append(block)
                raw_spans.append(text[block[0].start : block[-1].end])
            member_start = close + 1
            index = close + 1
            continue

        # Method/constructor/nested-type/instance-initializer body: it contributes no class-static
        # initialization at this level. Nested types are independently indexed by Atlas.
        member_start = close + 1
        index = close + 1

    flattened = tuple(token for span in token_spans for token in span)
    return flattened, tuple(raw_spans)


def static_initialization(
    text: str,
    tokens: Sequence[atlas.Token],
    typ: atlas.TypeDecl,
    braces: dict[int, int],
) -> StaticInitialization | None:
    """Build a stable fingerprint for one type's source-level class initialization."""
    flattened, raw_spans = _static_spans(text, tokens, typ, braces)
    if not flattened:
        return None
    raw = b"\x00".join(span.encode("utf-8") for span in raw_spans)
    return StaticInitialization(
        start_line=flattened[0].line,
        end_line=flattened[-1].line,
        normalized_sha256=atlas.normalized_fingerprint(flattened),
        body_sha256=atlas.sha256_bytes(raw),
        tokens=flattened,
    )


def _verify_source_identity(
    conn: sqlite3.Connection,
    *,
    archive_sha256: str,
    version: dict[str, object],
) -> None:
    meta = dict(conn.execute("SELECT key,value FROM meta"))
    expected = {
        "source_archive_sha256": archive_sha256,
        "minecraft_version": str(version.get("id", "")),
        "protocol_version": str(version.get("protocol_version", "")),
        "world_version": str(version.get("world_version", "")),
    }
    mismatches = [
        f"{key}: db={meta.get(key)!r} source={value!r}"
        for key, value in expected.items()
        if str(meta.get(key, "")) != value
    ]
    if mismatches:
        raise DeclarationIndexError(
            "source archive does not match Atlas database identity: " + "; ".join(mismatches)
        )


def _delete_existing(conn: sqlite3.Connection) -> None:
    rows = conn.execute(
        "SELECT id FROM methods WHERE name=? AND signature=? AND modifiers=?",
        (SYNTHETIC_NAME, SYNTHETIC_SIGNATURE, SYNTHETIC_MODIFIERS),
    ).fetchall()
    ids = [int(row[0]) for row in rows]
    for offset in range(0, len(ids), 500):
        chunk = ids[offset : offset + 500]
        placeholders = ",".join("?" for _ in chunk)
        conn.execute(
            f"UPDATE method_calls SET resolved_method_id=NULL,resolution=NULL "
            f"WHERE resolved_method_id IN ({placeholders})",
            chunk,
        )
        for table in ("method_calls", "field_accesses", "hazards", "classifications", "tracking"):
            column = "caller_method_id" if table == "method_calls" else "method_id"
            conn.execute(f"DELETE FROM {table} WHERE {column} IN ({placeholders})", chunk)
        conn.execute(
            f"DELETE FROM semantic_edges WHERE method_id IN ({placeholders})",
            chunk,
        )
        conn.execute(f"DELETE FROM methods WHERE id IN ({placeholders})", chunk)


def _hazards(package: str, initialization: StaticInitialization) -> tuple[str, ...]:
    texts = _token_texts(initialization.tokens)
    hazards: set[str] = set()
    if package.startswith("net.minecraft.network.protocol"):
        hazards.add("CLIENT_OBSERVABLE")
    if texts.intersection({"Codec", "MapCodec", "StreamCodec", "ByteBufCodecs", "STREAM_CODEC", "addPacket"}):
        hazards.add("CODEC")
    if any(text.startswith("Registry") or text in {"RegistryAccess", "RegistryOps"} for text in texts):
        hazards.add("REGISTRY")
    return tuple(sorted(hazards))


def _classifications(package: str) -> tuple[tuple[str, float, str], ...]:
    if package.startswith("net.minecraft.network.protocol"):
        return (
            ("CLIENT_OBSERVABLE", 0.95, "source-level static protocol declaration"),
            ("PROTOCOL", 0.99, "source-level static protocol declaration"),
        )
    return ()


def _expected_initializers(
    conn: sqlite3.Connection,
    source: Path,
) -> tuple[dict[int, StaticInitialization], dict[int, tuple[str, str]]]:
    zf, archive_sha256, version_raw = atlas.open_source(source)
    try:
        version = json.loads(version_raw)
        _verify_source_identity(
            conn,
            archive_sha256=archive_sha256,
            version=version,
        )
        type_rows = conn.execute(
            """SELECT t.id,t.qualified_name,f.path,f.package
               FROM types t JOIN source_files f ON f.id=t.file_id"""
        ).fetchall()
        type_ids = {
            (str(row[2]), str(row[1])): int(row[0])
            for row in type_rows
        }
        type_meta = {
            int(row[0]): (str(row[3]), str(row[2]))
            for row in type_rows
        }

        expected: dict[int, StaticInitialization] = {}
        for path in sorted(name for name in zf.namelist() if name.endswith(".java")):
            raw = zf.read(path)
            text = raw.decode("utf-8", errors="replace")
            tokens = atlas.tokenize_java(text)
            braces = atlas.matching_pairs(tokens, "{", "}")
            package, _imports = atlas.package_and_imports(tokens)
            for typ in atlas.extract_types(tokens, package, braces):
                type_id = type_ids.get((path, typ.qualified_name))
                if type_id is None:
                    raise DeclarationIndexError(
                        f"Atlas/source type mismatch: {typ.qualified_name} in {path}"
                    )
                initialization = static_initialization(text, tokens, typ, braces)
                if initialization is not None:
                    expected[type_id] = initialization
        return expected, type_meta
    finally:
        zf.close()


def _existing_initializers(conn: sqlite3.Connection) -> dict[int, tuple[str, str, int, int]]:
    return {
        int(row[0]): (str(row[1]), str(row[2]), int(row[3]), int(row[4]))
        for row in conn.execute(
            """SELECT type_id,normalized_sha256,body_sha256,start_line,end_line
               FROM methods WHERE name=? AND signature=? AND modifiers=?""",
            (SYNTHETIC_NAME, SYNTHETIC_SIGNATURE, SYNTHETIC_MODIFIERS),
        )
    }


def index_declarations(source: Path, db_path: Path, *, check: bool) -> dict[str, object]:
    """Index or verify all source-level static initialization in one generated Atlas DB."""
    conn = atlas.connect_db(db_path)
    try:
        expected, type_meta = _expected_initializers(conn, source)
        if check:
            actual = _existing_initializers(conn)
            expected_rows = {
                type_id: (
                    init.normalized_sha256,
                    init.body_sha256,
                    init.start_line,
                    init.end_line,
                )
                for type_id, init in expected.items()
            }
            if actual != expected_rows:
                missing = len(set(expected_rows) - set(actual))
                extra = len(set(actual) - set(expected_rows))
                changed = sum(
                    1
                    for type_id in set(expected_rows).intersection(actual)
                    if expected_rows[type_id] != actual[type_id]
                )
                raise DeclarationIndexError(
                    "declaration index drifted: "
                    f"missing={missing} extra={extra} changed={changed}"
                )
        else:
            _delete_existing(conn)
            for type_id, initialization in sorted(expected.items()):
                package, _path = type_meta[type_id]
                cursor = conn.execute(
                    """INSERT INTO methods(
                           type_id,name,signature,return_type,modifiers,is_constructor,param_count,
                           start_line,end_line,body_sha256,normalized_sha256
                       ) VALUES(?,?,?,?,?,?,?,?,?,?,?)""",
                    (
                        type_id,
                        SYNTHETIC_NAME,
                        SYNTHETIC_SIGNATURE,
                        "void",
                        SYNTHETIC_MODIFIERS,
                        0,
                        0,
                        initialization.start_line,
                        initialization.end_line,
                        initialization.body_sha256,
                        initialization.normalized_sha256,
                    ),
                )
                method_id = int(cursor.lastrowid)
                conn.execute("INSERT INTO tracking(method_id) VALUES(?)", (method_id,))
                for kind in _hazards(package, initialization):
                    conn.execute(
                        "INSERT OR IGNORE INTO hazards(method_id,kind,detail,line) VALUES(?,?,?,?)",
                        (
                            method_id,
                            kind,
                            "synthetic-class-initializer",
                            initialization.start_line,
                        ),
                    )
                for label, confidence, reason in _classifications(package):
                    conn.execute(
                        """INSERT OR REPLACE INTO classifications(
                               method_id,label,confidence,source,reason
                           ) VALUES(?,?,?,?,?)""",
                        (method_id, label, confidence, "heuristic-declaration", reason),
                    )
            conn.execute(
                "INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)",
                ("declaration_index_version", DECLARATION_INDEX_VERSION),
            )
            conn.execute(
                "INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)",
                ("declaration_index_synthetic_clinit", str(len(expected))),
            )
            conn.commit()

        protocol_initializers = sum(
            1
            for type_id in expected
            if type_meta[type_id][0].startswith("net.minecraft.network.protocol")
        )
        return {
            "schema": 1,
            "kind": "vanilla-atlas-declaration-index",
            "version": DECLARATION_INDEX_VERSION,
            "mode": "check" if check else "index",
            "synthetic_clinit": len(expected),
            "protocol_synthetic_clinit": protocol_initializers,
        }
    finally:
        conn.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--db", type=Path, default=atlas.default_db())
    parser.add_argument("--check", action="store_true")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        report = index_declarations(args.source, args.db, check=args.check)
    except (DeclarationIndexError, OSError, sqlite3.Error, json.JSONDecodeError) as error:
        print(f"declaration index error: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
