#!/usr/bin/env python3
"""Admit a narrow source-backed Crucible slice from Vanilla Atlas evidence.

The gate is deliberately read-only. It binds an explicit set of source methods to
version-controlled VAR records and the current generated Atlas index, then fails
closed when a required method is missing, ambiguous, stale, under-reviewed,
outside its declared frontier, insufficiently hazard-reviewed, or disconnected
from a Crucible SEM rule.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any, Sequence

import vanilla_atlas as atlas

SCHEMA = 1


class GateError(RuntimeError):
    """Raised for malformed gate configuration rather than failed evidence."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise GateError(f"{label} must be a JSON object")
    return value


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise GateError(f"{label} must be a non-empty string")
    return value


def _bool(value: object, label: str) -> bool:
    if type(value) is not bool:
        raise GateError(f"{label} must be a boolean")
    return value


def load_gate(path: Path) -> tuple[dict[str, Any], bytes]:
    if path.is_symlink() or not path.is_file():
        raise GateError(f"gate must be a real non-symlink file: {path}")
    raw = path.read_bytes()
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as error:
        raise GateError(f"invalid gate JSON: {error}") from error
    gate = _object(data, "gate")
    allowed = {
        "schema",
        "id",
        "frontier",
        "minimum_status",
        "require_semantic_rules",
        "require_hazards_reviewed",
        "methods",
    }
    unknown = sorted(set(gate) - allowed)
    required = {
        "schema",
        "id",
        "minimum_status",
        "require_semantic_rules",
        "require_hazards_reviewed",
        "methods",
    }
    missing = sorted(required - set(gate))
    if unknown:
        raise GateError(f"gate contains unknown keys: {', '.join(unknown)}")
    if missing:
        raise GateError(f"gate is missing required keys: {', '.join(missing)}")
    if type(gate["schema"]) is not int or gate["schema"] != SCHEMA:
        raise GateError(f"unsupported gate schema: {gate['schema']!r}")
    _string(gate["id"], "gate.id")
    minimum = _string(gate["minimum_status"], "gate.minimum_status")
    if minimum not in atlas.REVIEW_RANK or minimum == "STALE":
        raise GateError(f"invalid minimum_status: {minimum!r}")
    _bool(gate["require_semantic_rules"], "gate.require_semantic_rules")
    _bool(gate["require_hazards_reviewed"], "gate.require_hazards_reviewed")
    if "frontier" in gate:
        _string(gate["frontier"], "gate.frontier")
    methods = gate["methods"]
    if not isinstance(methods, list) or not methods:
        raise GateError("gate.methods must be a non-empty array")
    seen_queries: set[str] = set()
    seen_vars: set[str] = set()
    for index, raw_method in enumerate(methods):
        method = _object(raw_method, f"gate.methods[{index}]")
        if set(method) != {"query", "var_id"}:
            raise GateError(
                f"gate.methods[{index}] must contain exactly query and var_id"
            )
        query = _string(method["query"], f"gate.methods[{index}].query")
        var_id = _string(method["var_id"], f"gate.methods[{index}].var_id")
        if query in seen_queries:
            raise GateError(f"duplicate required source query: {query}")
        if var_id in seen_vars:
            raise GateError(f"duplicate required VAR id: {var_id}")
        seen_queries.add(query)
        seen_vars.add(var_id)
    return gate, raw


def _records_by_id(records_dir: Path) -> dict[str, tuple[Path, dict[str, object]]]:
    return {
        str(record["id"]): (path, record)
        for path, record in atlas._load_review_records(records_dir)
    }


def _status_admitted(status: str, minimum: str) -> bool:
    if status == "STALE" or status not in atlas.REVIEW_RANK:
        return False
    return atlas.REVIEW_RANK[status] >= atlas.REVIEW_RANK[minimum]


def _require_each_frontier_root(
    conn: Any,
    config: dict[str, Any],
    frontier_name: str,
) -> None:
    root_queries = config.get("root_queries")
    if not isinstance(root_queries, list) or not root_queries:
        raise GateError(f"frontier {frontier_name} must declare non-empty root_queries")
    for index, value in enumerate(root_queries):
        if not isinstance(value, str) or not value:
            raise GateError(
                f"frontier {frontier_name} root_queries[{index}] must be a non-empty string"
            )
        if not atlas.resolve_methods(conn, value):
            raise GateError(
                f"frontier {frontier_name} root query resolved zero methods: {value}"
            )


def _frontier_identity(
    conn: Any,
    frontier_name: str | None,
) -> tuple[set[int] | None, dict[str, object] | None]:
    if frontier_name is None:
        return None, None
    path = atlas.frontier_config_path(frontier_name)
    if path.is_symlink() or not path.is_file():
        raise GateError(f"frontier config not found: {path}")
    raw = path.read_bytes()
    config = _object(json.loads(raw), f"frontier {frontier_name}")
    _require_each_frontier_root(conn, config, frontier_name)
    roots, reachable = atlas.compute_frontier_method_ids(conn, config)
    if not roots:
        raise GateError(f"frontier {frontier_name} has no resolved roots")
    return reachable, {
        "name": frontier_name,
        "config_path": str(path),
        "config_sha256": sha256_bytes(raw),
        "root_methods": len(roots),
        "reachable_methods": len(reachable),
    }


def evaluate(
    *,
    db_path: Path,
    gate_path: Path,
    records_dir: Path,
) -> dict[str, object]:
    gate, gate_raw = load_gate(gate_path)
    conn = atlas.connect_db(db_path)
    meta = dict(conn.execute("SELECT key,value FROM meta"))
    records = _records_by_id(records_dir)
    minimum = str(gate["minimum_status"])
    require_semantic_rules = bool(gate["require_semantic_rules"])
    require_hazards_reviewed = bool(gate["require_hazards_reviewed"])
    frontier_name = str(gate["frontier"]) if gate.get("frontier") else None
    frontier_ids, frontier_report = _frontier_identity(conn, frontier_name)

    failures: list[str] = []
    admitted_methods: list[dict[str, object]] = []

    for required in gate["methods"]:
        assert isinstance(required, dict)
        query = str(required["query"])
        var_id = str(required["var_id"])
        rows = atlas.resolve_methods(conn, query)
        if len(rows) != 1:
            failures.append(
                f"{var_id}: required query must resolve exactly once; {query!r} matched {len(rows)}"
            )
            continue
        row = rows[0]
        method_id = int(row["id"])
        qualified_name = str(row["qualified_name"])
        signature = str(row["signature"])
        source_identity = f"{qualified_name}#{signature}"
        if frontier_ids is not None and method_id not in frontier_ids:
            failures.append(
                f"{var_id}: {source_identity} is outside frontier {frontier_name}"
            )

        record_entry = records.get(var_id)
        if record_entry is None:
            failures.append(f"{var_id}: version-controlled VAR record is missing")
            continue
        record_path, record = record_entry
        source = record.get("source")
        if not isinstance(source, dict):
            failures.append(f"{var_id}: record source object is missing")
            continue
        if str(source.get("type", "")) != qualified_name or str(
            source.get("signature", "")
        ) != signature:
            failures.append(
                f"{var_id}: record source identity does not match {source_identity}"
            )

        hashes = conn.execute(
            "SELECT normalized_sha256,body_sha256 FROM methods WHERE id=?", (method_id,)
        ).fetchone()
        assert hashes is not None
        expected_algorithm = str(meta.get("fingerprint_algorithm", ""))
        actual_algorithm = str(source.get("fingerprint_algorithm", ""))
        actual_normalized = str(source.get("normalized_sha256", ""))
        actual_body = str(source.get("body_sha256", ""))
        if actual_algorithm != expected_algorithm:
            failures.append(f"{var_id}: fingerprint algorithm is stale")
        if actual_normalized != str(hashes[0]):
            failures.append(f"{var_id}: normalized source fingerprint is stale")
        if actual_body != str(hashes[1]):
            failures.append(f"{var_id}: source body fingerprint is stale")

        status = str(record.get("status", ""))
        if not _status_admitted(status, minimum):
            failures.append(
                f"{var_id}: status {status!r} does not satisfy minimum {minimum!r}"
            )

        semantic_rules = record.get("semantic_rules", [])
        if not isinstance(semantic_rules, list) or any(
            not isinstance(item, str) or not item for item in semantic_rules
        ):
            failures.append(f"{var_id}: semantic_rules must be an array of non-empty strings")
            semantic_rules = []
        if require_semantic_rules and not semantic_rules:
            failures.append(f"{var_id}: reviewed source method has no SEM linkage")

        observed_hazards = {
            str(item[0])
            for item in conn.execute(
                "SELECT DISTINCT kind FROM hazards WHERE method_id=?", (method_id,)
            )
        }
        reviewed_hazards_raw = record.get("hazards_reviewed", [])
        if not isinstance(reviewed_hazards_raw, list) or any(
            not isinstance(item, str) or not item for item in reviewed_hazards_raw
        ):
            failures.append(
                f"{var_id}: hazards_reviewed must be an array of non-empty strings"
            )
            reviewed_hazards: set[str] = set()
        else:
            reviewed_hazards = {str(item) for item in reviewed_hazards_raw}
        missing_hazards = sorted(observed_hazards - reviewed_hazards)
        if require_hazards_reviewed and missing_hazards:
            failures.append(
                f"{var_id}: Atlas hazards not explicitly reviewed: {', '.join(missing_hazards)}"
            )

        admitted_methods.append(
            {
                "var_id": var_id,
                "record_path": str(record_path),
                "record_sha256": sha256_bytes(record_path.read_bytes()),
                "source": source_identity,
                "status": status,
                "normalized_sha256": str(hashes[0]),
                "body_sha256": str(hashes[1]),
                "semantic_rules": sorted(str(item) for item in semantic_rules),
                "observed_hazards": sorted(observed_hazards),
                "reviewed_hazards": sorted(reviewed_hazards),
            }
        )

    closure_status: dict[str, int] = {}
    unresolved_call_sites: int | None = None
    if frontier_ids is not None:
        statuses = atlas._method_status_counts(conn, frontier_ids)
        closure_status = dict(sorted(statuses.items()))
        unresolved_call_sites = 0
        ids = sorted(frontier_ids)
        for offset in range(0, len(ids), 700):
            chunk = ids[offset : offset + 700]
            placeholders = ",".join("?" for _ in chunk)
            unresolved_call_sites += int(
                conn.execute(
                    f"SELECT COUNT(*) FROM method_calls WHERE caller_method_id IN ({placeholders}) AND resolved_method_id IS NULL",
                    chunk,
                ).fetchone()[0]
            )

    conn.close()
    result: dict[str, object] = {
        "schema": SCHEMA,
        "gate_id": str(gate["id"]),
        "admitted": not failures,
        "gate_path": str(gate_path),
        "gate_sha256": sha256_bytes(gate_raw),
        "minimum_status": minimum,
        "source": {
            "minecraft_version": meta.get("minecraft_version"),
            "protocol_version": meta.get("protocol_version"),
            "world_version": meta.get("world_version"),
            "archive_sha256": meta.get("source_archive_sha256"),
            "fingerprint_algorithm": meta.get("fingerprint_algorithm"),
            "atlas_version": meta.get("atlas_version"),
            "schema_version": meta.get("schema_version"),
        },
        "frontier": frontier_report,
        "required_methods": admitted_methods,
        "closure_diagnostics": {
            "review_status": closure_status,
            "unresolved_call_sites": unresolved_call_sites,
            "note": "Closure diagnostics are review leads. Admission applies only to explicitly required methods.",
        },
        "failures": failures,
    }
    return result


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--db", type=Path, default=atlas.default_db(), help="generated Vanilla Atlas SQLite index"
    )
    parser.add_argument("--gate", type=Path, required=True)
    parser.add_argument("--records", type=Path, default=Path("vanilla/records"))
    parser.add_argument("--output", type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        report = evaluate(db_path=args.db, gate_path=args.gate, records_dir=args.records)
    except (GateError, OSError, json.JSONDecodeError) as error:
        print(f"source admission error: {error}", file=sys.stderr)
        return 1
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    else:
        print(encoded, end="")
    return 0 if report["admitted"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
