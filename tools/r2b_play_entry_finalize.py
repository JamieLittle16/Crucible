#!/usr/bin/env python3
"""Fail-closed verifier for the complete selected-profile R2B Play-entry evidence bundle.

The tool intentionally emits only a source-free verified-input manifest. It does not copy source-rich
review material and it does not by itself mark `GATE-NET-PLAY-ENTRY-26_2-001` admitted. The verified
manifest is the deterministic input to the subsequent VAR/gate/target generation step.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path
from typing import Any, Sequence

try:
    from . import r2b_play_bootstrap_oracle_extract as oracle_extract
    from . import r2b_play_entry_final_seams_source_review as final_seams
except ImportError:  # Direct `python3 tools/...` execution.
    import r2b_play_bootstrap_oracle_extract as oracle_extract  # type: ignore[no-redef]
    import r2b_play_entry_final_seams_source_review as final_seams  # type: ignore[no-redef]

SCHEMA = 1
KIND = "r2b-play-entry-admission-inputs-v1"
STATUS = "ADMISSION_INPUTS_VERIFIED"
SOURCE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
CAPTURE_SHA256 = "11ead8de74df70b40d7fb045ff9561f06f6e24238765d4141a1d090cab546b57"
FINAL_67_ID = "REVIEW-NET-R2B-PLAY-ENTRY-FINAL-26_2-001"
FINAL_67_COUNT = 67
FINAL_67_DOSSIER_SHA256 = "aecd49dd79962a905b5edb8f586c109ce731c304afe4f0f7c3ad3275c906b8b9"
WIRE_117_ID = "REVIEW-NET-R2B-PLAY-WIRE-CLOSURE-26_2-001"
WIRE_117_COUNT = 117
WIRE_117_DOSSIER_SHA256 = "93999fca0a4c69eda607e729af61c74e7ce40c96bf4201516904fabf79bc2e3a"
SEMANTIC_RULES = tuple(f"SEM-NET-R2B-PLAY-{index:03d}" for index in range(1, 16))
EXPECTED_FINAL_SEAM_GROUPS = {"GENERIC_REGISTRY_WIRE", "GLOBAL_POS_WIRE"}
EXPECTED_ORACLE = {"commands": 16, "update-recipes": 133}
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SEMANTICS = REPO_ROOT / "vanilla/semantics/network/R2B_PLAY_ENTRY_SEMANTICS.md"
DEFAULT_LOCK = REPO_ROOT / "vanilla/vanilla.lock.toml"


class FinalizeError(ValueError):
    """Raised when an evidence input cannot support R2B admission."""


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _read_bytes(path: Path, *, label: str) -> bytes:
    if path.is_symlink() or not path.is_file():
        raise FinalizeError(f"{label} must be a real non-symlink file: {path}")
    try:
        return path.read_bytes()
    except OSError as error:
        raise FinalizeError(f"could not read {label}: {error}") from error


def _read_json(path: Path, *, label: str) -> tuple[dict[str, Any], str]:
    raw = _read_bytes(path, label=label)
    try:
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise FinalizeError(f"{label} is not valid UTF-8 JSON: {error}") from error
    if not isinstance(value, dict):
        raise FinalizeError(f"{label} JSON root must be an object")
    return value, _sha256(raw)


def _lock_target(lock_path: Path) -> dict[str, object]:
    raw = _read_bytes(lock_path, label="vanilla lock")
    try:
        value = tomllib.loads(raw.decode("utf-8", errors="strict"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise FinalizeError(f"vanilla lock is invalid: {error}") from error
    source = value.get("source")
    atlas = value.get("atlas")
    minecraft = value.get("minecraft")
    protocol = value.get("protocol")
    data_version = value.get("data_version")
    if not isinstance(source, dict) or not isinstance(atlas, dict):
        raise FinalizeError("vanilla lock source/atlas tables are missing")
    archive = source.get("archive_sha256")
    fingerprint = atlas.get("fingerprint_algorithm")
    if (
        minecraft != "26.2"
        or protocol != 776
        or data_version != 4903
        or archive != SOURCE_SHA256
        or fingerprint != "java-token-v2-literal-sensitive"
    ):
        raise FinalizeError("vanilla lock does not match the selected R2B target/source pin")
    return {
        "minecraft": minecraft,
        "protocol": protocol,
        "data_version": data_version,
        "source_archive_sha256": archive,
        "fingerprint_algorithm": fingerprint,
    }


def validate_semantics(path: Path, *, lock_path: Path) -> dict[str, object]:
    target = _lock_target(lock_path)
    raw = _read_bytes(path, label="R2B semantic contract")
    try:
        text = raw.decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise FinalizeError(f"R2B semantic contract is not UTF-8: {error}") from error
    if SOURCE_SHA256 not in text or "Minecraft **26.2**, protocol **776**" not in text:
        raise FinalizeError("R2B semantic contract target/source identity is incomplete")
    missing = [rule for rule in SEMANTIC_RULES if rule not in text]
    if missing:
        raise FinalizeError(f"R2B semantic contract is missing rules: {missing}")
    return {
        "path": str(path),
        "sha256": _sha256(raw),
        "semantic_rules": list(SEMANTIC_RULES),
        "target": target,
    }


def validate_pinned_dossier(
    path: Path,
    *,
    label: str,
    expected_id: str,
    expected_count: int,
    expected_sha256: str,
) -> dict[str, object]:
    value, digest = _read_json(path, label=label)
    if digest != expected_sha256:
        raise FinalizeError(f"{label} SHA-256 mismatch: {digest}")
    if value.get("id") != expected_id or value.get("candidate_count") != expected_count:
        raise FinalizeError(f"{label} identity/count mismatch")
    if value.get("source_archive_sha256") != SOURCE_SHA256:
        raise FinalizeError(f"{label} source pin mismatch")
    if value.get("contains_official_source_text") is not True:
        raise FinalizeError(f"{label} must be the exact source-rich reviewed dossier")
    candidates = value.get("candidates")
    if not isinstance(candidates, list) or len(candidates) != expected_count:
        raise FinalizeError(f"{label} candidate array mismatch")
    identities: set[str] = set()
    for item in candidates:
        if not isinstance(item, dict):
            raise FinalizeError(f"{label} candidate must be an object")
        identity = item.get("source_identity")
        if not isinstance(identity, str) or not identity or identity in identities:
            raise FinalizeError(f"{label} source identities must be unique non-empty strings")
        identities.add(identity)
    return {
        "id": expected_id,
        "candidate_count": expected_count,
        "dossier_sha256": digest,
    }


def _canonical_source(item: object, *, label: str) -> dict[str, str]:
    if not isinstance(item, dict):
        raise FinalizeError(f"{label} source must be an object")
    required = {
        "type",
        "signature",
        "fingerprint_algorithm",
        "normalized_sha256",
        "body_sha256",
    }
    if set(item) != required:
        raise FinalizeError(f"{label} source fields are not canonical")
    result: dict[str, str] = {}
    for key in required:
        value = item[key]
        if not isinstance(value, str) or not value:
            raise FinalizeError(f"{label} source.{key} must be non-empty text")
        result[key] = value
    if result["fingerprint_algorithm"] != "java-token-v2-literal-sensitive":
        raise FinalizeError(f"{label} fingerprint algorithm mismatch")
    for key in ("normalized_sha256", "body_sha256"):
        digest = result[key]
        if len(digest) != 64 or any(ch not in "0123456789abcdef" for ch in digest):
            raise FinalizeError(f"{label} source.{key} is not lowercase SHA-256")
    return result


def validate_final_seams(dossier_path: Path, worksheet_path: Path) -> dict[str, object]:
    dossier, dossier_sha = _read_json(dossier_path, label="final-seams dossier")
    worksheet, worksheet_sha = _read_json(worksheet_path, label="final-seams reviewed worksheet")
    if dossier.get("id") != final_seams.REVIEW_ID or worksheet.get("id") != final_seams.REVIEW_ID:
        raise FinalizeError("final-seams review identity mismatch")
    if dossier.get("kind") != final_seams.PREPARED_KIND or worksheet.get("kind") != final_seams.WORKSHEET_KIND:
        raise FinalizeError("final-seams review kind mismatch")
    if dossier.get("contains_official_source_text") is not True:
        raise FinalizeError("final-seams dossier must contain the inspected source excerpts")
    if worksheet.get("contains_official_source_text") is not False:
        raise FinalizeError("finalizer accepts only a source-free final-seams worksheet")
    if dossier.get("source_archive_sha256") != SOURCE_SHA256 or worksheet.get("source_archive_sha256") != SOURCE_SHA256:
        raise FinalizeError("final-seams source pin mismatch")
    expected_prior = {
        "id": WIRE_117_ID,
        "candidate_count": WIRE_117_COUNT,
        "dossier_sha256": WIRE_117_DOSSIER_SHA256,
    }
    if dossier.get("prior_review") != expected_prior or worksheet.get("prior_review") != expected_prior:
        raise FinalizeError("final-seams prior-review commitment mismatch")
    dossier_items = dossier.get("candidates")
    worksheet_items = worksheet.get("candidates")
    if not isinstance(dossier_items, list) or not isinstance(worksheet_items, list):
        raise FinalizeError("final-seams candidates must be arrays")
    count = dossier.get("candidate_count")
    if type(count) is not int or count <= 0 or count != len(dossier_items) or count != len(worksheet_items):
        raise FinalizeError("final-seams candidate count mismatch")

    dossier_by_id: dict[str, dict[str, Any]] = {}
    for item in dossier_items:
        if not isinstance(item, dict):
            raise FinalizeError("final-seams dossier candidate must be an object")
        candidate_id = item.get("candidate_id")
        if not isinstance(candidate_id, str) or not candidate_id or candidate_id in dossier_by_id:
            raise FinalizeError("final-seams dossier candidate IDs must be unique")
        dossier_by_id[candidate_id] = item

    seen: set[str] = set()
    groups: set[str] = set()
    source_records: list[dict[str, object]] = []
    for item in worksheet_items:
        if not isinstance(item, dict):
            raise FinalizeError("final-seams worksheet candidate must be an object")
        candidate_id = item.get("candidate_id")
        if not isinstance(candidate_id, str) or candidate_id in seen or candidate_id not in dossier_by_id:
            raise FinalizeError(f"invalid/duplicate final-seams candidate id: {candidate_id!r}")
        seen.add(candidate_id)
        source_item = dossier_by_id[candidate_id]
        for field in ("source_identity", "source", "atlas_observed_hazards", "group_ids"):
            if item.get(field) != source_item.get(field):
                raise FinalizeError(f"{candidate_id}: worksheet {field} does not match source dossier")
        group_ids = item.get("group_ids")
        if not isinstance(group_ids, list) or not group_ids:
            raise FinalizeError(f"{candidate_id}: group_ids must be non-empty")
        if any(group not in EXPECTED_FINAL_SEAM_GROUPS for group in group_ids):
            raise FinalizeError(f"{candidate_id}: final-seam group escaped the frozen boundary")
        groups.update(group_ids)
        observed = item.get("atlas_observed_hazards")
        decision = item.get("decision")
        if not isinstance(observed, list) or not isinstance(decision, dict):
            raise FinalizeError(f"{candidate_id}: hazards/decision are invalid")
        if decision.get("source_inspected") is not True or decision.get("accepted") is not True:
            raise FinalizeError(f"{candidate_id}: source must be explicitly inspected and accepted")
        reviewed = decision.get("hazards_reviewed")
        if not isinstance(reviewed, list) or sorted(set(reviewed)) != sorted(set(observed)):
            raise FinalizeError(f"{candidate_id}: every observed hazard must be explicitly reviewed")
        if decision.get("followup_dependencies") != []:
            raise FinalizeError(f"{candidate_id}: unresolved final-seam dependency remains")
        observations = decision.get("semantic_observations")
        note = decision.get("note")
        if not isinstance(observations, list) or not observations or any(
            not isinstance(obs, str) or not obs.strip() for obs in observations
        ):
            raise FinalizeError(f"{candidate_id}: semantic observations are required")
        if not isinstance(note, str) or not note.strip():
            raise FinalizeError(f"{candidate_id}: review note is required")
        source_records.append(
            {
                "candidate_id": candidate_id,
                "group_ids": list(group_ids),
                "source_identity": item["source_identity"],
                "source": _canonical_source(item.get("source"), label=candidate_id),
                "hazards_reviewed": sorted(set(reviewed)),
            }
        )

    if seen != set(dossier_by_id):
        raise FinalizeError("final-seams reviewed worksheet does not cover every dossier candidate")
    if groups != EXPECTED_FINAL_SEAM_GROUPS:
        raise FinalizeError(f"final-seams groups incomplete: {sorted(groups)}")
    source_records.sort(key=lambda item: str(item["candidate_id"]))
    return {
        "id": final_seams.REVIEW_ID,
        "candidate_count": count,
        "dossier_sha256": dossier_sha,
        "worksheet_sha256": worksheet_sha,
        "group_ids": sorted(groups),
        "source_records": source_records,
    }


def _decode_varint_prefix(body: bytes) -> tuple[int, int]:
    return oracle_extract.decode_varint_prefix(body)


def validate_oracle(path: Path) -> dict[str, object]:
    value, digest = _read_json(path, label="R2B bootstrap oracle")
    if value.get("schema") != oracle_extract.SCHEMA or value.get("kind") != oracle_extract.KIND:
        raise FinalizeError("oracle identity mismatch")
    if value.get("oracle_only") is not True or value.get("production_admitted") is not False:
        raise FinalizeError("oracle must remain explicitly non-production evidence")
    target = value.get("target")
    if target != {
        "minecraft": "26.2",
        "protocol": 776,
        "source_archive_sha256": SOURCE_SHA256,
        "capture_sha256": CAPTURE_SHA256,
    }:
        raise FinalizeError("oracle target/source/capture commitment mismatch")
    artifacts = value.get("artifacts")
    if not isinstance(artifacts, list) or len(artifacts) != len(EXPECTED_ORACLE):
        raise FinalizeError("oracle must contain exactly the two composition artifacts")
    seen: set[str] = set()
    result: list[dict[str, object]] = []
    for item in artifacts:
        if not isinstance(item, dict):
            raise FinalizeError("oracle artifact must be an object")
        name = item.get("name")
        if not isinstance(name, str) or name in seen or name not in EXPECTED_ORACLE:
            raise FinalizeError(f"unexpected/duplicate oracle artifact: {name!r}")
        seen.add(name)
        if item.get("phase") != "play" or item.get("direction") != "clientbound":
            raise FinalizeError(f"{name}: oracle artifact phase/direction mismatch")
        packet_id = item.get("packet_id")
        if packet_id != EXPECTED_ORACLE[name]:
            raise FinalizeError(f"{name}: oracle packet id mismatch")
        body_hex = item.get("body_hex")
        body_sha = item.get("body_sha256")
        if not isinstance(body_hex, str) or not isinstance(body_sha, str):
            raise FinalizeError(f"{name}: oracle body hex/hash missing")
        try:
            body = bytes.fromhex(body_hex)
        except ValueError as error:
            raise FinalizeError(f"{name}: oracle body hex invalid") from error
        if len(body) != item.get("body_bytes") or _sha256(body) != body_sha:
            raise FinalizeError(f"{name}: oracle body length/hash mismatch")
        decoded_id, width = _decode_varint_prefix(body)
        if decoded_id != packet_id or width != item.get("packet_id_bytes"):
            raise FinalizeError(f"{name}: oracle body packet-id prefix mismatch")
        result.append(
            {
                "name": name,
                "semantic_group": item.get("semantic_group"),
                "packet_id": packet_id,
                "body_bytes": len(body),
                "body_sha256": body_sha,
            }
        )
    if seen != set(EXPECTED_ORACLE):
        raise FinalizeError("oracle composition artifacts incomplete")
    result.sort(key=lambda item: str(item["name"]))
    return {"sha256": digest, "artifacts": result}


def finalize(
    *,
    final_67_dossier: Path,
    wire_117_dossier: Path,
    final_seams_dossier: Path,
    final_seams_worksheet: Path,
    oracle_path: Path,
    semantic_contract: Path,
    lock_path: Path,
) -> dict[str, object]:
    semantics = validate_semantics(semantic_contract, lock_path=lock_path)
    first = validate_pinned_dossier(
        final_67_dossier,
        label="67-body Play-entry dossier",
        expected_id=FINAL_67_ID,
        expected_count=FINAL_67_COUNT,
        expected_sha256=FINAL_67_DOSSIER_SHA256,
    )
    wire = validate_pinned_dossier(
        wire_117_dossier,
        label="117-body wire dossier",
        expected_id=WIRE_117_ID,
        expected_count=WIRE_117_COUNT,
        expected_sha256=WIRE_117_DOSSIER_SHA256,
    )
    seams = validate_final_seams(final_seams_dossier, final_seams_worksheet)
    oracle = validate_oracle(oracle_path)
    return {
        "schema": SCHEMA,
        "kind": KIND,
        "status": STATUS,
        "contains_official_source_text": False,
        "production_admitted": False,
        "gate": "GATE-NET-PLAY-ENTRY-26_2-001",
        "gate_emission_ready": True,
        "target": semantics["target"],
        "semantic_contract": {
            "path": semantics["path"],
            "sha256": semantics["sha256"],
            "rules": semantics["semantic_rules"],
        },
        "source_reviews": {
            "play_entry_67": first,
            "wire_117": wire,
            "final_seams": {
                key: value for key, value in seams.items() if key != "source_records"
            },
        },
        "final_seam_source_records": seams["source_records"],
        "composition_oracle": oracle,
        "next_required_step": (
            "Generate canonical R2B VAR records, independent source gate, target facts and the "
            "selected-profile semantic bootstrap image; this manifest is verified input, not "
            "production admission."
        ),
    }


def _write_output(path: Path, value: dict[str, object]) -> None:
    if path.exists() or path.is_symlink():
        raise FinalizeError(f"output must not already exist: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.unlink(missing_ok=True)
    try:
        temporary.write_text(
            json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
        temporary.replace(path)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--final-67-dossier", type=Path, required=True)
    parser.add_argument("--wire-117-dossier", type=Path, required=True)
    parser.add_argument("--final-seams-dossier", type=Path, required=True)
    parser.add_argument("--final-seams-worksheet", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--semantic-contract", type=Path, default=DEFAULT_SEMANTICS)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        value = finalize(
            final_67_dossier=args.final_67_dossier,
            wire_117_dossier=args.wire_117_dossier,
            final_seams_dossier=args.final_seams_dossier,
            final_seams_worksheet=args.final_seams_worksheet,
            oracle_path=args.oracle,
            semantic_contract=args.semantic_contract,
            lock_path=args.lock,
        )
        _write_output(args.output, value)
    except (FinalizeError, OSError, KeyError, TypeError, ValueError) as error:
        print(f"R2B Play-entry finalization error: {error}", file=sys.stderr)
        return 2
    print(f"r2b_play_entry_admission_inputs={args.output}")
    print(f"status={STATUS}")
    print("gate_emission_ready=true")
    print("production_admitted=false")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
