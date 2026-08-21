#!/usr/bin/env python3
"""Crucible Vanilla Atlas.

Builds a disposable SQLite structural index from a pinned official Minecraft Java
source archive without copying source bodies into the repository.  The index is
local/generated state; compact aggregate audit reports are safe to commit.

The parser is deliberately dependency-free.  It performs Java lexical analysis
and conservative structural extraction.  It does not pretend to be a Java type
checker: call targets are syntactic unless/until a later resolution pass proves
more.  Uncertainty is represented explicitly in the schema.
"""
from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import os
import re
import sqlite3
import sys
import time
import tomllib
import zipfile
from collections import Counter, defaultdict
from pathlib import Path
from typing import Iterable, Iterator, Sequence

SCHEMA_VERSION = 1
ATLAS_VERSION = "0.1.0"
REVIEW_STATUSES = (
    "UNSEEN", "INDEXED", "CLASSIFIED", "VAR_REVIEWED", "SEM_EXTRACTED",
    "REFERENCE_IMPLEMENTED", "REFERENCE_PARITY", "PRODUCTION_IMPLEMENTED",
    "EQUIVALENCE_QUALIFIED", "PERFORMANCE_QUALIFIED", "INTEGRATED", "STALE",
)
REVIEW_RANK = {status: i for i, status in enumerate(REVIEW_STATUSES)}
JAVA_KEYWORDS = {
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char",
    "class", "const", "continue", "default", "do", "double", "else", "enum",
    "extends", "final", "finally", "float", "for", "goto", "if", "implements",
    "import", "instanceof", "int", "interface", "long", "native", "new", "package",
    "private", "protected", "public", "record", "return", "sealed", "short", "static",
    "strictfp", "super", "switch", "synchronized", "this", "throw", "throws",
    "transient", "try", "var", "void", "volatile", "while", "yield", "permits",
    "non-sealed", "true", "false", "null",
}
TYPE_DECL_KINDS = {"class", "interface", "enum", "record"}
CONTROL_CALL_WORDS = {"if", "for", "while", "switch", "catch", "synchronized", "return", "throw", "new"}
MODIFIERS = {
    "public", "protected", "private", "static", "final", "abstract", "synchronized",
    "native", "strictfp", "default", "transient", "volatile", "sealed", "non-sealed",
}
ASSIGN_OPS = {"=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", ">>>="}
INC_OPS = {"++", "--"}

HAZARD_PATTERNS: dict[str, tuple[str, ...]] = {
    "RNG": (
        "RandomSource", "LegacyRandomSource", "WorldgenRandom", "ThreadSafeLegacyRandomSource",
        "nextInt", "nextLong", "nextFloat", "nextDouble", "nextBoolean", "nextGaussian",
        "nextBetweenInclusive", "nextIntBetweenInclusive", "triangle", "forkPositional",
    ),
    "WALL_TIME": ("currentTimeMillis", "nanoTime", "Instant.now", "getMillis", "getNanos"),
    "CONCURRENCY": (
        "CompletableFuture", "Executor", "ExecutorService", "ForkJoinPool", "ThreadingDetector",
        "Thread", "Mailbox", "ProcessorMailbox", "synchronized", "AtomicInteger", "AtomicLong",
        "AtomicReference", "ReentrantLock", "StampedLock", "thenApplyAsync", "thenComposeAsync",
        "runAsync", "supplyAsync", "submit", "execute",
    ),
    "ORDERING": (
        "HashMap", "HashSet", "IdentityHashMap", "LinkedHashMap", "LinkedHashSet", "TreeMap",
        "TreeSet", "PriorityQueue", "Object2ObjectOpenHashMap", "ObjectOpenHashSet", "Int2ObjectOpenHashMap",
        "Long2ObjectOpenHashMap", "LongOpenHashSet", "sort", "sorted",
    ),
    "CHUNK_ACCESS": (
        "getChunk", "getChunkNow", "getChunkFuture", "getChunkFutureMainThread", "getChunkForLighting",
        "getChunkAt", "scheduleChunkLoad", "addTicket", "removeTicket", "ChunkHolder", "ChunkResult",
    ),
    "SCHEDULED_TICK": (
        "scheduleTick", "ScheduledTick", "LevelTicks", "LevelChunkTicks", "tickAccess", "TickPriority",
    ),
    "NEIGHBOR_UPDATE": (
        "updateNeighborsAt", "neighborChanged", "updateShape", "updateNeighbourForOutputSignal",
        "NeighborUpdater", "shapeUpdate",
    ),
    "NETWORK_SEND": ("sendPacket", "broadcastAndSend", "broadcast", "Clientbound", "Packet"),
    "PERSISTENCE": (
        "CompoundTag", "ListTag", "Tag", "Nbt", "RegionFile", "IOWorker", "SerializableChunkData",
        "DataInput", "DataOutput", "SavedData", "DimensionDataStorage",
    ),
    "CODEC": ("Codec", "MapCodec", "StreamCodec", "ByteBufCodecs", "RecordCodecBuilder"),
    "REGISTRY": ("Registry", "Holder", "ResourceKey", "BuiltInRegistries", "Registries"),
}

@dataclasses.dataclass(frozen=True, slots=True)
class Token:
    kind: str
    text: str
    line: int
    start: int
    end: int

@dataclasses.dataclass(slots=True)
class TypeDecl:
    simple_name: str
    qualified_name: str
    kind: str
    modifiers: str
    start_token: int
    body_open: int
    body_close: int
    start_line: int
    end_line: int
    parent_qualified_name: str | None

@dataclasses.dataclass(slots=True)
class MethodDecl:
    name: str
    signature: str
    return_type: str
    modifiers: str
    start_token: int
    params_open: int
    params_close: int
    body_open: int | None
    body_close: int | None
    start_line: int
    end_line: int
    is_constructor: bool
    param_count: int

@dataclasses.dataclass(slots=True)
class FieldDecl:
    name: str
    type_name: str
    modifiers: str
    line: int


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def tokenize_java(text: str) -> list[Token]:
    """Lex Java enough for structural extraction while preserving line numbers."""
    out: list[Token] = []
    i = 0
    line = 1
    n = len(text)
    multi_ops = (
        ">>>=", "<<=", ">>=", "...", "::", "->", "++", "--", "==", "!=", "<=", ">=",
        "&&", "||", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", ">>>",
    )
    while i < n:
        c = text[i]
        if c.isspace():
            if c == "\n":
                line += 1
            i += 1
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            i += 2
            while i < n and text[i] != "\n":
                i += 1
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            i += 2
            while i < n - 1:
                if text[i] == "\n":
                    line += 1
                if text[i] == "*" and text[i + 1] == "/":
                    i += 2
                    break
                i += 1
            continue
        if c in ('"', "'"):
            quote = c
            start = i
            start_line = line
            i += 1
            while i < n:
                if text[i] == "\\":
                    i += 2
                    continue
                if text[i] == "\n":
                    line += 1
                if text[i] == quote:
                    i += 1
                    break
                i += 1
            out.append(Token("string" if quote == '"' else "char", quote, start_line, start, i))
            continue
        if c.isalpha() or c in "_$":
            start = i
            i += 1
            while i < n and (text[i].isalnum() or text[i] in "_$"):
                i += 1
            word = text[start:i]
            kind = "keyword" if word in JAVA_KEYWORDS else "ident"
            out.append(Token(kind, word, line, start, i))
            continue
        if c.isdigit():
            start = i
            i += 1
            while i < n and (text[i].isalnum() or text[i] in "._+-"):
                if text[i] in "+-" and text[i - 1] not in "eEpP":
                    break
                i += 1
            out.append(Token("number", "#", line, start, i))
            continue
        matched = None
        for op in multi_ops:
            if text.startswith(op, i):
                matched = op
                break
        if matched:
            out.append(Token("symbol", matched, line, i, i + len(matched)))
            i += len(matched)
        else:
            out.append(Token("symbol", c, line, i, i + 1))
            i += 1
    return out


def matching_pairs(tokens: Sequence[Token], left: str, right: str) -> dict[int, int]:
    stack: list[int] = []
    pairs: dict[int, int] = {}
    for idx, tok in enumerate(tokens):
        if tok.text == left:
            stack.append(idx)
        elif tok.text == right and stack:
            opener = stack.pop()
            pairs[opener] = idx
    return pairs


def join_tokens(tokens: Sequence[Token]) -> str:
    return " ".join(t.text for t in tokens)


def normalized_fingerprint(tokens: Sequence[Token]) -> str:
    parts: list[str] = []
    for token in tokens:
        if token.kind == "string":
            parts.append('"$"')
        elif token.kind == "char":
            parts.append("'$'")
        elif token.kind == "number":
            parts.append("#")
        else:
            parts.append(token.text)
    return sha256_bytes(" ".join(parts).encode())


def package_and_imports(tokens: Sequence[Token]) -> tuple[str, list[tuple[str, bool]]]:
    package = ""
    imports: list[tuple[str, bool]] = []
    i = 0
    while i < len(tokens):
        if tokens[i].text == "package":
            j = i + 1
            parts: list[str] = []
            while j < len(tokens) and tokens[j].text != ";":
                parts.append(tokens[j].text)
                j += 1
            package = "".join(parts)
            i = j + 1
            continue
        if tokens[i].text == "import":
            j = i + 1
            is_static = j < len(tokens) and tokens[j].text == "static"
            if is_static:
                j += 1
            parts: list[str] = []
            while j < len(tokens) and tokens[j].text != ";":
                parts.append(tokens[j].text)
                j += 1
            imports.append(("".join(parts), is_static))
            i = j + 1
            continue
        i += 1
    return package, imports


def _declaration_modifiers(tokens: Sequence[Token], kind_index: int) -> str:
    start = kind_index - 1
    collected: list[str] = []
    while start >= 0:
        text = tokens[start].text
        if text in {";", "{", "}"}:
            break
        if text in MODIFIERS or text == "@" or tokens[start].kind == "ident":
            collected.append(text)
        start -= 1
    mods = [x for x in reversed(collected) if x in MODIFIERS]
    return " ".join(dict.fromkeys(mods))


def extract_types(tokens: Sequence[Token], package: str, braces: dict[int, int]) -> list[TypeDecl]:
    raw: list[tuple[int, int, int, str, str, int, int]] = []
    i = 0
    while i < len(tokens) - 1:
        tok = tokens[i]
        if tok.text in TYPE_DECL_KINDS and tokens[i + 1].kind == "ident":
            name = tokens[i + 1].text
            j = i + 2
            while j < len(tokens) and tokens[j].text not in {"{", ";"}:
                j += 1
            if j < len(tokens) and tokens[j].text == "{" and j in braces:
                close = braces[j]
                raw.append((i, j, close, name, tok.text, tok.line, tokens[close].line))
            i = j
        i += 1

    result: list[TypeDecl] = []
    # Nested type parent is the smallest enclosing previously discovered type.
    for kind_idx, open_idx, close_idx, name, kind, start_line, end_line in raw:
        parent: TypeDecl | None = None
        for candidate in result:
            if candidate.body_open < kind_idx < candidate.body_close:
                if parent is None or candidate.body_open > parent.body_open:
                    parent = candidate
        qualified = f"{package}.{name}" if package else name
        if parent is not None:
            qualified = f"{parent.qualified_name}${name}"
        result.append(TypeDecl(
            simple_name=name,
            qualified_name=qualified,
            kind=kind,
            modifiers=_declaration_modifiers(tokens, kind_idx),
            start_token=kind_idx,
            body_open=open_idx,
            body_close=close_idx,
            start_line=start_line,
            end_line=end_line,
            parent_qualified_name=parent.qualified_name if parent else None,
        ))
    return result


def depth_map(tokens: Sequence[Token], start: int, end: int) -> list[int]:
    depth = 0
    out = [0] * (end - start + 1)
    for offset, idx in enumerate(range(start, end + 1)):
        out[offset] = depth
        if tokens[idx].text == "{":
            depth += 1
        elif tokens[idx].text == "}":
            depth = max(0, depth - 1)
    return out


def count_top_level_items(tokens: Sequence[Token]) -> int:
    if not tokens:
        return 0
    depth = 0
    count = 1
    for token in tokens:
        if token.text in {"(", "[", "{", "<"}:
            depth += 1
        elif token.text in {")", "]", "}", ">"}:
            depth = max(0, depth - 1)
        elif token.text == "," and depth == 0:
            count += 1
    return count


def extract_members(tokens: Sequence[Token], typ: TypeDecl, parens: dict[int, int], braces: dict[int, int]) -> tuple[list[MethodDecl], list[FieldDecl]]:
    methods: list[MethodDecl] = []
    fields: list[FieldDecl] = []
    i = typ.body_open + 1
    member_start = i
    depth = 0
    while i < typ.body_close:
        text = tokens[i].text
        if text == "{":
            # Initializer or nested body: skip whole body when at member level.
            if depth == 0 and i in braces:
                i = braces[i] + 1
                member_start = i
                continue
            depth += 1
            i += 1
            continue
        if text == "}":
            depth = max(0, depth - 1)
            i += 1
            continue
        if depth != 0:
            i += 1
            continue

        if text == "(" and i in parens:
            name_idx = i - 1
            if name_idx >= member_start and tokens[name_idx].kind == "ident" and tokens[name_idx].text not in CONTROL_CALL_WORDS:
                close_paren = parens[i]
                # A member method declaration must terminate with {, ;, throws..., or default expression.
                j = close_paren + 1
                while j < typ.body_close and tokens[j].text not in {"{", ";", "="}:
                    j += 1
                if j < typ.body_close and tokens[j].text in {"{", ";"}:
                    prefix = list(tokens[member_start:name_idx])
                    # Reject annotations/member expressions that clearly contain assignment before the candidate.
                    if not any(t.text == "=" for t in prefix):
                        name = tokens[name_idx].text
                        param_tokens = tokens[i + 1:close_paren]
                        params = join_tokens(param_tokens)
                        sig = f"{name}({params})"
                        param_count = count_top_level_items(param_tokens)
                        mods = " ".join(t.text for t in prefix if t.text in MODIFIERS)
                        # Return type is the last non-modifier/annotation-ish token before method name.
                        ret_candidates = [t.text for t in prefix if t.text not in MODIFIERS and t.text not in {"@"}]
                        is_ctor = name == typ.simple_name
                        return_type = "" if is_ctor else (ret_candidates[-1] if ret_candidates else "")
                        body_open = j if tokens[j].text == "{" else None
                        body_close = braces.get(j) if body_open is not None else None
                        end_line = tokens[body_close].line if body_close is not None else tokens[j].line
                        methods.append(MethodDecl(
                            name=name,
                            signature=sig,
                            return_type=return_type,
                            modifiers=mods,
                            start_token=member_start,
                            params_open=i,
                            params_close=close_paren,
                            body_open=body_open,
                            body_close=body_close,
                            start_line=tokens[member_start].line if member_start < len(tokens) else tokens[name_idx].line,
                            end_line=end_line,
                            is_constructor=is_ctor,
                            param_count=param_count,
                        ))
                        i = (body_close + 1) if body_close is not None else (j + 1)
                        member_start = i
                        continue
            i = parens[i] + 1
            continue

        if text == ";":
            segment = list(tokens[member_start:i])
            # Field declarations only. Method declarations ending ; are handled above.
            if segment and not any(t.text == "(" for t in segment):
                # Remove annotations/modifiers; infer type from prefix and declared names from commas/assignment.
                stripped = [t for t in segment if t.text not in MODIFIERS]
                if stripped:
                    # Candidate variable names are identifiers followed by = , [ or end, excluding obvious type/name path pieces.
                    candidates: list[int] = []
                    for k, tok in enumerate(stripped):
                        if tok.kind != "ident":
                            continue
                        nxt = stripped[k + 1].text if k + 1 < len(stripped) else None
                        if nxt in {"=", ",", "["} or nxt is None:
                            candidates.append(k)
                    if candidates:
                        first_name = candidates[0]
                        type_tokens = stripped[:first_name]
                        type_name = "".join(t.text for t in type_tokens if t.text != "@")
                        mods = " ".join(t.text for t in segment if t.text in MODIFIERS)
                        for k in candidates:
                            name = stripped[k].text
                            if name not in JAVA_KEYWORDS and name != typ.simple_name:
                                fields.append(FieldDecl(name=name, type_name=type_name, modifiers=mods, line=stripped[k].line))
            i += 1
            member_start = i
            continue
        i += 1
    # De-duplicate false-positive repeated declarators.
    dedup: dict[tuple[str, int], FieldDecl] = {(f.name, f.line): f for f in fields}
    return methods, list(dedup.values())


def syntactic_calls(tokens: Sequence[Token], method: MethodDecl) -> list[tuple[str | None, str, int, int]]:
    if method.body_open is None or method.body_close is None:
        return []
    body = tokens[method.body_open + 1:method.body_close]
    result: list[tuple[str | None, str, int, int]] = []
    paren_pairs = matching_pairs(body, "(", ")")
    for i in range(len(body) - 1):
        tok = body[i]
        if tok.kind != "ident" or body[i + 1].text != "(":
            continue
        name = tok.text
        if name in CONTROL_CALL_WORDS:
            continue
        owner: str | None = None
        if i >= 2 and body[i - 1].text == "." and body[i - 2].kind in {"ident", "keyword"}:
            # Capture a short dotted owner chain, capped to avoid expression noise.
            parts = [body[i - 2].text]
            k = i - 3
            while k >= 1 and body[k].text == "." and body[k - 1].kind in {"ident", "keyword"} and len(parts) < 4:
                parts.append(body[k - 1].text)
                k -= 2
            owner = ".".join(reversed(parts))
        open_local = i + 1
        close_local = paren_pairs.get(open_local)
        argc = 0
        if close_local is not None and close_local > open_local + 1:
            depth = 0
            argc = 1
            for t in body[open_local + 1:close_local]:
                if t.text in {"(", "[", "{"}:
                    depth += 1
                elif t.text in {")", "]", "}"}:
                    depth = max(0, depth - 1)
                elif t.text == "," and depth == 0:
                    argc += 1
        result.append((owner, name, argc, tok.line))
    return result


def field_accesses(tokens: Sequence[Token], method: MethodDecl, field_names: set[str]) -> list[tuple[str, str, int]]:
    if method.body_open is None or method.body_close is None or not field_names:
        return []
    body = tokens[method.body_open + 1:method.body_close]
    result: list[tuple[str, str, int]] = []
    for i, tok in enumerate(body):
        if tok.kind != "ident" or tok.text not in field_names:
            continue
        prev = body[i - 1].text if i > 0 else None
        nxt = body[i + 1].text if i + 1 < len(body) else None
        # Avoid obvious local declarations shadowing fields: Type field = ...
        if i > 0 and body[i - 1].kind == "ident" and prev not in {"this", "super"}:
            if nxt in ASSIGN_OPS | {";", ","}:
                continue
        if nxt in INC_OPS or prev in INC_OPS or nxt in ASSIGN_OPS - {"="}:
            mode = "read_write"
        elif nxt == "=":
            mode = "write"
        else:
            mode = "read"
        result.append((tok.text, mode, tok.line))
    return result


def hazards_for(tokens: Sequence[Token], method: MethodDecl, package: str, type_name: str) -> list[tuple[str, str, int]]:
    if method.body_open is None or method.body_close is None:
        return []
    body = tokens[method.body_open + 1:method.body_close]
    texts = [t.text for t in body]
    text_set = set(texts)
    out: list[tuple[str, str, int]] = []
    seen: set[tuple[str, str]] = set()
    for kind, needles in HAZARD_PATTERNS.items():
        for needle in needles:
            if needle not in text_set:
                continue
            for tok in body:
                if tok.text == needle:
                    key = (kind, needle)
                    if key not in seen:
                        out.append((kind, needle, tok.line))
                        seen.add(key)
                    break
    # Package/type contextual hazards are explicit and conservative.
    if ".network.protocol" in package and ("NETWORK_SEND", "packet-surface") not in seen:
        out.append(("CLIENT_OBSERVABLE", "protocol-package", method.start_line))
    if any(x in type_name for x in ("Chunk", "ServerLevel", "Level")) and any(c[1] in {"getChunk", "getChunkNow", "getChunkFuture"} for c in out):
        out.append(("CHUNK_REALIZATION_CANDIDATE", "chunk-access-call", method.start_line))
    return out


def classifications_for(package: str, type_name: str, method: MethodDecl, hazards: Sequence[tuple[str, str, int]]) -> list[tuple[str, float, str]]:
    labels: dict[str, tuple[float, str]] = {}
    hazard_kinds = {h[0] for h in hazards}
    if ".network.protocol" in package:
        labels["PROTOCOL"] = (0.99, "package")
        labels["CLIENT_OBSERVABLE"] = (0.95, "protocol surface")
    if package.startswith("net.minecraft.world.level.block") or package.startswith("net.minecraft.world.entity") or package.startswith("net.minecraft.world.item"):
        labels["SEMANTIC_GAMEPLAY_CANDIDATE"] = (0.85, "gameplay package")
    if package.startswith("net.minecraft.server.level"):
        labels["SERVER_SIMULATION_CANDIDATE"] = (0.80, "server-level package")
    if "RNG" in hazard_kinds:
        labels["RNG_SENSITIVE"] = (0.99, "random API use")
    if "ORDERING" in hazard_kinds:
        labels["ORDER_SENSITIVE_CANDIDATE"] = (0.70, "ordering-sensitive collection/sort use")
    if "CONCURRENCY" in hazard_kinds:
        labels["SCHEDULING_CANDIDATE"] = (0.80, "concurrency API use")
    if "SCHEDULED_TICK" in hazard_kinds:
        labels["SCHEDULED_TICK"] = (0.99, "scheduled tick API use")
    if "PERSISTENCE" in hazard_kinds or ".storage" in package or ".nbt" in package:
        labels["PERSISTENCE"] = (0.90, "storage/NBT surface")
    if "CODEC" in hazard_kinds:
        labels["CODEC_SERIALIZATION"] = (0.90, "codec API use")
    if "CHUNK_ACCESS" in hazard_kinds:
        labels["CHUNK_ACCESS"] = (0.95, "chunk API use")
    if "NEIGHBOR_UPDATE" in hazard_kinds:
        labels["NEIGHBOR_UPDATE"] = (0.95, "neighbor-update API use")
    if any(s in type_name for s in ("Heightmap", "Cache", "Tracker")) or method.name.startswith(("recalc", "update", "refresh")):
        labels["DERIVED_STATE_CANDIDATE"] = (0.55, "name heuristic")
    if method.name in {"load", "unload", "onLoad", "onRemove", "remove", "add", "tick", "close"}:
        labels["LIFECYCLE_CANDIDATE"] = (0.50, "method-name heuristic")
    return [(label, confidence, reason) for label, (confidence, reason) in labels.items()]


def schema_sql() -> str:
    return """
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS source_files(
  id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, package TEXT NOT NULL,
  sha256 TEXT NOT NULL, bytes INTEGER NOT NULL, lines INTEGER NOT NULL, nonblank_lines INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS imports(
  id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL REFERENCES source_files(id) ON DELETE CASCADE,
  import_name TEXT NOT NULL, is_static INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS types(
  id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL REFERENCES source_files(id) ON DELETE CASCADE,
  qualified_name TEXT NOT NULL UNIQUE, simple_name TEXT NOT NULL, kind TEXT NOT NULL,
  modifiers TEXT NOT NULL, parent_qualified_name TEXT, start_line INTEGER NOT NULL, end_line INTEGER NOT NULL,
  normalized_sha256 TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS fields(
  id INTEGER PRIMARY KEY, type_id INTEGER NOT NULL REFERENCES types(id) ON DELETE CASCADE,
  name TEXT NOT NULL, type_name TEXT NOT NULL, modifiers TEXT NOT NULL, line INTEGER NOT NULL,
  UNIQUE(type_id, name, line)
);
CREATE TABLE IF NOT EXISTS methods(
  id INTEGER PRIMARY KEY, type_id INTEGER NOT NULL REFERENCES types(id) ON DELETE CASCADE,
  name TEXT NOT NULL, signature TEXT NOT NULL, return_type TEXT NOT NULL, modifiers TEXT NOT NULL,
  is_constructor INTEGER NOT NULL, param_count INTEGER NOT NULL, start_line INTEGER NOT NULL, end_line INTEGER NOT NULL,
  body_sha256 TEXT NOT NULL, normalized_sha256 TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_methods_name ON methods(name);
CREATE INDEX IF NOT EXISTS idx_methods_type ON methods(type_id);
CREATE TABLE IF NOT EXISTS method_calls(
  id INTEGER PRIMARY KEY, caller_method_id INTEGER NOT NULL REFERENCES methods(id) ON DELETE CASCADE,
  owner_text TEXT, callee_name TEXT NOT NULL, arg_count INTEGER NOT NULL, line INTEGER NOT NULL,
  resolution TEXT NOT NULL DEFAULT 'syntactic', resolved_method_id INTEGER REFERENCES methods(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_calls_caller ON method_calls(caller_method_id);
CREATE INDEX IF NOT EXISTS idx_calls_name ON method_calls(callee_name);
CREATE TABLE IF NOT EXISTS field_accesses(
  id INTEGER PRIMARY KEY, method_id INTEGER NOT NULL REFERENCES methods(id) ON DELETE CASCADE,
  field_name TEXT NOT NULL, mode TEXT NOT NULL, line INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_field_access_method ON field_accesses(method_id);
CREATE TABLE IF NOT EXISTS hazards(
  id INTEGER PRIMARY KEY, method_id INTEGER NOT NULL REFERENCES methods(id) ON DELETE CASCADE,
  kind TEXT NOT NULL, detail TEXT NOT NULL, line INTEGER NOT NULL,
  UNIQUE(method_id, kind, detail)
);
CREATE INDEX IF NOT EXISTS idx_hazards_kind ON hazards(kind);
CREATE TABLE IF NOT EXISTS classifications(
  id INTEGER PRIMARY KEY, method_id INTEGER NOT NULL REFERENCES methods(id) ON DELETE CASCADE,
  label TEXT NOT NULL, confidence REAL NOT NULL, source TEXT NOT NULL, reason TEXT NOT NULL,
  UNIQUE(method_id, label, source)
);
CREATE INDEX IF NOT EXISTS idx_class_label ON classifications(label);
CREATE TABLE IF NOT EXISTS tracking(
  method_id INTEGER PRIMARY KEY REFERENCES methods(id) ON DELETE CASCADE,
  review_status TEXT NOT NULL DEFAULT 'UNSEEN', var_id TEXT, notes TEXT
);
CREATE TABLE IF NOT EXISTS semantic_edges(
  id INTEGER PRIMARY KEY, method_id INTEGER REFERENCES methods(id) ON DELETE SET NULL,
  var_id TEXT, sem_id TEXT, evidence_id TEXT, relation TEXT NOT NULL
);
"""


def init_db(path: Path, *, replace: bool) -> sqlite3.Connection:
    path.parent.mkdir(parents=True, exist_ok=True)
    if replace and path.exists():
        path.unlink()
    conn = sqlite3.connect(path)
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA synchronous=NORMAL")
    conn.executescript(schema_sql())
    return conn


def open_source(path: Path) -> tuple[zipfile.ZipFile, str, bytes]:
    if not path.is_file():
        raise SystemExit(f"source archive not found: {path}")
    raw_hash = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            raw_hash.update(chunk)
    zf = zipfile.ZipFile(path)
    try:
        version_raw = zf.read("src/version.json")
    except KeyError as exc:
        zf.close()
        raise SystemExit("source archive missing src/version.json") from exc
    return zf, raw_hash.hexdigest(), version_raw



def _simple_type_name(type_name: str) -> str:
    # Remove generic/array noise; keep the final identifier-like type token.
    clean = re.sub(r"<.*>", "", type_name)
    clean = clean.replace("[]", "").replace("?", "")
    names = re.findall(r"[A-Za-z_$][A-Za-z0-9_$]*", clean)
    return names[-1] if names else ""


def resolve_calls(conn: sqlite3.Connection) -> dict[str, int]:
    """Conservatively resolve call edges when the target is provable from local structure.

    v1 intentionally leaves ambiguous/local-variable dispatch unresolved rather than guessing.
    """
    type_rows = conn.execute("SELECT id,file_id,qualified_name,simple_name FROM types").fetchall()
    type_by_id = {int(r[0]): r for r in type_rows}
    simple_to_types: dict[str, list[int]] = defaultdict(list)
    qname_to_id: dict[str, int] = {}
    for row in type_rows:
        tid = int(row[0])
        simple_to_types[str(row[3])].append(tid)
        qname_to_id[str(row[2])] = tid

    imports_by_file: dict[int, dict[str, int]] = defaultdict(dict)
    for file_id, import_name in conn.execute("SELECT file_id,import_name FROM imports WHERE is_static=0"):
        simple = str(import_name).split('.')[-1]
        target = qname_to_id.get(str(import_name))
        if target is not None:
            imports_by_file[int(file_id)][simple] = target

    methods_by_key: dict[tuple[int, str, int], list[int]] = defaultdict(list)
    method_type: dict[int, int] = {}
    for mid, tid, name, argc in conn.execute("SELECT id,type_id,name,param_count FROM methods"):
        mid_i, tid_i = int(mid), int(tid)
        methods_by_key[(tid_i, str(name), int(argc))].append(mid_i)
        method_type[mid_i] = tid_i

    fields_by_type: dict[int, dict[str, str]] = defaultdict(dict)
    for tid, name, type_name in conn.execute("SELECT type_id,name,type_name FROM fields"):
        fields_by_type[int(tid)][str(name)] = str(type_name)

    stats = Counter()
    calls = conn.execute("SELECT id,caller_method_id,owner_text,callee_name,arg_count FROM method_calls").fetchall()
    for call_id, caller_id, owner_text, callee_name, argc in calls:
        caller_id = int(caller_id)
        caller_tid = method_type[caller_id]
        caller_row = type_by_id[caller_tid]
        file_id = int(caller_row[1])
        owner = str(owner_text) if owner_text is not None else None
        target_tid: int | None = None
        resolution: str | None = None

        if owner is None or owner in {"this", "super"}:
            target_tid = caller_tid
            resolution = "same-type"
        else:
            first = owner.split('.')[0]
            if first == "this":
                # this.field.method()
                parts = owner.split('.')
                if len(parts) >= 2:
                    field_type = fields_by_type[caller_tid].get(parts[1])
                    simple = _simple_type_name(field_type or "")
                    if simple in imports_by_file[file_id]:
                        target_tid = imports_by_file[file_id][simple]
                        resolution = "field-import"
                    elif len(simple_to_types.get(simple, [])) == 1:
                        target_tid = simple_to_types[simple][0]
                        resolution = "field-unique-type"
            elif first in fields_by_type[caller_tid]:
                simple = _simple_type_name(fields_by_type[caller_tid][first])
                if simple in imports_by_file[file_id]:
                    target_tid = imports_by_file[file_id][simple]
                    resolution = "field-import"
                elif len(simple_to_types.get(simple, [])) == 1:
                    target_tid = simple_to_types[simple][0]
                    resolution = "field-unique-type"
            elif first in imports_by_file[file_id]:
                target_tid = imports_by_file[file_id][first]
                resolution = "imported-type"
            elif len(simple_to_types.get(first, [])) == 1:
                target_tid = simple_to_types[first][0]
                resolution = "unique-simple-type"

        if target_tid is None:
            stats["unresolved"] += 1
            continue
        candidates = methods_by_key.get((target_tid, str(callee_name), int(argc)), [])
        if len(candidates) != 1:
            stats["ambiguous" if candidates else "unresolved"] += 1
            continue
        conn.execute(
            "UPDATE method_calls SET resolution=?,resolved_method_id=? WHERE id=?",
            (resolution, candidates[0], int(call_id)),
        )
        stats["resolved"] += 1
        stats[f"resolved_{resolution}"] += 1
    conn.commit()
    return dict(stats)


def index_archive(source: Path, db_path: Path, report_json: Path | None, report_md: Path | None) -> dict[str, object]:
    started = time.time()
    zf, archive_sha, version_raw = open_source(source)
    version = json.loads(version_raw)
    java_names = sorted(n for n in zf.namelist() if n.endswith(".java"))
    conn = init_db(db_path, replace=True)
    cur = conn.cursor()
    meta = {
        "schema_version": str(SCHEMA_VERSION),
        "atlas_version": ATLAS_VERSION,
        "minecraft_version": str(version.get("id", "")),
        "world_version": str(version.get("world_version", "")),
        "protocol_version": str(version.get("protocol_version", "")),
        "source_archive_sha256": archive_sha,
        "source_archive_name": source.name,
    }
    cur.executemany("INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)", meta.items())

    counters = Counter()
    package_counts: Counter[str] = Counter()
    hazard_counts: Counter[str] = Counter()
    method_hazard_score: Counter[str] = Counter()
    method_call_count: Counter[str] = Counter()
    method_field_writes: Counter[str] = Counter()

    for file_no, name in enumerate(java_names, 1):
        raw = zf.read(name)
        text = raw.decode("utf-8", errors="replace")
        tokens = tokenize_java(text)
        braces = matching_pairs(tokens, "{", "}")
        parens = matching_pairs(tokens, "(", ")")
        package, imports = package_and_imports(tokens)
        lines = text.count("\n") + (1 if text else 0)
        nonblank = sum(1 for line in text.splitlines() if line.strip())
        cur.execute(
            "INSERT INTO source_files(path,package,sha256,bytes,lines,nonblank_lines) VALUES(?,?,?,?,?,?)",
            (name, package, sha256_bytes(raw), len(raw), lines, nonblank),
        )
        file_id = int(cur.lastrowid)
        cur.executemany("INSERT INTO imports(file_id,import_name,is_static) VALUES(?,?,?)", ((file_id, imp, int(st)) for imp, st in imports))
        counters["files"] += 1
        counters["lines"] += lines
        counters["nonblank_lines"] += nonblank
        root_pkg = ".".join(package.split(".")[:4]) if package else "<default>"
        package_counts[root_pkg] += 1

        types = extract_types(tokens, package, braces)
        for typ in types:
            type_tokens = tokens[typ.start_token:typ.body_close + 1]
            cur.execute(
                "INSERT INTO types(file_id,qualified_name,simple_name,kind,modifiers,parent_qualified_name,start_line,end_line,normalized_sha256) VALUES(?,?,?,?,?,?,?,?,?)",
                (file_id, typ.qualified_name, typ.simple_name, typ.kind, typ.modifiers, typ.parent_qualified_name,
                 typ.start_line, typ.end_line, normalized_fingerprint(type_tokens)),
            )
            type_id = int(cur.lastrowid)
            counters["types"] += 1
            methods, fields = extract_members(tokens, typ, parens, braces)
            field_names = {f.name for f in fields}
            cur.executemany(
                "INSERT OR IGNORE INTO fields(type_id,name,type_name,modifiers,line) VALUES(?,?,?,?,?)",
                ((type_id, f.name, f.type_name, f.modifiers, f.line) for f in fields),
            )
            counters["fields"] += len(fields)

            for method in methods:
                if method.body_open is not None and method.body_close is not None:
                    body_tokens = tokens[method.body_open + 1:method.body_close]
                    body_exact = join_tokens(body_tokens).encode()
                    body_sha = sha256_bytes(body_exact)
                    norm_sha = normalized_fingerprint(body_tokens)
                else:
                    body_tokens = []
                    body_sha = sha256_bytes(b"")
                    norm_sha = body_sha
                cur.execute(
                    "INSERT INTO methods(type_id,name,signature,return_type,modifiers,is_constructor,param_count,start_line,end_line,body_sha256,normalized_sha256) VALUES(?,?,?,?,?,?,?,?,?,?,?)",
                    (type_id, method.name, method.signature, method.return_type, method.modifiers, int(method.is_constructor), method.param_count,
                     method.start_line, method.end_line, body_sha, norm_sha),
                )
                method_id = int(cur.lastrowid)
                counters["methods"] += 1
                key = f"{typ.qualified_name}#{method.signature}"

                calls = syntactic_calls(tokens, method)
                cur.executemany(
                    "INSERT INTO method_calls(caller_method_id,owner_text,callee_name,arg_count,line) VALUES(?,?,?,?,?)",
                    ((method_id, owner, callee, argc, line) for owner, callee, argc, line in calls),
                )
                counters["calls"] += len(calls)
                method_call_count[key] += len(calls)

                accesses = field_accesses(tokens, method, field_names)
                cur.executemany(
                    "INSERT INTO field_accesses(method_id,field_name,mode,line) VALUES(?,?,?,?)",
                    ((method_id, field, mode, line) for field, mode, line in accesses),
                )
                counters["field_accesses"] += len(accesses)
                writes = sum(1 for _, mode, _ in accesses if mode in {"write", "read_write"})
                method_field_writes[key] += writes

                hazards = hazards_for(tokens, method, package, typ.simple_name)
                cur.executemany(
                    "INSERT OR IGNORE INTO hazards(method_id,kind,detail,line) VALUES(?,?,?,?)",
                    ((method_id, kind, detail, line) for kind, detail, line in hazards),
                )
                for kind, _, _ in hazards:
                    hazard_counts[kind] += 1
                method_hazard_score[key] += len({kind for kind, _, _ in hazards})

                classes = classifications_for(package, typ.simple_name, method, hazards)
                cur.executemany(
                    "INSERT OR IGNORE INTO classifications(method_id,label,confidence,source,reason) VALUES(?,?,?,?,?)",
                    ((method_id, label, confidence, "heuristic", reason) for label, confidence, reason in classes),
                )
                counters["classifications"] += len(classes)
                cur.execute("INSERT INTO tracking(method_id) VALUES(?)", (method_id,))

        if file_no % 250 == 0:
            conn.commit()
            print(f"indexed {file_no}/{len(java_names)} Java files", file=sys.stderr)

    conn.commit()
    resolution_stats = resolve_calls(conn)
    zf.close()

    def top(counter: Counter[str], n: int = 20) -> list[dict[str, object]]:
        return [{"symbol": symbol, "value": value} for symbol, value in counter.most_common(n) if value]

    report: dict[str, object] = {
        "atlas_version": ATLAS_VERSION,
        "schema_version": SCHEMA_VERSION,
        "source": {
            "minecraft_version": version.get("id"),
            "world_version": version.get("world_version"),
            "protocol_version": version.get("protocol_version"),
            "archive_sha256": archive_sha,
            "java_files": counters["files"],
            "java_lines": counters["lines"],
            "java_nonblank_lines": counters["nonblank_lines"],
        },
        "index": {
            "types": counters["types"],
            "fields": counters["fields"],
            "methods": counters["methods"],
            "syntactic_calls": counters["calls"],
            "field_accesses": counters["field_accesses"],
            "heuristic_classifications": counters["classifications"],
            "call_resolution": resolution_stats,
        },
        "hazards": dict(sorted(hazard_counts.items())),
        "top_package_roots": [{"package": k, "java_files": v} for k, v in package_counts.most_common(30)],
        "top_hazard_methods": top(method_hazard_score),
        "top_call_sites": top(method_call_count),
        "top_field_mutators": top(method_field_writes),
        "notes": [
            "Call sites are recorded syntactically; only conservatively provable targets receive resolved_method_id in schema v1.",
            "Heuristic classifications are candidates and never assert IMPLEMENTATION_ONLY.",
            "The generated SQLite database contains fingerprints and structural metadata, not source bodies.",
        ],
    }

    if report_json:
        report_json.parent.mkdir(parents=True, exist_ok=True)
        report_json.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if report_md:
        report_md.parent.mkdir(parents=True, exist_ok=True)
        report_md.write_text(render_markdown_report(report), encoding="utf-8")
    print(f"index completed in {time.time() - started:.3f}s", file=sys.stderr)
    print(json.dumps(report, indent=2))
    conn.close()
    return report


def render_markdown_report(report: dict[str, object]) -> str:
    src = report["source"]
    idx = report["index"]
    hazards = report["hazards"]
    assert isinstance(src, dict) and isinstance(idx, dict) and isinstance(hazards, dict)
    lines = [
        "# Minecraft 26.2 Official Source Structural Audit",
        "",
        "**Generated by:** Vanilla Atlas 0.1",
        "",
        "> This is a structural inventory, not a claim that every method's semantics have been reviewed.",
        "",
        "## Source pin",
        "",
        f"- Minecraft: **{src['minecraft_version']}**",
        f"- protocol: **{src['protocol_version']}**",
        f"- world/data version: **{src['world_version']}**",
        f"- source archive SHA-256: `{src['archive_sha256']}`",
        f"- Java files: **{src['java_files']:,}**",
        f"- Java lines: **{src['java_lines']:,}**",
        f"- nonblank Java lines: **{src['java_nonblank_lines']:,}**",
        "",
        "## Structural index",
        "",
        f"- types: **{idx['types']:,}**",
        f"- fields: **{idx['fields']:,}**",
        f"- methods/constructors: **{idx['methods']:,}**",
        f"- syntactic call sites: **{idx['syntactic_calls']:,}**",
        f"- owner-field accesses: **{idx['field_accesses']:,}**",
        f"- heuristic classification labels: **{idx['heuristic_classifications']:,}**",
        f"- conservatively resolved call edges: **{idx['call_resolution'].get('resolved', 0):,}**",
        "",
        "## Hazard sites",
        "",
        "| Hazard | Distinct method/detail observations |",
        "| --- | ---: |",
    ]
    for key, value in sorted(hazards.items(), key=lambda x: (-x[1], x[0])):
        lines.append(f"| `{key}` | {value:,} |")
    lines += [
        "",
        "## Interpretation",
        "",
        "The Atlas deliberately separates **discovery** from **semantic review**. A hazard is a review lead, not proof that the corresponding Java implementation detail is part of Crucible's semantic contract. Calls are syntactic in schema v1 and are upgraded to resolved edges only when later passes can prove the target.",
        "",
        "The generated SQLite database is disposable local state. Human-reviewed VAR/SEM/EQUIV records remain version-controlled artifacts and will reference stable source/method fingerprints from this index.",
        "",
        "## Highest-hazard methods",
        "",
        "| Method | Hazard classes |",
        "| --- | ---: |",
    ]
    for row in report["top_hazard_methods"][:20]:
        lines.append(f"| `{row['symbol']}` | {row['value']} |")
    lines += ["", ""]
    return "\n".join(lines)


def connect_db(path: Path) -> sqlite3.Connection:
    if not path.is_file():
        raise SystemExit(f"Atlas database not found: {path}; run `vanilla index` first")
    conn = sqlite3.connect(path)
    conn.row_factory = sqlite3.Row
    return conn


def cmd_status(db_path: Path) -> int:
    conn = connect_db(db_path)
    meta = dict(conn.execute("SELECT key,value FROM meta"))
    counts = {
        "files": conn.execute("SELECT COUNT(*) FROM source_files").fetchone()[0],
        "types": conn.execute("SELECT COUNT(*) FROM types").fetchone()[0],
        "methods": conn.execute("SELECT COUNT(*) FROM methods").fetchone()[0],
        "reviewed": conn.execute("SELECT COUNT(*) FROM tracking WHERE review_status <> 'UNSEEN'").fetchone()[0],
        "hazards": conn.execute("SELECT COUNT(*) FROM hazards").fetchone()[0],
    }
    print(f"Vanilla Atlas {meta.get('atlas_version', '?')} schema {meta.get('schema_version', '?')}")
    print(f"Minecraft {meta.get('minecraft_version')} protocol {meta.get('protocol_version')} world {meta.get('world_version')}")
    print(f"source {meta.get('source_archive_sha256')}")
    print(f"files={counts['files']} types={counts['types']} methods={counts['methods']} reviewed={counts['reviewed']} hazards={counts['hazards']}")
    return 0


def resolve_methods(conn: sqlite3.Connection, query: str) -> list[sqlite3.Row]:
    # Exact qualified type#method/signature, then suffix/substring fallback.
    rows = conn.execute(
        """SELECT m.id, t.qualified_name, m.name, m.signature, m.start_line, m.end_line, f.path
           FROM methods m JOIN types t ON t.id=m.type_id JOIN source_files f ON f.id=t.file_id
           WHERE t.qualified_name || '#' || m.signature = ?
              OR t.qualified_name || '#' || m.name = ?
           ORDER BY t.qualified_name,m.start_line""",
        (query, query),
    ).fetchall()
    if rows:
        return rows
    needle = f"%{query}%"
    return conn.execute(
        """SELECT m.id, t.qualified_name, m.name, m.signature, m.start_line, m.end_line, f.path
           FROM methods m JOIN types t ON t.id=m.type_id JOIN source_files f ON f.id=t.file_id
           WHERE t.qualified_name LIKE ? OR m.signature LIKE ? OR (t.qualified_name || '#' || m.signature) LIKE ?
           ORDER BY t.qualified_name,m.start_line LIMIT 100""",
        (needle, needle, needle),
    ).fetchall()


def cmd_show(db_path: Path, query: str) -> int:
    conn = connect_db(db_path)
    rows = resolve_methods(conn, query)
    if not rows:
        print(f"no Atlas method matched: {query}", file=sys.stderr)
        return 1
    for row in rows:
        print(f"{row['qualified_name']}#{row['signature']}  {row['path']}:{row['start_line']}-{row['end_line']}")
        hazards = conn.execute("SELECT kind,detail,line FROM hazards WHERE method_id=? ORDER BY kind,detail", (row['id'],)).fetchall()
        classes = conn.execute("SELECT label,confidence,source,reason FROM classifications WHERE method_id=? ORDER BY label", (row['id'],)).fetchall()
        tracking = conn.execute("SELECT review_status,var_id,notes FROM tracking WHERE method_id=?", (row['id'],)).fetchone()
        if tracking:
            print(f"  review: {tracking['review_status']}  VAR={tracking['var_id'] or '-'}")
        if classes:
            print("  classifications:")
            for c in classes:
                print(f"    {c['label']} confidence={c['confidence']:.2f} ({c['source']}: {c['reason']})")
        if hazards:
            print("  hazards:")
            for h in hazards:
                print(f"    {h['kind']}: {h['detail']} @ line {h['line']}")
    return 0


def cmd_deps(db_path: Path, query: str) -> int:
    conn = connect_db(db_path)
    rows = resolve_methods(conn, query)
    if not rows:
        print(f"no Atlas method matched: {query}", file=sys.stderr)
        return 1
    if len(rows) > 10:
        print(f"query matched {len(rows)} methods; refine it", file=sys.stderr)
        return 2
    for row in rows:
        print(f"{row['qualified_name']}#{row['signature']}")
        calls = conn.execute(
            "SELECT owner_text,callee_name,arg_count,line,resolution FROM method_calls WHERE caller_method_id=? ORDER BY line,id",
            (row['id'],),
        ).fetchall()
        for c in calls:
            owner = f"{c['owner_text']}." if c['owner_text'] else ""
            print(f"  -> {owner}{c['callee_name']}/{c['arg_count']} @ {c['line']} [{c['resolution']}]")
    return 0


def cmd_hazards(db_path: Path, kind: str | None, limit: int) -> int:
    conn = connect_db(db_path)
    if kind:
        rows = conn.execute(
            """SELECT h.kind,h.detail,h.line,t.qualified_name,m.signature,f.path
               FROM hazards h JOIN methods m ON m.id=h.method_id JOIN types t ON t.id=m.type_id JOIN source_files f ON f.id=t.file_id
               WHERE h.kind=? ORDER BY t.qualified_name,m.start_line LIMIT ?""",
            (kind, limit),
        ).fetchall()
    else:
        rows = conn.execute(
            """SELECT h.kind,h.detail,h.line,t.qualified_name,m.signature,f.path
               FROM hazards h JOIN methods m ON m.id=h.method_id JOIN types t ON t.id=m.type_id JOIN source_files f ON f.id=t.file_id
               ORDER BY h.kind,t.qualified_name,m.start_line LIMIT ?""",
            (limit,),
        ).fetchall()
    for r in rows:
        print(f"{r['kind']:28} {r['qualified_name']}#{r['signature']} {r['path']}:{r['line']} [{r['detail']}]")
    return 0


def cmd_coverage(db_path: Path) -> int:
    conn = connect_db(db_path)
    total = conn.execute("SELECT COUNT(*) FROM methods").fetchone()[0]
    statuses = conn.execute("SELECT review_status,COUNT(*) AS n FROM tracking GROUP BY review_status ORDER BY n DESC").fetchall()
    print(f"methods: {total}")
    for status, count in statuses:
        pct = (count * 100.0 / total) if total else 0.0
        print(f"  {status:24} {count:7d} {pct:6.2f}%")
    print("classifications:")
    for label, count in conn.execute("SELECT label,COUNT(DISTINCT method_id) FROM classifications GROUP BY label ORDER BY 2 DESC,label"):
        print(f"  {label:32} {count:7d}")
    return 0



def cmd_verify_source(source: Path, lock_path: Path) -> int:
    if not lock_path.is_file():
        print(f"vanilla lock not found: {lock_path}", file=sys.stderr)
        return 1
    lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    zf, archive_sha, version_raw = open_source(source)
    version = json.loads(version_raw)
    java_files = sum(1 for name in zf.namelist() if name.endswith(".java"))
    zf.close()
    checks = {
        "minecraft": (str(version.get("id")), str(lock.get("minecraft"))),
        "protocol": (str(version.get("protocol_version")), str(lock.get("protocol"))),
        "data_version": (str(version.get("world_version")), str(lock.get("data_version"))),
        "archive_sha256": (archive_sha, str(lock.get("source", {}).get("archive_sha256"))),
        "java_files": (str(java_files), str(lock.get("source", {}).get("java_files"))),
    }
    ok = True
    for label, (actual, expected) in checks.items():
        matched = actual == expected
        print(f"{label:16} {'OK' if matched else 'MISMATCH':8} actual={actual} expected={expected}")
        ok &= matched
    return 0 if ok else 2


def cmd_callers(db_path: Path, query: str, limit: int) -> int:
    conn = connect_db(db_path)
    rows = resolve_methods(conn, query)
    if not rows:
        print(f"no Atlas method matched: {query}", file=sys.stderr)
        return 1
    target_ids = [int(r['id']) for r in rows[:20]]
    for row in rows[:20]:
        print(f"{row['qualified_name']}#{row['signature']}")
        callers = conn.execute(
            """SELECT t.qualified_name,m.signature,c.line,c.resolution
               FROM method_calls c
               JOIN methods m ON m.id=c.caller_method_id
               JOIN types t ON t.id=m.type_id
               WHERE c.resolved_method_id=? ORDER BY t.qualified_name,m.start_line LIMIT ?""",
            (row['id'], limit),
        ).fetchall()
        if not callers:
            print("  (no conservatively resolved callers)")
        for c in callers:
            print(f"  <- {c['qualified_name']}#{c['signature']} @ {c['line']} [{c['resolution']}]")
    return 0


def frontier_config_path(name: str) -> Path:
    return Path("vanilla/frontiers") / f"{name}.json"


def _method_status_counts(conn: sqlite3.Connection, method_ids: set[int]) -> Counter[str]:
    if not method_ids:
        return Counter()
    counts: Counter[str] = Counter()
    ids = sorted(method_ids)
    for offset in range(0, len(ids), 800):
        chunk = ids[offset:offset+800]
        qs = ",".join("?" for _ in chunk)
        for status, n in conn.execute(f"SELECT review_status,COUNT(*) FROM tracking WHERE method_id IN ({qs}) GROUP BY review_status", chunk):
            counts[str(status)] += int(n)
    return counts


def cmd_frontier(db_path: Path, name: str, config_path: Path | None, json_out: bool) -> int:
    path = config_path or frontier_config_path(name)
    if not path.is_file():
        print(f"frontier config not found: {path}", file=sys.stderr)
        return 1
    config = json.loads(path.read_text(encoding="utf-8"))
    conn = connect_db(db_path)
    root_labels = [str(q) for q in config.get("root_queries", [])]
    roots, seen = compute_frontier_method_ids(conn, config)
    if not roots:
        print("frontier has no resolved roots", file=sys.stderr)
        return 2
    max_depth = int(config.get("max_depth", 12))
    # Depth sizes are diagnostic only; exact closure is produced by the shared helper.
    depth_counts: list[int] = [len(roots), len(seen) - len(roots)]

    statuses = _method_status_counts(conn, seen)
    hazards: Counter[str] = Counter()
    unresolved: Counter[str] = Counter()
    ids = sorted(seen)
    for offset in range(0, len(ids), 700):
        chunk = ids[offset:offset+700]
        qs = ",".join("?" for _ in chunk)
        for kind, n in conn.execute(f"SELECT kind,COUNT(*) FROM hazards WHERE method_id IN ({qs}) GROUP BY kind", chunk):
            hazards[str(kind)] += int(n)
        for callee, n in conn.execute(
            f"SELECT callee_name,COUNT(*) FROM method_calls WHERE caller_method_id IN ({qs}) AND resolved_method_id IS NULL GROUP BY callee_name",
            chunk,
        ):
            unresolved[str(callee)] += int(n)

    report = {
        "frontier": name,
        "description": config.get("description", ""),
        "roots": root_labels,
        "root_methods": len(roots),
        "reachable_methods": len(seen),
        "max_depth": max_depth,
        "depth_new_methods": depth_counts,
        "review_status": dict(sorted(statuses.items())),
        "hazards": dict(sorted(hazards.items())),
        "unresolved_call_sites": sum(unresolved.values()),
        "top_unresolved_callees": [{"callee": k, "sites": v} for k, v in unresolved.most_common(30)],
    }
    if json_out:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(f"Frontier: {name}")
        if report['description']:
            print(report['description'])
        print(f"roots={report['root_methods']} reachable_methods={report['reachable_methods']} max_depth={max_depth}")
        print("review status:")
        for status, n in statuses.most_common():
            print(f"  {status:24} {n:7d}")
        print("hazards:")
        for kind, n in hazards.most_common():
            print(f"  {kind:28} {n:7d}")
        print(f"unresolved call sites inside frontier: {report['unresolved_call_sites']}")
        for row in report['top_unresolved_callees'][:15]:
            print(f"  ? {row['callee']:32} {row['sites']:6d}")
    return 0



def cmd_record_template(db_path: Path, query: str, var_id: str) -> int:
    conn = connect_db(db_path)
    rows = resolve_methods(conn, query)
    if len(rows) != 1:
        print(f"record-template requires exactly one method match; got {len(rows)}", file=sys.stderr)
        for row in rows[:20]:
            print(f"  {row['qualified_name']}#{row['signature']}", file=sys.stderr)
        return 2
    row = rows[0]
    fp = conn.execute("SELECT normalized_sha256 FROM methods WHERE id=?", (row['id'],)).fetchone()[0]
    hazards = [r[0] for r in conn.execute("SELECT DISTINCT kind FROM hazards WHERE method_id=? ORDER BY kind", (row['id'],))]
    classes = [r[0] for r in conn.execute("SELECT label FROM classifications WHERE method_id=? AND source='heuristic' ORDER BY label", (row['id'],))]
    record = {
        "schema": 1,
        "id": var_id,
        "status": "INDEXED",
        "source": {
            "type": row['qualified_name'],
            "signature": row['signature'],
            "normalized_sha256": fp,
        },
        "classifications": classes,
        "hazards_reviewed": [],
        "semantic_rules": [],
        "evidence": [],
        "notes": [],
        "atlas_observed_hazards": hazards,
    }
    print(json.dumps(record, indent=2, sort_keys=True))
    return 0


def _load_review_records(records_dir: Path) -> list[tuple[Path, dict[str, object]]]:
    if not records_dir.exists():
        return []
    records: list[tuple[Path, dict[str, object]]] = []
    for path in sorted(records_dir.rglob("*.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(data, dict) or data.get("schema") != 1 or not data.get("id"):
            raise SystemExit(f"invalid Vanilla Atlas record: {path}")
        records.append((path, data))
    return records


def cmd_sync_records(db_path: Path, records_dir: Path) -> int:
    conn = connect_db(db_path)
    records = _load_review_records(records_dir)
    stale = 0
    applied = 0
    for path, record in records:
        source = record.get("source")
        if not isinstance(source, dict):
            print(f"record missing source object: {path}", file=sys.stderr)
            return 2
        qname = str(source.get("type", ""))
        signature = str(source.get("signature", ""))
        expected_fp = str(source.get("normalized_sha256", ""))
        row = conn.execute(
            """SELECT m.id,m.normalized_sha256 FROM methods m JOIN types t ON t.id=m.type_id
               WHERE t.qualified_name=? AND m.signature=?""",
            (qname, signature),
        ).fetchone()
        if row is None:
            print(f"STALE {record['id']}: source method no longer exists ({qname}#{signature})")
            stale += 1
            continue
        method_id = int(row[0])
        actual_fp = str(row[1])
        requested_status = str(record.get("status", "INDEXED"))
        if requested_status not in REVIEW_RANK:
            print(f"invalid review status in {path}: {requested_status}", file=sys.stderr)
            return 2
        status = requested_status
        if expected_fp and expected_fp != actual_fp:
            status = "STALE"
            stale += 1
            print(f"STALE {record['id']}: fingerprint changed")
        conn.execute(
            "UPDATE tracking SET review_status=?,var_id=?,notes=? WHERE method_id=?",
            (status, str(record['id']), json.dumps(record.get("notes", []), separators=(",", ":")), method_id),
        )
        for label in record.get("classifications", []):
            conn.execute(
                "INSERT OR REPLACE INTO classifications(method_id,label,confidence,source,reason) VALUES(?,?,?,?,?)",
                (method_id, str(label), 1.0, "manual", str(record['id'])),
            )
        for sem_id in record.get("semantic_rules", []):
            conn.execute(
                "INSERT INTO semantic_edges(method_id,var_id,sem_id,relation) VALUES(?,?,?,?)",
                (method_id, str(record['id']), str(sem_id), "VAR_SUPPORTS_SEM"),
            )
        for evidence_id in record.get("evidence", []):
            conn.execute(
                "INSERT INTO semantic_edges(method_id,var_id,evidence_id,relation) VALUES(?,?,?,?)",
                (method_id, str(record['id']), str(evidence_id), "VAR_HAS_EVIDENCE"),
            )
        applied += 1
    conn.commit()
    print(f"synced {applied} review records; stale={stale}")
    return 2 if stale else 0


def cmd_stale(db_path: Path) -> int:
    conn = connect_db(db_path)
    rows = conn.execute(
        """SELECT t.qualified_name,m.signature,tr.var_id,tr.notes FROM tracking tr
           JOIN methods m ON m.id=tr.method_id JOIN types t ON t.id=m.type_id
           WHERE tr.review_status='STALE' ORDER BY tr.var_id,t.qualified_name"""
    ).fetchall()
    if not rows:
        print("no stale tracked source records")
        return 0
    for row in rows:
        print(f"{row['var_id'] or '-'}  {row['qualified_name']}#{row['signature']}")
    return 2


def compute_frontier_method_ids(conn: sqlite3.Connection, config: dict[str, object]) -> tuple[set[int], set[int]]:
    roots: set[int] = set()
    for query in config.get("root_queries", []):
        roots.update(int(r['id']) for r in resolve_methods(conn, str(query)))
    max_depth = int(config.get("max_depth", 12))
    exclude = tuple(str(x) for x in config.get("exclude_package_prefixes", []))
    include = tuple(str(x) for x in config.get("include_package_prefixes", []))
    method_qname = {int(mid): str(qname) for mid, qname in conn.execute("SELECT m.id,t.qualified_name FROM methods m JOIN types t ON t.id=m.type_id")}
    seen = set(roots)
    current = set(roots)
    for _ in range(max_depth):
        if not current:
            break
        nxt: set[int] = set()
        ids = sorted(current)
        for offset in range(0, len(ids), 700):
            chunk = ids[offset:offset+700]
            qs = ",".join("?" for _ in chunk)
            for (target,) in conn.execute(f"SELECT DISTINCT resolved_method_id FROM method_calls WHERE caller_method_id IN ({qs}) AND resolved_method_id IS NOT NULL", chunk):
                tid = int(target)
                if tid in seen:
                    continue
                qname = method_qname.get(tid, "")
                if exclude and qname.startswith(exclude):
                    continue
                if include and not qname.startswith(include):
                    continue
                nxt.add(tid)
        seen.update(nxt)
        current = nxt
    return roots, seen


def cmd_next(db_path: Path, frontier_name: str, config_path: Path | None, limit: int) -> int:
    path = config_path or frontier_config_path(frontier_name)
    if not path.is_file():
        print(f"frontier config not found: {path}", file=sys.stderr)
        return 1
    config = json.loads(path.read_text(encoding="utf-8"))
    conn = connect_db(db_path)
    roots, ids = compute_frontier_method_ids(conn, config)
    if not ids:
        print("frontier is empty", file=sys.stderr)
        return 2
    scored: list[tuple[float, sqlite3.Row]] = []
    for offset in range(0, len(ids), 700):
        chunk = sorted(ids)[offset:offset+700]
        qs = ",".join("?" for _ in chunk)
        rows = conn.execute(
            f"""SELECT m.id,t.qualified_name,m.signature,tr.review_status,
                       (SELECT COUNT(DISTINCT kind) FROM hazards h WHERE h.method_id=m.id) AS hazard_kinds,
                       (SELECT COUNT(*) FROM field_accesses fa WHERE fa.method_id=m.id AND fa.mode<>'read') AS writes,
                       (SELECT COUNT(*) FROM method_calls c WHERE c.resolved_method_id=m.id) AS callers,
                       (SELECT COUNT(*) FROM method_calls c WHERE c.caller_method_id=m.id AND c.resolved_method_id IS NULL) AS unresolved
                FROM methods m JOIN types t ON t.id=m.type_id JOIN tracking tr ON tr.method_id=m.id
                WHERE m.id IN ({qs})""",
            chunk,
        ).fetchall()
        for row in rows:
            status_rank = REVIEW_RANK.get(str(row['review_status']), 0)
            if str(row['review_status']) in {"INTEGRATED", "PERFORMANCE_QUALIFIED"}:
                continue
            # Prefer unreviewed high-consequence/high-connectivity methods. Later statuses naturally sink.
            score = (
                12.0 * int(row['hazard_kinds'])
                + 1.5 * min(int(row['callers']), 20)
                + 1.0 * min(int(row['writes']), 20)
                + 0.15 * min(int(row['unresolved']), 40)
                - 8.0 * status_rank
                + (30.0 if int(row['id']) in roots else 0.0)
            )
            scored.append((score, row))
    scored.sort(key=lambda x: (-x[0], x[1]['qualified_name'], x[1]['signature']))
    print(f"Next source-review candidates for frontier `{frontier_name}`:")
    for score, row in scored[:limit]:
        print(
            f"{score:6.1f}  {row['review_status']:18}  hazards={row['hazard_kinds']} callers={row['callers']} writes={row['writes']} unresolved={row['unresolved']}  "
            f"{row['qualified_name']}#{row['signature']}"
        )
    return 0


def default_db() -> Path:
    return Path(os.environ.get("CRUCIBLE_ATLAS_DB", ".crucible/vanilla/atlas.sqlite"))


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="vanilla-atlas")
    p.add_argument("--db", type=Path, default=default_db(), help="generated SQLite index")
    sub = p.add_subparsers(dest="command", required=True)
    idx = sub.add_parser("index", help="build a complete structural index from the official source ZIP")
    idx.add_argument("source", type=Path)
    idx.add_argument("--report-json", type=Path)
    idx.add_argument("--report-md", type=Path)
    verify = sub.add_parser("verify-source", help="verify a local official source archive against vanilla.lock.toml")
    verify.add_argument("source", type=Path)
    verify.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    sub.add_parser("status", help="print source/index identity and counts")
    show = sub.add_parser("show", help="show tracked metadata for a method/type query")
    show.add_argument("query")
    deps = sub.add_parser("deps", help="show outgoing syntactic call edges")
    deps.add_argument("query")
    callers = sub.add_parser("callers", help="show conservatively resolved incoming call edges")
    callers.add_argument("query")
    callers.add_argument("--limit", type=int, default=100)
    haz = sub.add_parser("hazards", help="list source review hazards")
    haz.add_argument("kind", nargs="?")
    haz.add_argument("--limit", type=int, default=100)
    sub.add_parser("coverage", help="show semantic-review tracking coverage")
    template = sub.add_parser("record-template", help="emit a fingerprint-pinned machine-readable VAR tracking record")
    template.add_argument("query")
    template.add_argument("--id", required=True)
    sync = sub.add_parser("sync-records", help="apply version-controlled source review records to the generated index")
    sync.add_argument("--records", type=Path, default=Path("vanilla/records"))
    sub.add_parser("stale", help="list source records invalidated by fingerprint/source changes")
    nxt = sub.add_parser("next", help="rank the next source-review candidates within a frontier")
    nxt.add_argument("frontier")
    nxt.add_argument("--config", type=Path)
    nxt.add_argument("--limit", type=int, default=25)
    frontier = sub.add_parser("frontier", help="compute a milestone/subsystem dependency frontier")
    frontier.add_argument("name")
    frontier.add_argument("--config", type=Path)
    frontier.add_argument("--json", action="store_true")
    return p


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.command == "index":
        index_archive(args.source, args.db, args.report_json, args.report_md)
        return 0
    if args.command == "verify-source":
        return cmd_verify_source(args.source, args.lock)
    if args.command == "status":
        return cmd_status(args.db)
    if args.command == "show":
        return cmd_show(args.db, args.query)
    if args.command == "deps":
        return cmd_deps(args.db, args.query)
    if args.command == "callers":
        return cmd_callers(args.db, args.query, args.limit)
    if args.command == "hazards":
        return cmd_hazards(args.db, args.kind, args.limit)
    if args.command == "coverage":
        return cmd_coverage(args.db)
    if args.command == "record-template":
        return cmd_record_template(args.db, args.query, args.id)
    if args.command == "sync-records":
        return cmd_sync_records(args.db, args.records)
    if args.command == "stale":
        return cmd_stale(args.db)
    if args.command == "next":
        return cmd_next(args.db, args.frontier, args.config, args.limit)
    if args.command == "frontier":
        return cmd_frontier(args.db, args.name, args.config, args.json)
    raise AssertionError(args.command)


if __name__ == "__main__":
    raise SystemExit(main())
