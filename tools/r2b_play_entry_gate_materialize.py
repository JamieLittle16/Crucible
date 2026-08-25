#!/usr/bin/env python3
"""Materialize canonical source-free R2B Play-entry VAR records and candidate source gate.

This is deliberately not the admission step. It consumes the exact reviewed source-rich dossiers plus
an `ADMISSION_INPUTS_VERIFIED` manifest, emits only source-free canonical records, and produces a
candidate `GATE-NET-PLAY-ENTRY-26_2-001.json`. The ordinary `vanilla_source_gate.py` must still run
against the pinned Atlas database and report `admitted=true` before target/product code may rely on it.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
from pathlib import Path
from typing import Any, Sequence

try:
    from . import r2b_play_entry_finalize as finalizer
except ImportError:  # Direct `python3 tools/...` execution.
    import r2b_play_entry_finalize as finalizer  # type: ignore[no-redef]

SCHEMA = 1
GATE_ID = "GATE-NET-PLAY-ENTRY-26_2-001"
KIND = "r2b-play-entry-gate-materialization-v1"
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_67_RESULT = REPO_ROOT / "vanilla/reviews/network/r2b-play-entry-final-67-review-result.json"
DEFAULT_117_RESULT = REPO_ROOT / "vanilla/reviews/network/r2b-play-entry-wire-117-review-result.json"

REVIEW_RULES_117: dict[str, tuple[str, ...]] = {
    "COMMAND_TREE": (
        "SEM-NET-R2B-PLAY-003",
        "SEM-NET-R2B-PLAY-011",
        "SEM-NET-R2B-PLAY-015",
    ),
    "RECIPE_BOOK_SETTINGS": (
        "SEM-NET-R2B-PLAY-004",
        "SEM-NET-R2B-PLAY-012",
    ),
    "SYNCHRONIZED_RECIPES": (
        "SEM-NET-R2B-PLAY-002",
        "SEM-NET-R2B-PLAY-011",
        "SEM-NET-R2B-PLAY-015",
    ),
    "CLOCK_FULL_SYNC": (
        "SEM-NET-R2B-PLAY-009",
        "SEM-NET-R2B-PLAY-012",
    ),
    "DIMENSION_TYPE": (
        "SEM-NET-R2B-PLAY-001",
        "SEM-NET-R2B-PLAY-012",
    ),
    "DEFAULT_SPAWN": (
        "SEM-NET-R2B-PLAY-009",
        "SEM-NET-R2B-PLAY-012",
    ),
    "INITIAL_INVENTORY": (
        "SEM-NET-R2B-PLAY-010",
        "SEM-NET-R2B-PLAY-012",
    ),
}
FINAL_SEAM_RULES: dict[str, tuple[str, ...]] = {
    "GENERIC_REGISTRY_WIRE": (
        "SEM-NET-R2B-PLAY-009",
        "SEM-NET-R2B-PLAY-012",
    ),
    "GLOBAL_POS_WIRE": (
        "SEM-NET-R2B-PLAY-009",
        "SEM-NET-R2B-PLAY-012",
    ),
}


class MaterializeError(ValueError):
    """Raised when reviewed evidence cannot be canonicalized without ambiguity."""


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _read_json(path: Path, *, label: str) -> tuple[dict[str, Any], str]:
    if path.is_symlink() or not path.is_file():
        raise MaterializeError(f"{label} must be a real non-symlink file: {path}")
    try:
        raw = path.read_bytes()
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise MaterializeError(f"could not read {label}: {error}") from error
    if not isinstance(value, dict):
        raise MaterializeError(f"{label} root must be an object")
    return value, _sha256(raw)


def _review_result(
    path: Path,
    *,
    expected_id: str,
    expected_count: int,
    expected_dossier_sha: str,
) -> dict[str, Any]:
    value, _digest = _read_json(path, label=f"{expected_id} review result")
    required = {
        "id": expected_id,
        "candidate_count": expected_count,
        "dossier_sha256": expected_dossier_sha,
        "source_archive_sha256": finalizer.SOURCE_SHA256,
        "contains_official_source_text": False,
        "all_candidates_source_inspected": True,
        "atlas_observed_hazards_reviewed": True,
    }
    mismatches = {
        key: {"expected": expected, "actual": value.get(key)}
        for key, expected in required.items()
        if value.get(key) != expected
    }
    if mismatches:
        raise MaterializeError(
            f"{expected_id} review result is not canonical: {json.dumps(mismatches, sort_keys=True)}"
        )
    return value


def _canonical_source(value: object, *, label: str) -> dict[str, str]:
    if not isinstance(value, dict):
        raise MaterializeError(f"{label}: source must be an object")
    required = {
        "type",
        "signature",
        "fingerprint_algorithm",
        "normalized_sha256",
        "body_sha256",
    }
    if set(value) != required:
        raise MaterializeError(f"{label}: source fields are not canonical")
    result = {key: value[key] for key in required}
    if any(not isinstance(item, str) or not item for item in result.values()):
        raise MaterializeError(f"{label}: source values must be non-empty strings")
    if result["fingerprint_algorithm"] != "java-token-v2-literal-sensitive":
        raise MaterializeError(f"{label}: fingerprint algorithm mismatch")
    for key in ("normalized_sha256", "body_sha256"):
        digest = result[key]
        if len(digest) != 64 or any(ch not in "0123456789abcdef" for ch in digest):
            raise MaterializeError(f"{label}: {key} is not lowercase SHA-256")
    return result  # type: ignore[return-value]


def _rules_for_67(candidate_id: str) -> tuple[str, ...]:
    token = candidate_id.upper()
    categories: tuple[tuple[tuple[str, ...], tuple[str, ...]], ...] = (
        (("GAME-PROTOCOLS",), ("SEM-NET-R2B-PLAY-001", "SEM-NET-R2B-PLAY-012")),
        (("PLACE-NEW-PLAYER",), (
            "SEM-NET-R2B-PLAY-001", "SEM-NET-R2B-PLAY-002", "SEM-NET-R2B-PLAY-005",
            "SEM-NET-R2B-PLAY-007", "SEM-NET-R2B-PLAY-008", "SEM-NET-R2B-PLAY-009",
            "SEM-NET-R2B-PLAY-010", "SEM-NET-R2B-PLAY-014",
        )),
        (("PERMISSION", "COMMANDS", "ENTITY-EVENT"), ("SEM-NET-R2B-PLAY-003", "SEM-NET-R2B-PLAY-011")),
        (("RECIPE-BOOK", "RECIPE-SETTINGS", "RECIPE-ADD"), ("SEM-NET-R2B-PLAY-004",)),
        (("RECIPES-CODEC", "RECIPE-PROPERTIES", "STONECUTTER"), ("SEM-NET-R2B-PLAY-002", "SEM-NET-R2B-PLAY-011", "SEM-NET-R2B-PLAY-015")),
        (("SCOREBOARD", "ACTIVE-EFFECTS", "BOSS"), ("SEM-NET-R2B-PLAY-005",)),
        (("LEVEL-INFO",), ("SEM-NET-R2B-PLAY-009",)),
        (("TELEPORT", "POSITION-CODEC", "POSITION-OF", "POSITION-MOVE", "RELATIVE"), ("SEM-NET-R2B-PLAY-006", "SEM-NET-R2B-PLAY-012")),
        (("SERVER-STATUS", "SERVER-DATA"), ("SEM-NET-R2B-PLAY-007",)),
        (("PLAYER-INFO", "BROADCAST-ALL"), ("SEM-NET-R2B-PLAY-008", "SEM-NET-R2B-PLAY-012")),
        (("INVENTORY", "INIT-MENU", "CONTAINER"), ("SEM-NET-R2B-PLAY-010",)),
        (("CREATE-SPAWN-INFO", "SPAWN-INFO", "LOGIN", "DIFFICULTY", "ABILITIES", "HELD-SLOT"), ("SEM-NET-R2B-PLAY-002", "SEM-NET-R2B-PLAY-012")),
        (("BORDER", "CLOCK", "SPAWN-POSITION", "GAME-EVENT", "TICK-RATE", "TICKING"), ("SEM-NET-R2B-PLAY-009", "SEM-NET-R2B-PLAY-012")),
    )
    for needles, rules in categories:
        if any(needle in token for needle in needles):
            return rules
    raise MaterializeError(f"67-body candidate has no explicit SEM mapping: {candidate_id}")


def _rules_for_groups(group_ids: object, mapping: dict[str, tuple[str, ...]], *, label: str) -> tuple[str, ...]:
    if not isinstance(group_ids, list) or not group_ids or any(not isinstance(item, str) for item in group_ids):
        raise MaterializeError(f"{label}: group_ids must be non-empty strings")
    rules: set[str] = set()
    for group in group_ids:
        group_rules = mapping.get(group)
        if group_rules is None:
            raise MaterializeError(f"{label}: no SEM mapping for group {group}")
        rules.update(group_rules)
    return tuple(sorted(rules))


def _var_id(candidate_id: str) -> str:
    if candidate_id.startswith("DISC-NET-R1B-PLAY-FOLLOWUP-"):
        return "VAR-NET-R2B-PLAY-FOLLOWUP-" + candidate_id.removeprefix("DISC-NET-R1B-PLAY-FOLLOWUP-")
    if candidate_id.startswith("DISC-NET-R1B-PLAY-"):
        return "VAR-NET-R2B-PLAY-" + candidate_id.removeprefix("DISC-NET-R1B-PLAY-")
    if candidate_id.startswith("DISC-NET-R2B-PLAY-"):
        return "VAR-NET-R2B-PLAY-" + candidate_id.removeprefix("DISC-NET-R2B-PLAY-")
    raise MaterializeError(f"cannot canonicalize candidate id: {candidate_id}")


def _dossier_records(
    path: Path,
    *,
    expected_id: str,
    expected_count: int,
    expected_sha: str,
    review_id: str,
    family: str,
) -> list[dict[str, object]]:
    value, digest = _read_json(path, label=f"{family} dossier")
    if digest != expected_sha:
        raise MaterializeError(f"{family} dossier SHA mismatch: {digest}")
    if value.get("id") != expected_id or value.get("candidate_count") != expected_count:
        raise MaterializeError(f"{family} dossier identity/count mismatch")
    if value.get("source_archive_sha256") != finalizer.SOURCE_SHA256:
        raise MaterializeError(f"{family} dossier source pin mismatch")
    if value.get("contains_official_source_text") is not True:
        raise MaterializeError(f"{family} dossier must be source-rich")
    items = value.get("candidates")
    if not isinstance(items, list) or len(items) != expected_count:
        raise MaterializeError(f"{family} dossier candidates mismatch")
    records: list[dict[str, object]] = []
    seen_ids: set[str] = set()
    seen_sources: set[str] = set()
    for item in items:
        if not isinstance(item, dict):
            raise MaterializeError(f"{family}: candidate must be object")
        candidate_id = item.get("candidate_id")
        source_identity = item.get("source_identity")
        if not isinstance(candidate_id, str) or candidate_id in seen_ids:
            raise MaterializeError(f"{family}: candidate ids must be unique")
        source = _canonical_source(item.get("source"), label=candidate_id)
        expected_identity = f"{source['type']}#{source['signature']}"
        if source_identity != expected_identity or source_identity in seen_sources:
            raise MaterializeError(f"{candidate_id}: source identity mismatch/duplicate")
        hazards = item.get("atlas_observed_hazards")
        if not isinstance(hazards, list) or any(not isinstance(hazard, str) or not hazard for hazard in hazards):
            raise MaterializeError(f"{candidate_id}: Atlas hazards must be strings")
        if family == "67-body":
            rules = _rules_for_67(candidate_id)
        else:
            rules = _rules_for_groups(item.get("group_ids"), REVIEW_RULES_117, label=candidate_id)
        records.append({
            "schema": 1,
            "id": _var_id(candidate_id),
            "status": "VAR_REVIEWED",
            "source": source,
            "classifications": ["PROTOCOL"],
            "hazards_reviewed": sorted(set(hazards)),
            "semantic_rules": list(rules),
            "evidence": [review_id],
            "notes": [f"Canonicalized from exact pinned {family} dossier after complete body/hazard review."],
        })
        seen_ids.add(candidate_id)
        seen_sources.add(source_identity)
    return records


def _final_seam_records(manifest: dict[str, Any]) -> list[dict[str, object]]:
    items = manifest.get("final_seam_source_records")
    if not isinstance(items, list) or not items:
        raise MaterializeError("verified manifest contains no final-seam source records")
    records: list[dict[str, object]] = []
    for item in items:
        if not isinstance(item, dict):
            raise MaterializeError("final-seam source record must be object")
        candidate_id = item.get("candidate_id")
        if not isinstance(candidate_id, str):
            raise MaterializeError("final-seam candidate id missing")
        rules = _rules_for_groups(item.get("group_ids"), FINAL_SEAM_RULES, label=candidate_id)
        hazards = item.get("hazards_reviewed")
        if not isinstance(hazards, list):
            raise MaterializeError(f"{candidate_id}: reviewed hazards missing")
        records.append({
            "schema": 1,
            "id": _var_id(candidate_id),
            "status": "VAR_REVIEWED",
            "source": _canonical_source(item.get("source"), label=candidate_id),
            "classifications": ["PROTOCOL"],
            "hazards_reviewed": sorted(set(hazards)),
            "semantic_rules": list(rules),
            "evidence": ["REVIEW-NET-R2B-PLAY-FINAL-SEAMS-26_2-001"],
            "notes": ["Canonicalized from explicitly accepted final-seam worksheet with zero follow-up dependencies."],
        })
    return records


def materialize(
    *,
    verified_manifest: Path,
    final_67_dossier: Path,
    wire_117_dossier: Path,
    final_67_result: Path,
    wire_117_result: Path,
) -> tuple[list[dict[str, object]], dict[str, object], dict[str, object]]:
    verified, verified_sha = _read_json(verified_manifest, label="verified admission manifest")
    if (
        verified.get("schema") != 1
        or verified.get("kind") != finalizer.KIND
        or verified.get("status") != finalizer.STATUS
        or verified.get("production_admitted") is not False
        or verified.get("gate_emission_ready") is not True
        or verified.get("gate") != GATE_ID
    ):
        raise MaterializeError("verified admission manifest is not gate-emission ready")
    target = verified.get("target")
    if not isinstance(target, dict) or target.get("source_archive_sha256") != finalizer.SOURCE_SHA256:
        raise MaterializeError("verified manifest target/source pin mismatch")

    _review_result(
        final_67_result,
        expected_id=finalizer.FINAL_67_ID,
        expected_count=finalizer.FINAL_67_COUNT,
        expected_dossier_sha=finalizer.FINAL_67_DOSSIER_SHA256,
    )
    _review_result(
        wire_117_result,
        expected_id=finalizer.WIRE_117_ID,
        expected_count=finalizer.WIRE_117_COUNT,
        expected_dossier_sha=finalizer.WIRE_117_DOSSIER_SHA256,
    )

    records = _dossier_records(
        final_67_dossier,
        expected_id=finalizer.FINAL_67_ID,
        expected_count=finalizer.FINAL_67_COUNT,
        expected_sha=finalizer.FINAL_67_DOSSIER_SHA256,
        review_id=finalizer.FINAL_67_ID,
        family="67-body",
    )
    records.extend(_dossier_records(
        wire_117_dossier,
        expected_id=finalizer.WIRE_117_ID,
        expected_count=finalizer.WIRE_117_COUNT,
        expected_sha=finalizer.WIRE_117_DOSSIER_SHA256,
        review_id=finalizer.WIRE_117_ID,
        family="117-body",
    ))
    records.extend(_final_seam_records(verified))

    ids = [str(record["id"]) for record in records]
    source_ids = [f"{record['source']['type']}#{record['source']['signature']}" for record in records]  # type: ignore[index]
    if len(ids) != len(set(ids)):
        raise MaterializeError("canonical VAR ids collide")
    if len(source_ids) != len(set(source_ids)):
        raise MaterializeError("review families contain duplicate source identities")
    records.sort(key=lambda record: str(record["id"]))

    gate = {
        "schema": 1,
        "id": GATE_ID,
        "minimum_status": "VAR_REVIEWED",
        "require_semantic_rules": True,
        "require_hazards_reviewed": True,
        "methods": [
            {
                "query": f"{record['source']['type']}#{record['source']['signature']}",  # type: ignore[index]
                "var_id": record["id"],
            }
            for record in records
        ],
    }
    oracle = verified.get("composition_oracle")
    if not isinstance(oracle, dict):
        raise MaterializeError("verified manifest has no composition oracle")
    composition = {
        "schema": 1,
        "kind": "r2b-play-entry-composition-artifacts-v1",
        "production_admitted": False,
        "target": target,
        "selected_profile": {
            "entry": "fresh-offline-default",
            "permission": "default-non-operator",
            "composition": "pinned-vanilla-26.2-default",
        },
        "semantic_rules": [
            "SEM-NET-R2B-PLAY-003",
            "SEM-NET-R2B-PLAY-011",
            "SEM-NET-R2B-PLAY-015",
        ],
        "oracle": oracle,
        "invalidation": (
            "Any target, command/argument/registry composition, enabled-feature, permission or recipe/data-pack "
            "composition mismatch is a cache miss/unsupported profile; artifact reuse is forbidden."
        ),
    }
    manifest = {
        "schema": SCHEMA,
        "kind": KIND,
        "source_free": True,
        "production_admitted": False,
        "verified_input_sha256": verified_sha,
        "var_record_count": len(records),
        "gate_id": GATE_ID,
        "next_required_step": (
            "Install the generated source-free records/gate in a clean branch and run tools/vanilla_source_gate.py "
            "against the pinned Atlas database. Only an independent admitted=true report can promote the gate."
        ),
    }
    return records, gate, {"manifest": manifest, "composition": composition}


def _write(output_dir: Path, records: list[dict[str, object]], gate: dict[str, object], extra: dict[str, object]) -> None:
    if output_dir.exists() or output_dir.is_symlink():
        raise MaterializeError(f"output directory must not already exist: {output_dir}")
    output_dir.mkdir(parents=True)
    try:
        records_dir = output_dir / "records"
        records_dir.mkdir()
        for record in records:
            (records_dir / f"{record['id']}.json").write_text(
                json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n",
                encoding="utf-8",
            )
        (output_dir / f"{GATE_ID}.json").write_text(
            json.dumps(gate, sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
        (output_dir / "composition-artifacts.json").write_text(
            json.dumps(extra["composition"], sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
        (output_dir / "manifest.json").write_text(
            json.dumps(extra["manifest"], sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
    except Exception:
        shutil.rmtree(output_dir, ignore_errors=True)
        raise


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verified-manifest", type=Path, required=True)
    parser.add_argument("--final-67-dossier", type=Path, required=True)
    parser.add_argument("--wire-117-dossier", type=Path, required=True)
    parser.add_argument("--final-67-result", type=Path, default=DEFAULT_67_RESULT)
    parser.add_argument("--wire-117-result", type=Path, default=DEFAULT_117_RESULT)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        records, gate, extra = materialize(
            verified_manifest=args.verified_manifest,
            final_67_dossier=args.final_67_dossier,
            wire_117_dossier=args.wire_117_dossier,
            final_67_result=args.final_67_result,
            wire_117_result=args.wire_117_result,
        )
        _write(args.output_dir, records, gate, extra)
    except (MaterializeError, OSError, KeyError, TypeError, ValueError) as error:
        print(f"R2B Play-entry gate materialization error: {error}", file=sys.stderr)
        return 2
    print(f"r2b_gate_materialization={args.output_dir}")
    print(f"var_records={len(records)}")
    print(f"gate={GATE_ID}")
    print("source_free=true")
    print("production_admitted=false")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
