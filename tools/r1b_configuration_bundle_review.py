#!/usr/bin/env python3
"""Validate an ephemeral R1B source bundle and emit a commit-safe review pack.

The source probe deliberately emits a local bundle containing one narrow official-source excerpt.
This tool is the firewall between that ephemeral evidence and version-controlled review material. It
requires every intended R1B candidate to resolve exactly once, binds all fingerprints to the pinned
source lock, checks the canonical selected frontier, and emits only fingerprints/hashes plus INDEXED
VAR drafts and a fail-closed gate skeleton.

It never upgrades source evidence to VAR_REVIEWED and never copies official source text into output.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any, Mapping, Sequence

try:
    from . import r1b_configuration_source_probe as source_probe
except ImportError:  # Direct `python3 tools/...` execution.
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]

SCHEMA = 1
REVIEW_PACK_KIND = "r1b-configuration-source-review-pack"
COMMIT_POLICY = "REVIEW_REQUIRED_BEFORE_COMMIT"
DEFAULT_LOCK = Path("vanilla/vanilla.lock.toml")
DEFAULT_FRONTIER = Path("vanilla/frontiers/r1b-configuration-selected.json")
GATE_ID = "GATE-NET-CONFIG-26_2-001"
FRONTIER_NAME = "r1b-configuration-selected"
RECORD_DIR = Path("vanilla/records/network/r1/configuration")
GATE_PATH = Path("vanilla/gates/network") / f"{GATE_ID}.json"
HEX_256 = re.compile(r"^[0-9a-f]{64}$")

BUNDLE_KEYS = {
    "schema",
    "kind",
    "commit_policy",
    "contains_official_source_text",
    "source_archive_sha256",
    "frontier",
    "frontier_roots",
    "var_candidates",
    "play_bootstrap_source",
    "summary",
}
TEMPLATE_KEYS = {
    "schema",
    "id",
    "status",
    "source",
    "classifications",
    "hazards_reviewed",
    "semantic_rules",
    "evidence",
    "notes",
    "atlas_observed_hazards",
}
SOURCE_KEYS = {
    "type",
    "signature",
    "fingerprint_algorithm",
    "normalized_sha256",
    "body_sha256",
}


class ReviewPackError(RuntimeError):
    """Fail-closed bundle/review-pack validation error."""


def canonical_bytes(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n"
    ).encode("utf-8")


def pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode(
        "utf-8"
    )


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ReviewPackError(f"{label} must be a JSON object")
    return value


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ReviewPackError(f"{label} must be a non-empty string")
    return value


def _int(value: object, label: str) -> int:
    if type(value) is not int:
        raise ReviewPackError(f"{label} must be an integer")
    return value


def _string_list(value: object, label: str) -> list[str]:
    if not isinstance(value, list) or any(not isinstance(item, str) or not item for item in value):
        raise ReviewPackError(f"{label} must be an array of non-empty strings")
    return [str(item) for item in value]


def _empty_list(value: object, label: str) -> None:
    if value != []:
        raise ReviewPackError(f"{label} must remain empty before manual review")


def _sha256(value: object, label: str) -> str:
    text = _string(value, label)
    if HEX_256.fullmatch(text) is None:
        raise ReviewPackError(f"{label} must be a lowercase SHA-256 hex digest")
    return text


def _load_json(path: Path, label: str) -> tuple[dict[str, Any], bytes]:
    if path.is_symlink() or not path.is_file():
        raise ReviewPackError(f"{label} must be a real non-symlink file: {path}")
    raw = path.read_bytes()
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        raise ReviewPackError(f"invalid {label} JSON: {error}") from error
    return _object(value, label), raw


def _load_lock(path: Path) -> dict[str, object]:
    if path.is_symlink() or not path.is_file():
        raise ReviewPackError(f"source lock must be a real non-symlink file: {path}")
    try:
        lock = tomllib.loads(path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as error:
        raise ReviewPackError(f"invalid source lock TOML: {error}") from error
    source = _object(lock.get("source"), "lock.source")
    atlas = _object(lock.get("atlas"), "lock.atlas")
    protocol = _int(lock.get("protocol"), "lock.protocol")
    data_version = _int(lock.get("data_version"), "lock.data_version")
    return {
        "minecraft": _string(lock.get("minecraft"), "lock.minecraft"),
        "protocol": protocol,
        "data_version": data_version,
        "source_archive_sha256": _sha256(source.get("archive_sha256"), "lock.source.archive_sha256"),
        "fingerprint_algorithm": _string(
            atlas.get("fingerprint_algorithm"), "lock.atlas.fingerprint_algorithm"
        ),
    }


def _frontier_roots(path: Path) -> tuple[list[str], str]:
    frontier, raw = _load_json(path, "frontier")
    roots = _string_list(frontier.get("root_queries"), "frontier.root_queries")
    if not roots:
        raise ReviewPackError("frontier.root_queries must not be empty")
    return roots, sha256_bytes(raw)


def _validate_template(
    template_value: object,
    *,
    expected_id: str,
    fingerprint_algorithm: str,
    label: str,
) -> tuple[dict[str, object], list[str]]:
    template = _object(template_value, label)
    if set(template) != TEMPLATE_KEYS:
        unknown = sorted(set(template) - TEMPLATE_KEYS)
        missing = sorted(TEMPLATE_KEYS - set(template))
        raise ReviewPackError(
            f"{label} keys differ from the source-probe template; unknown={unknown}, missing={missing}"
        )
    if template.get("schema") != SCHEMA:
        raise ReviewPackError(f"{label}.schema must be {SCHEMA}")
    if template.get("id") != expected_id:
        raise ReviewPackError(f"{label}.id does not match {expected_id}")
    if template.get("status") != "INDEXED":
        raise ReviewPackError(f"{label}.status must remain INDEXED before manual review")

    source = _object(template.get("source"), f"{label}.source")
    if set(source) != SOURCE_KEYS:
        raise ReviewPackError(f"{label}.source must contain exactly {sorted(SOURCE_KEYS)}")
    source_type = _string(source.get("type"), f"{label}.source.type")
    signature = _string(source.get("signature"), f"{label}.source.signature")
    algorithm = _string(
        source.get("fingerprint_algorithm"), f"{label}.source.fingerprint_algorithm"
    )
    if algorithm != fingerprint_algorithm:
        raise ReviewPackError(f"{label} fingerprint algorithm does not match source lock")
    normalized = _sha256(source.get("normalized_sha256"), f"{label}.source.normalized_sha256")
    body = _sha256(source.get("body_sha256"), f"{label}.source.body_sha256")

    classifications = _string_list(template.get("classifications"), f"{label}.classifications")
    observed_hazards = _string_list(
        template.get("atlas_observed_hazards"), f"{label}.atlas_observed_hazards"
    )
    _empty_list(template.get("hazards_reviewed"), f"{label}.hazards_reviewed")
    _empty_list(template.get("semantic_rules"), f"{label}.semantic_rules")
    _empty_list(template.get("evidence"), f"{label}.evidence")
    _empty_list(template.get("notes"), f"{label}.notes")

    draft = {
        "schema": SCHEMA,
        "id": expected_id,
        "status": "INDEXED",
        "source": {
            "type": source_type,
            "signature": signature,
            "fingerprint_algorithm": algorithm,
            "normalized_sha256": normalized,
            "body_sha256": body,
        },
        "classifications": sorted(set(classifications)),
        "hazards_reviewed": [],
        "semantic_rules": [],
        "evidence": [],
        "notes": [],
    }
    return draft, sorted(set(observed_hazards))


def validate_bundle(
    *,
    bundle_path: Path,
    lock_path: Path = DEFAULT_LOCK,
    frontier_path: Path = DEFAULT_FRONTIER,
) -> dict[str, object]:
    """Validate one source-probe bundle and return a source-text-free review model."""
    bundle, bundle_raw = _load_json(bundle_path, "R1B admission bundle")
    if set(bundle) != BUNDLE_KEYS:
        unknown = sorted(set(bundle) - BUNDLE_KEYS)
        missing = sorted(BUNDLE_KEYS - set(bundle))
        raise ReviewPackError(f"bundle keys differ from schema; unknown={unknown}, missing={missing}")
    if bundle.get("schema") != SCHEMA:
        raise ReviewPackError(f"bundle.schema must be {SCHEMA}")
    if bundle.get("kind") != source_probe.BUNDLE_KIND:
        raise ReviewPackError(f"bundle.kind must be {source_probe.BUNDLE_KIND}")
    if bundle.get("commit_policy") != "EPHEMERAL_DO_NOT_COMMIT":
        raise ReviewPackError("bundle.commit_policy must remain EPHEMERAL_DO_NOT_COMMIT")
    if bundle.get("contains_official_source_text") is not True:
        raise ReviewPackError("bundle must explicitly declare contained official source text")

    lock = _load_lock(lock_path)
    source_sha = _sha256(bundle.get("source_archive_sha256"), "bundle.source_archive_sha256")
    if source_sha != lock["source_archive_sha256"]:
        raise ReviewPackError("bundle source archive SHA-256 does not match vanilla source lock")

    canonical_roots, frontier_sha = _frontier_roots(frontier_path)
    bundle_frontier = _string(bundle.get("frontier"), "bundle.frontier")
    if Path(bundle_frontier).name != frontier_path.name:
        raise ReviewPackError("bundle frontier does not name the selected R1B frontier")
    root_items = bundle.get("frontier_roots")
    if not isinstance(root_items, list):
        raise ReviewPackError("bundle.frontier_roots must be an array")
    observed_root_queries: list[str] = []
    root_match_counts: list[int] = []
    for index, value in enumerate(root_items):
        item = _object(value, f"bundle.frontier_roots[{index}]")
        if set(item) != {"query", "matches"}:
            raise ReviewPackError(f"bundle.frontier_roots[{index}] must contain query and matches")
        observed_root_queries.append(_string(item.get("query"), f"bundle.frontier_roots[{index}].query"))
        matches = _string_list(item.get("matches"), f"bundle.frontier_roots[{index}].matches")
        if not matches:
            raise ReviewPackError(f"bundle.frontier_roots[{index}] resolved no methods")
        root_match_counts.append(len(matches))
    if observed_root_queries != canonical_roots:
        raise ReviewPackError("bundle frontier roots do not exactly match the selected frontier")

    candidates_value = bundle.get("var_candidates")
    if not isinstance(candidates_value, list):
        raise ReviewPackError("bundle.var_candidates must be an array")
    expected = list(source_probe.CANDIDATES)
    if len(candidates_value) != len(expected):
        raise ReviewPackError(
            f"bundle must contain exactly {len(expected)} R1B VAR candidates, got {len(candidates_value)}"
        )

    review_candidates: list[dict[str, object]] = []
    draft_records: list[dict[str, object]] = []
    seen_sources: set[str] = set()
    for index, ((expected_id, expected_query), value) in enumerate(zip(expected, candidates_value)):
        item = _object(value, f"bundle.var_candidates[{index}]")
        if item.get("var_id") != expected_id or item.get("query") != expected_query:
            raise ReviewPackError(
                f"bundle.var_candidates[{index}] does not match expected {expected_id} / {expected_query}"
            )
        if _int(item.get("match_count"), f"bundle.var_candidates[{index}].match_count") != 1:
            raise ReviewPackError(f"{expected_id} must resolve exactly once before review-pack emission")
        if "candidates" in item:
            raise ReviewPackError(f"{expected_id} still carries ambiguous candidate identities")
        if "record_template" not in item:
            raise ReviewPackError(f"{expected_id} is missing its unique record_template")
        if set(item) != {"var_id", "query", "match_count", "record_template"}:
            raise ReviewPackError(f"{expected_id} candidate contains unexpected fields")
        draft, observed_hazards = _validate_template(
            item["record_template"],
            expected_id=expected_id,
            fingerprint_algorithm=str(lock["fingerprint_algorithm"]),
            label=f"bundle.var_candidates[{index}].record_template",
        )
        source = _object(draft["source"], f"draft {expected_id} source")
        source_identity = f"{source['type']}#{source['signature']}"
        if source_identity in seen_sources:
            raise ReviewPackError(f"duplicate R1B source identity across candidates: {source_identity}")
        seen_sources.add(source_identity)
        draft_records.append(draft)
        review_candidates.append(
            {
                "var_id": expected_id,
                "query": expected_query,
                "source": dict(source),
                "classifications": list(draft["classifications"]),
                "atlas_observed_hazards": observed_hazards,
                "suggested_record_path": str(RECORD_DIR / f"{expected_id}.json"),
            }
        )

    summary = _object(bundle.get("summary"), "bundle.summary")
    expected_summary = {
        "roots_ok": len(canonical_roots),
        "record_templates_emitted": len(expected),
        "record_templates_needing_refinement": 0,
    }
    if summary != expected_summary:
        raise ReviewPackError(f"bundle.summary must equal {expected_summary}")

    bootstrap = _object(bundle.get("play_bootstrap_source"), "bundle.play_bootstrap_source")
    if set(bootstrap) != {"path", "owner", "method", "source"}:
        raise ReviewPackError("bundle.play_bootstrap_source has unexpected fields")
    if bootstrap.get("path") != source_probe.PLAYER_LIST_PATH:
        raise ReviewPackError("play bootstrap source path does not match PlayerList source member")
    if bootstrap.get("owner") != "PlayerList" or bootstrap.get("method") != "placeNewPlayer":
        raise ReviewPackError("play bootstrap source does not identify PlayerList.placeNewPlayer")
    excerpt = _string(bootstrap.get("source"), "bundle.play_bootstrap_source.source")

    gate = {
        "schema": SCHEMA,
        "id": GATE_ID,
        "frontier": FRONTIER_NAME,
        "minimum_status": "VAR_REVIEWED",
        "require_semantic_rules": True,
        "require_hazards_reviewed": True,
        "methods": [
            {"query": query, "var_id": var_id} for var_id, query in source_probe.CANDIDATES
        ],
    }
    return {
        "lock": lock,
        "bundle_sha256": sha256_bytes(bundle_raw),
        "frontier_sha256": frontier_sha,
        "frontier_roots": canonical_roots,
        "frontier_root_match_counts": root_match_counts,
        "play_bootstrap_source_sha256": sha256_bytes(excerpt.encode("utf-8")),
        "review_candidates": review_candidates,
        "draft_records": draft_records,
        "gate": gate,
    }


def materialize_review_pack(
    *,
    bundle_path: Path,
    output_dir: Path,
    lock_path: Path = DEFAULT_LOCK,
    frontier_path: Path = DEFAULT_FRONTIER,
) -> dict[str, object]:
    """Validate a bundle and create a deterministic source-text-free review directory."""
    model = validate_bundle(bundle_path=bundle_path, lock_path=lock_path, frontier_path=frontier_path)
    if output_dir.exists():
        raise ReviewPackError(f"output directory must not already exist: {output_dir}")
    output_dir.mkdir(parents=True)
    records_dir = output_dir / "records"
    records_dir.mkdir()

    record_files: list[dict[str, str]] = []
    for record in model["draft_records"]:
        assert isinstance(record, dict)
        var_id = str(record["id"])
        relative = Path("records") / f"{var_id}.json"
        raw = pretty_bytes(record)
        (output_dir / relative).write_bytes(raw)
        record_files.append(
            {"var_id": var_id, "path": str(relative), "sha256": sha256_bytes(raw)}
        )

    gate = model["gate"]
    gate_relative = Path("gate") / f"{GATE_ID}.json"
    (output_dir / "gate").mkdir()
    gate_raw = pretty_bytes(gate)
    (output_dir / gate_relative).write_bytes(gate_raw)

    manifest = {
        "schema": SCHEMA,
        "kind": REVIEW_PACK_KIND,
        "commit_policy": COMMIT_POLICY,
        "contains_official_source_text": False,
        "source": dict(model["lock"]),
        "ephemeral_bundle_sha256": model["bundle_sha256"],
        "frontier": {
            "name": FRONTIER_NAME,
            "path": str(frontier_path),
            "sha256": model["frontier_sha256"],
            "root_queries": model["frontier_roots"],
            "root_match_counts": model["frontier_root_match_counts"],
        },
        "play_bootstrap": {
            "path": source_probe.PLAYER_LIST_PATH,
            "owner": "PlayerList",
            "method": "placeNewPlayer",
            "source_excerpt_sha256": model["play_bootstrap_source_sha256"],
        },
        "review_candidates": model["review_candidates"],
        "generated": {
            "record_files": record_files,
            "gate_file": {
                "path": str(gate_relative),
                "sha256": sha256_bytes(gate_raw),
                "suggested_repository_path": str(GATE_PATH),
            },
        },
        "review_requirements": {
            "records_remain_indexed": True,
            "manual_var_review_required": True,
            "manual_hazard_review_required": True,
            "semantic_rule_linkage_required": True,
            "gate_must_be_rerun_against_pinned_atlas": True,
        },
    }
    manifest_raw = pretty_bytes(manifest)
    (output_dir / "manifest.json").write_bytes(manifest_raw)
    return manifest


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-configuration-bundle-review")
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--frontier", type=Path, default=DEFAULT_FRONTIER)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        manifest = materialize_review_pack(
            bundle_path=args.bundle,
            output_dir=args.output_dir,
            lock_path=args.lock,
            frontier_path=args.frontier,
        )
    except (OSError, ReviewPackError) as error:
        print(f"R1B Configuration bundle review error: {error}", file=sys.stderr)
        return 2
    print(f"review_pack={args.output_dir}")
    print(f"ephemeral_bundle_sha256={manifest['ephemeral_bundle_sha256']}")
    print("contains_official_source_text=false")
    print("records_status=INDEXED")
    print("manual_review_required=true")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
