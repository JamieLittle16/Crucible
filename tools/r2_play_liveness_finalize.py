#!/usr/bin/env python3
"""Finalize an explicitly reviewed, source-free R2 Play-liveness worksheet."""
from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path
from typing import Any

try:
    from . import protocol_codegen
    from . import r2_play_liveness_source_review as review
except ImportError:  # Direct `python3 tools/...` execution.
    import protocol_codegen  # type: ignore[no-redef]
    import r2_play_liveness_source_review as review  # type: ignore[no-redef]

SCHEMA = 1
CONTRACT_ID = "PROTO-NET-PLAY-LIVENESS-26-2-001"
GOLDEN_ID = 0x0102030405060708

EXPECTED_RULES: dict[str, tuple[str, ...]] = {
    "VAR-NET-R2-PLAY-REGISTRATION-001": (
        "SEM-NET-R2-LIVE-009",
        "SEM-NET-R2-LIVE-010",
    ),
    "VAR-NET-R2-KEEPALIVE-CB-CODEC-001": ("SEM-NET-R2-LIVE-009",),
    "VAR-NET-R2-KEEPALIVE-SB-CODEC-001": ("SEM-NET-R2-LIVE-010",),
    "VAR-NET-R2-LIVENESS-CONSTRUCT-001": (
        "SEM-NET-R2-LIVE-001",
        "SEM-NET-R2-LIVE-005",
    ),
    "VAR-NET-R2-LIVENESS-CLOSE-001": ("SEM-NET-R2-LIVE-006",),
    "VAR-NET-R2-LIVENESS-CLOSED-GATE-001": (
        "SEM-NET-R2-LIVE-006",
        "SEM-NET-R2-LIVE-008",
    ),
    "VAR-NET-R2-LIVENESS-SERVICE-001": (
        "SEM-NET-R2-LIVE-002",
        "SEM-NET-R2-LIVE-003",
        "SEM-NET-R2-LIVE-006",
        "SEM-NET-R2-LIVE-008",
        "SEM-NET-R2-LIVE-009",
    ),
    "VAR-NET-R2-LIVENESS-REPLY-001": (
        "SEM-NET-R2-LIVE-004",
        "SEM-NET-R2-LIVE-005",
        "SEM-NET-R2-LIVE-007",
        "SEM-NET-R2-LIVE-010",
    ),
}


class FinalizeError(ValueError):
    """Raised when review evidence is incomplete or inconsistent."""


def _read_json(path: Path) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise FinalizeError(f"worksheet must be a real non-symlink file: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise FinalizeError(f"could not read worksheet: {error}") from error
    if not isinstance(value, dict):
        raise FinalizeError("worksheet must be a JSON object")
    return value


def _lock_target(lock_path: Path) -> dict[str, object]:
    try:
        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise FinalizeError(f"could not read vanilla lock: {error}") from error
    source = lock.get("source")
    atlas = lock.get("atlas")
    if not isinstance(source, dict) or not isinstance(atlas, dict):
        raise FinalizeError("vanilla lock source/atlas tables are missing")
    minecraft = lock.get("minecraft")
    protocol = lock.get("protocol")
    archive = source.get("archive_sha256")
    fingerprint = atlas.get("fingerprint_algorithm")
    if not isinstance(minecraft, str) or type(protocol) is not int:
        raise FinalizeError("vanilla lock target identity is invalid")
    if not isinstance(archive, str) or not isinstance(fingerprint, str):
        raise FinalizeError("vanilla lock source identity is invalid")
    return {
        "minecraft": minecraft,
        "protocol": protocol,
        "source_archive_sha256": archive,
        "fingerprint_algorithm": fingerprint,
    }


def _source(value: object, *, candidate_id: str, fingerprint: str) -> dict[str, str]:
    if not isinstance(value, dict):
        raise FinalizeError(f"{candidate_id}: source must be an object")
    required = {
        "type",
        "signature",
        "fingerprint_algorithm",
        "normalized_sha256",
        "body_sha256",
    }
    if set(value) != required:
        raise FinalizeError(f"{candidate_id}: source fields are not canonical")
    result: dict[str, str] = {}
    for key in required:
        item = value[key]
        if not isinstance(item, str) or not item:
            raise FinalizeError(f"{candidate_id}: source.{key} must be non-empty text")
        result[key] = item
    if result["fingerprint_algorithm"] != fingerprint:
        raise FinalizeError(f"{candidate_id}: fingerprint algorithm does not match vanilla lock")
    for key in ("normalized_sha256", "body_sha256"):
        digest = result[key]
        if len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
            raise FinalizeError(f"{candidate_id}: source.{key} is not lowercase SHA-256")
    return result


def finalize(worksheet: dict[str, Any], *, lock_path: Path) -> tuple[list[dict[str, object]], dict[str, object], str]:
    target = _lock_target(lock_path)
    expected_top = {
        "schema",
        "kind",
        "contains_official_source_text",
        "source_archive_sha256",
        "candidate_count",
        "candidates",
    }
    if set(worksheet) != expected_top:
        raise FinalizeError("worksheet top-level fields are not canonical")
    if worksheet["schema"] != SCHEMA or worksheet["kind"] != review.WORKSHEET_KIND:
        raise FinalizeError("worksheet identity mismatch")
    if worksheet["contains_official_source_text"] is not False:
        raise FinalizeError("finalizer accepts source-free worksheets only")
    if worksheet["source_archive_sha256"] != target["source_archive_sha256"]:
        raise FinalizeError("worksheet source archive does not match vanilla lock")
    candidates = worksheet["candidates"]
    if not isinstance(candidates, list) or worksheet["candidate_count"] != len(candidates):
        raise FinalizeError("worksheet candidate count is invalid")
    if len(candidates) != len(EXPECTED_RULES):
        raise FinalizeError("worksheet does not contain the complete eight-body frontier")

    records: list[dict[str, object]] = []
    seen: set[str] = set()
    for item in candidates:
        if not isinstance(item, dict):
            raise FinalizeError("worksheet candidate must be an object")
        candidate_id = item.get("candidate_id")
        if not isinstance(candidate_id, str) or candidate_id not in EXPECTED_RULES:
            raise FinalizeError(f"unexpected candidate id: {candidate_id!r}")
        if candidate_id in seen:
            raise FinalizeError(f"duplicate candidate id: {candidate_id}")
        seen.add(candidate_id)
        decision = item.get("decision")
        if not isinstance(decision, dict):
            raise FinalizeError(f"{candidate_id}: decision must be an object")
        if decision.get("source_inspected") is not True or decision.get("accepted") is not True:
            raise FinalizeError(f"{candidate_id}: source must be explicitly inspected and accepted")
        observed = item.get("atlas_observed_hazards")
        reviewed = decision.get("hazards_reviewed")
        if not isinstance(observed, list) or not isinstance(reviewed, list):
            raise FinalizeError(f"{candidate_id}: hazards must be arrays")
        if sorted(set(reviewed)) != sorted(set(observed)):
            raise FinalizeError(f"{candidate_id}: all observed hazards must be explicitly reviewed")
        rules = decision.get("semantic_rules")
        if not isinstance(rules, list) or tuple(sorted(rules)) != tuple(sorted(EXPECTED_RULES[candidate_id])):
            raise FinalizeError(f"{candidate_id}: semantic-rule links do not match the closed frontier")
        followups = decision.get("followup_dependencies")
        if followups != []:
            raise FinalizeError(f"{candidate_id}: unresolved follow-up dependencies remain")
        note = decision.get("note")
        if not isinstance(note, str) or not note.strip():
            raise FinalizeError(f"{candidate_id}: reviewed decision requires a note")
        source = _source(
            item.get("source"),
            candidate_id=candidate_id,
            fingerprint=str(target["fingerprint_algorithm"]),
        )
        records.append(
            {
                "schema": 1,
                "id": candidate_id,
                "status": "VAR_REVIEWED",
                "source": source,
                "classifications": ["PROTOCOL"],
                "hazards_reviewed": sorted(set(reviewed)),
                "semantic_rules": list(EXPECTED_RULES[candidate_id]),
                "evidence": [],
                "notes": [note.strip()],
            }
        )

    if seen != set(EXPECTED_RULES):
        raise FinalizeError("worksheet is missing required candidates")
    records.sort(key=lambda record: str(record["id"]))

    challenge = GOLDEN_ID.to_bytes(8, "big", signed=True)
    cb_body = bytes([0x2C]) + challenge
    sb_body = bytes([0x1C]) + challenge
    contract = {
        "schema": 1,
        "id": CONTRACT_ID,
        "target": target,
        "packets": [
            {
                "name": "keep-alive",
                "phase": "play",
                "direction": "clientbound",
                "id": 0x2C,
                "semantic_rules": ["SEM-NET-R2-LIVE-009"],
                "source_records": [
                    "VAR-NET-R2-PLAY-REGISTRATION-001",
                    "VAR-NET-R2-KEEPALIVE-CB-CODEC-001",
                ],
                "golden": {
                    "body_hex": cb_body.hex(),
                    "frame_hex": (bytes([len(cb_body)]) + cb_body).hex(),
                },
            },
            {
                "name": "keep-alive",
                "phase": "play",
                "direction": "serverbound",
                "id": 0x1C,
                "semantic_rules": ["SEM-NET-R2-LIVE-010"],
                "source_records": [
                    "VAR-NET-R2-PLAY-REGISTRATION-001",
                    "VAR-NET-R2-KEEPALIVE-SB-CODEC-001",
                ],
                "golden": {
                    "body_hex": sb_body.hex(),
                    "frame_hex": (bytes([len(sb_body)]) + sb_body).hex(),
                },
            },
        ],
    }
    generated = protocol_codegen.render_rust(contract)
    return records, contract, generated


def _write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("worksheet", type=Path)
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    try:
        worksheet = _read_json(args.worksheet)
        records, contract, generated = finalize(worksheet, lock_path=args.lock)
        if args.output_dir.exists() or args.output_dir.is_symlink():
            raise FinalizeError(f"output directory must not already exist: {args.output_dir}")
        args.output_dir.mkdir(parents=True)
        records_dir = args.output_dir / "records"
        for record in records:
            _write_json(records_dir / f"{record['id']}.json", record)
        _write_json(args.output_dir / f"{CONTRACT_ID}.json", contract)
        (args.output_dir / "play_liveness_26_2.rs").write_text(generated, encoding="utf-8")
    except (FinalizeError, OSError, KeyError, TypeError, ValueError) as error:
        print(f"R2 Play-liveness finalization error: {error}", file=sys.stderr)
        return 2
    print(f"r2_play_liveness_finalized={args.output_dir}")
    print(f"records={len(records)}")
    print(f"contract={CONTRACT_ID}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
