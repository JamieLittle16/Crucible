#!/usr/bin/env python3
"""Index initialized Java instance fields as reviewable synthetic Atlas evidence.

Atlas methods and the declaration index already cover ordinary methods and source-level class
initialization. Some observable vanilla law lives instead in initialized instance fields, including
anonymous callback implementations. This pass projects each simple initialized instance-field
declaration into one synthetic ``<fieldinit:NAME>()`` Atlas method so unchanged VAR/gate tooling can
fingerprint, review and invalidate that source law without committing official source text.

The projection is evidence tooling only. It does not claim that Java exposes a method with this name.
Static/interface fields remain the responsibility of ``vanilla_declaration_index.py`` and are never
duplicated here.
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

INSTANCE_FIELD_INDEX_VERSION = "0.1.0"
SYNTHETIC_PREFIX = "<fieldinit:"
SYNTHETIC_MODIFIERS = "synthetic atlas-instance-field-v1"


class InstanceFieldIndexError(RuntimeError):
    """Raised when instance-field evidence cannot be bound safely."""


@dataclass(frozen=True, slots=True)
class InstanceFieldInitialization:
    """Canonical source projection for one initialized instance field declaration."""

    name: str
    start_line: int
    end_line: int
    normalized_sha256: str
    body_sha256: str
    tokens: tuple[atlas.Token, ...]

    @property
    def synthetic_name(self) -> str:
        return f"{SYNTHETIC_PREFIX}{self.name}>"

    @property
    def synthetic_signature(self) -> str:
        return f"{self.synthetic_name}()"


def _token_texts(tokens: Sequence[atlas.Token]) -> tuple[str, ...]:
    return tuple(token.text for token in tokens)


def _field_name(member: Sequence[atlas.Token], *, interface: bool) -> str | None:
    """Return a simple initialized instance-field name, or ``None`` when not eligible.

    The Atlas source corpus overwhelmingly uses one declarator per field declaration. We fail closed
    for declarations whose prefix cannot identify one unambiguous name; no heuristic fallback may
    turn a method/local/static declaration into review evidence.
    """
    if interface:
        return None
    texts = _token_texts(member)
    try:
        equals_index = texts.index("=")
    except ValueError:
        return None
    prefix = member[:equals_index]
    prefix_texts = _token_texts(prefix)
    if "static" in prefix_texts:
        return None
    identifiers = [token.text for token in prefix if token.kind == "ident"]
    if not identifiers:
        return None
    name = identifiers[-1]
    # A top-level comma before the initializer indicates a multi-declarator field. Skip it rather
    # than pretending the full declaration belongs uniquely to one field.
    depth = 0
    for token in prefix:
        if token.text in {"(", "[", "{", "<"}:
            depth += 1
        elif token.text in {")", "]", "}", ">"}:
            depth = max(0, depth - 1)
        elif token.text == "," and depth == 0:
            return None
    return name


def instance_field_initializations(
    text: str,
    tokens: Sequence[atlas.Token],
    typ: atlas.TypeDecl,
    braces: dict[int, int],
) -> tuple[InstanceFieldInitialization, ...]:
    """Extract simple top-level initialized instance fields from one source type."""
    if typ.kind == "interface":
        return ()

    result: list[InstanceFieldInitialization] = []
    member_start = typ.body_open + 1
    index = member_start

    # Enum constant preambles are not instance fields. Advance to the first semicolon while keeping
    # anonymous constant bodies intact. Named enum fields after that point are scanned normally.
    if typ.kind == "enum":
        while index < typ.body_close:
            token = tokens[index]
            if token.text == "{" and index in braces:
                index = braces[index] + 1
                continue
            if token.text == ";":
                member_start = index + 1
                index = member_start
                break
            index += 1

    while index < typ.body_close:
        token = tokens[index]
        if token.text == ";":
            member = tuple(tokens[member_start : index + 1])
            name = _field_name(member, interface=False)
            if name is not None:
                raw = text[member[0].start : member[-1].end]
                result.append(
                    InstanceFieldInitialization(
                        name=name,
                        start_line=member[0].line,
                        end_line=member[-1].line,
                        normalized_sha256=atlas.normalized_fingerprint(member),
                        body_sha256=atlas.sha256_bytes(raw.encode("utf-8")),
                        tokens=member,
                    )
                )
            member_start = index + 1
            index += 1
            continue

        if token.text != "{" or index not in braces:
            index += 1
            continue

        close = braces[index]
        if close > typ.body_close:
            raise InstanceFieldIndexError(
                f"member brace escapes type {typ.qualified_name} at line {token.line}"
            )
        prefix = tuple(tokens[member_start:index])
        if "=" in _token_texts(prefix):
            # Anonymous classes, lambdas and array/object initializers remain part of the field.
            index = close + 1
            continue

        # Method/constructor/nested type/initializer block: skip the body and resume after it.
        member_start = close + 1
        index = close + 1

    names = [field.name for field in result]
    if len(names) != len(set(names)):
        raise InstanceFieldIndexError(
            f"duplicate synthetic field identity while scanning {typ.qualified_name}: {names}"
        )
    return tuple(result)


def _verify_source_identity(
    conn: sqlite3.Connection, *, archive_sha256: str, version: dict[str, object]
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
        raise InstanceFieldIndexError(
            "source archive does not match Atlas database identity: " + "; ".join(mismatches)
        )


def _delete_method_rows(conn: sqlite3.Connection, ids: Sequence[int]) -> None:
    for offset in range(0, len(ids), 500):
        chunk = list(ids[offset : offset + 500])
        if not chunk:
            continue
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


def _delete_existing(conn: sqlite3.Connection) -> None:
    rows = conn.execute(
        "SELECT id FROM methods WHERE modifiers=?",
        (SYNTHETIC_MODIFIERS,),
    ).fetchall()
    _delete_method_rows(conn, [int(row[0]) for row in rows])


def _hazards(initialization: InstanceFieldInitialization) -> tuple[str, ...]:
    texts = set(_token_texts(initialization.tokens))
    hazards: set[str] = set()
    if any(text.startswith(("Clientbound", "Serverbound")) for text in texts) or {
        "Packet",
        "send",
        "sendPacket",
    }.intersection(texts):
        hazards.add("CLIENT_OBSERVABLE")
        hazards.add("NETWORK_SEND")
    if texts.intersection(
        {"Codec", "MapCodec", "StreamCodec", "ByteBufCodecs", "STREAM_CODEC"}
    ):
        hazards.add("CODEC")
    if any(
        text.startswith("Registry") or text in {"RegistryAccess", "RegistryOps"}
        for text in texts
    ):
        hazards.add("REGISTRY")
    return tuple(sorted(hazards))


def _expected(
    conn: sqlite3.Connection, source: Path
) -> tuple[
    dict[tuple[int, str], InstanceFieldInitialization],
    dict[int, tuple[str, str]],
]:
    zf, archive_sha256, version_raw = atlas.open_source(source)
    try:
        version = json.loads(version_raw)
        _verify_source_identity(conn, archive_sha256=archive_sha256, version=version)
        rows = conn.execute(
            """SELECT t.id,t.qualified_name,f.path,f.package
               FROM types t JOIN source_files f ON f.id=t.file_id"""
        ).fetchall()
        type_ids = {(str(row[2]), str(row[1])): int(row[0]) for row in rows}
        type_meta = {int(row[0]): (str(row[3]), str(row[2])) for row in rows}

        expected: dict[tuple[int, str], InstanceFieldInitialization] = {}
        for path in sorted(name for name in zf.namelist() if name.endswith(".java")):
            text = zf.read(path).decode("utf-8", errors="replace")
            tokens = atlas.tokenize_java(text)
            braces = atlas.matching_pairs(tokens, "{", "}")
            package, _imports = atlas.package_and_imports(tokens)
            for typ in atlas.extract_types(tokens, package, braces):
                type_id = type_ids.get((path, typ.qualified_name))
                if type_id is None:
                    raise InstanceFieldIndexError(
                        f"Atlas/source type mismatch: {typ.qualified_name} in {path}"
                    )
                for initialization in instance_field_initializations(text, tokens, typ, braces):
                    key = (type_id, initialization.name)
                    if key in expected:
                        raise InstanceFieldIndexError(
                            f"duplicate field initializer identity: {typ.qualified_name}."
                            f"{initialization.name}"
                        )
                    expected[key] = initialization
        return expected, type_meta
    finally:
        zf.close()


def _existing(
    conn: sqlite3.Connection,
) -> dict[tuple[int, str], tuple[str, str, int, int]]:
    result: dict[tuple[int, str], tuple[str, str, int, int]] = {}
    for row in conn.execute(
        """SELECT type_id,name,normalized_sha256,body_sha256,start_line,end_line
           FROM methods WHERE modifiers=?""",
        (SYNTHETIC_MODIFIERS,),
    ):
        name = str(row[1])
        if not name.startswith(SYNTHETIC_PREFIX) or not name.endswith(">"):
            raise InstanceFieldIndexError(f"malformed synthetic field node name: {name}")
        field_name = name[len(SYNTHETIC_PREFIX) : -1]
        result[(int(row[0]), field_name)] = (
            str(row[2]),
            str(row[3]),
            int(row[4]),
            int(row[5]),
        )
    return result


def index_instance_fields(source: Path, db_path: Path, *, check: bool) -> dict[str, object]:
    """Index or verify initialized instance-field evidence in one generated Atlas DB."""
    conn = atlas.connect_db(db_path)
    try:
        expected, type_meta = _expected(conn, source)
        expected_rows = {
            key: (
                initialization.normalized_sha256,
                initialization.body_sha256,
                initialization.start_line,
                initialization.end_line,
            )
            for key, initialization in expected.items()
        }
        if check:
            actual = _existing(conn)
            if actual != expected_rows:
                missing = len(set(expected_rows) - set(actual))
                extra = len(set(actual) - set(expected_rows))
                changed = sum(
                    1
                    for key in set(expected_rows).intersection(actual)
                    if expected_rows[key] != actual[key]
                )
                raise InstanceFieldIndexError(
                    "instance-field index drifted: "
                    f"missing={missing} extra={extra} changed={changed}"
                )
        else:
            _delete_existing(conn)
            for (type_id, _field_name), initialization in sorted(expected.items()):
                cursor = conn.execute(
                    """INSERT INTO methods(
                           type_id,name,signature,return_type,modifiers,is_constructor,param_count,
                           start_line,end_line,body_sha256,normalized_sha256
                       ) VALUES(?,?,?,?,?,?,?,?,?,?,?)""",
                    (
                        type_id,
                        initialization.synthetic_name,
                        initialization.synthetic_signature,
                        "field-initializer",
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
                for kind in _hazards(initialization):
                    conn.execute(
                        "INSERT OR IGNORE INTO hazards(method_id,kind,detail,line) VALUES(?,?,?,?)",
                        (
                            method_id,
                            kind,
                            "synthetic-instance-field-initializer",
                            initialization.start_line,
                        ),
                    )
                if "CLIENT_OBSERVABLE" in _hazards(initialization):
                    conn.execute(
                        """INSERT OR REPLACE INTO classifications(
                               method_id,label,confidence,source,reason
                           ) VALUES(?,?,?,?,?)""",
                        (
                            method_id,
                            "CLIENT_OBSERVABLE",
                            0.95,
                            "heuristic-instance-field",
                            "instance field initializer contains packet/send law",
                        ),
                    )
            conn.execute(
                "INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)",
                ("instance_field_index_version", INSTANCE_FIELD_INDEX_VERSION),
            )
            conn.execute(
                "INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)",
                ("instance_field_index_synthetic_count", str(len(expected))),
            )
            conn.commit()

        observable = sum(
            1
            for initialization in expected.values()
            if "CLIENT_OBSERVABLE" in _hazards(initialization)
        )
        return {
            "schema": 1,
            "kind": "vanilla-atlas-instance-field-index",
            "index_version": INSTANCE_FIELD_INDEX_VERSION,
            "synthetic_field_initializers": len(expected),
            "observable_field_initializers": observable,
            "check": check,
        }
    finally:
        conn.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="vanilla-instance-field-index")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--db", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        report = index_instance_fields(args.source, args.db, check=args.check)
    except (OSError, json.JSONDecodeError, sqlite3.Error, InstanceFieldIndexError) as error:
        print(f"instance-field index error: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
