#!/usr/bin/env python3
"""Verify an admitted representative population before target-hardware measurement."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import stat
import tomllib
from pathlib import Path, PurePosixPath
from typing import Any

import section_representative_plan

ARTIFACT_SCHEMA = 2
ARTIFACT_KIND = "section-representative-set-workflow-artifact"
ADMISSION_SCHEMA = 1
ADMISSION_KIND = "section-representative-set-admission"
SET_SCHEMA = 1
SET_KIND = "section-corpus-set"
POPULATION_KIND = "section-corpus-population-identity"
RUN_PLAN_SCHEMA = 1
RUN_PLAN_KIND = "section-performance-run-plan"
RUN_POLICY = "section-target-hardware-v1"
REPRESENTATIVE_POLICY = "vanilla-section-representative-v1"
MEMBER_EXTRACTOR = "vanilla-save-region-v2-representative-member"
CORPUS_MAGIC = "CRUCIBLE-SECTION-CORPUS|1"
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
COMMIT_SHA = re.compile(r"[0-9a-f]{40}\Z")
POSITIVE_DECIMAL = re.compile(r"[1-9][0-9]*\Z")
EXPECTED_CANDIDATES = (
    ("direct-reference", False),
    ("direct", True),
    ("adaptive", True),
    ("fast-local", True),
    ("packed-local", True),
)
EXPECTED_LATTICE = {
    "minecraft:overworld": {"min_section_y": -4, "max_section_y": 19, "section_count": 24},
    "minecraft:the_nether": {"min_section_y": 0, "max_section_y": 15, "section_count": 16},
    "minecraft:the_end": {"min_section_y": 0, "max_section_y": 15, "section_count": 16},
}
EXPECTED_CHUNKS_PER_MEMBER_DIMENSION = 64
EXPECTED_MEMBER_COUNT = 4
DEFAULT_STATE_MANIFEST = Path("vanilla/state-data/26.2-state-data-manifest.json")
DEFAULT_LOCK = Path("vanilla/vanilla.lock.toml")


class PerformanceInputError(ValueError):
    """Raised when benchmark handoff evidence is incomplete or inconsistent."""


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise PerformanceInputError(f"{label} must be a JSON object")
    return value


def _list(value: object, label: str) -> list[Any]:
    if not isinstance(value, list):
        raise PerformanceInputError(f"{label} must be a JSON array")
    return value


def _integer(mapping: dict[str, Any], key: str, label: str) -> int:
    value = mapping.get(key)
    if isinstance(value, bool) or not isinstance(value, int):
        raise PerformanceInputError(f"{label}.{key} must be an integer")
    return value


def _string(mapping: dict[str, Any], key: str, label: str) -> str:
    value = mapping.get(key)
    if not isinstance(value, str) or not value:
        raise PerformanceInputError(f"{label}.{key} must be a non-empty string")
    return value


def _boolean(mapping: dict[str, Any], key: str, label: str) -> bool:
    value = mapping.get(key)
    if not isinstance(value, bool):
        raise PerformanceInputError(f"{label}.{key} must be a boolean")
    return value


def _sha(value: object, label: str) -> str:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise PerformanceInputError(f"{label} must be canonical lowercase SHA-256")
    return value


def _equal(actual: object, expected: object, label: str) -> None:
    if actual != expected:
        raise PerformanceInputError(
            f"{label} mismatch: expected {expected!r}, got {actual!r}"
        )


def _canonical_digest(value: object) -> str:
    payload = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def _file_sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _load_json(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise PerformanceInputError(f"could not load {label} {path}: {error}") from error
    return _object(value, label)


def _verify_canonical_record_digest(
    record: dict[str, Any], digest_field: str, label: str
) -> str:
    expected = _sha(record.get(digest_field), f"{label}.{digest_field}")
    payload = dict(record)
    payload.pop(digest_field)
    _equal(_canonical_digest(payload), expected, f"{label} canonical digest")
    return expected


def _canonical_inventory_path(raw: object) -> str:
    if not isinstance(raw, str) or not raw or "\\" in raw:
        raise PerformanceInputError(f"artifact inventory path is not canonical: {raw!r}")
    path = PurePosixPath(raw)
    if path.is_absolute() or path.as_posix() != raw:
        raise PerformanceInputError(f"artifact inventory path is not canonical: {raw!r}")
    if any(part in ("", ".", "..") for part in path.parts):
        raise PerformanceInputError(f"artifact inventory path escapes/aliases root: {raw!r}")
    return raw


def _validate_artifact_inventory(
    root: Path, artifact: dict[str, Any]
) -> dict[str, dict[str, object]]:
    entries = _list(artifact.get("files"), "artifact files")
    observed: dict[str, dict[str, object]] = {}
    for index, raw_entry in enumerate(entries):
        label = f"artifact files[{index}]"
        entry = _object(raw_entry, label)
        relative = _canonical_inventory_path(entry.get("path"))
        if relative in observed:
            raise PerformanceInputError(f"duplicate artifact inventory path: {relative}")
        size = _integer(entry, "size", label)
        if size < 0:
            raise PerformanceInputError(f"{label}.size must be non-negative")
        digest = _sha(entry.get("sha256"), f"{label}.sha256")
        path = root / relative
        try:
            mode = path.lstat().st_mode
        except OSError as error:
            raise PerformanceInputError(f"missing inventoried file {relative}: {error}") from error
        if stat.S_ISLNK(mode) or not stat.S_ISREG(mode):
            raise PerformanceInputError(f"inventoried path must be a regular non-symlink file: {relative}")
        _equal(path.stat().st_size, size, f"artifact size {relative}")
        _equal(_file_sha(path), digest, f"artifact SHA-256 {relative}")
        observed[relative] = {"size": size, "sha256": digest}

    actual: set[str] = set()
    for path in root.rglob("*"):
        if path.is_symlink():
            raise PerformanceInputError(
                f"handoff directory must not contain symlinks: {path.relative_to(root)}"
            )
        if path.is_file() and path.name != "artifact-manifest.json":
            relative = path.relative_to(root).as_posix()
            actual.add(relative)
    _equal(actual, set(observed), "artifact inventory / directory file set")
    return observed


def _expected_target(state_manifest: dict[str, Any]) -> dict[str, object]:
    target = _object(state_manifest.get("target"), "state manifest target")
    return {
        "minecraft_version": _string(target, "minecraft_version", "state manifest target"),
        "protocol_version": _integer(target, "protocol_version", "state manifest target"),
        "data_version": _integer(target, "data_version", "state manifest target"),
        "state_count": _integer(state_manifest, "state_count", "state manifest"),
        "state_data_generation_sha256": _sha(
            state_manifest.get("generation_digest"), "state manifest generation_digest"
        ),
        "state_data_input_sha256": _sha(
            state_manifest.get("input_digest"), "state manifest input_digest"
        ),
    }


def _verify_artifact_manifest(root: Path) -> tuple[dict[str, Any], dict[str, dict[str, object]], str]:
    artifact = _load_json(root / "artifact-manifest.json", "artifact manifest")
    _equal(_integer(artifact, "schema", "artifact manifest"), ARTIFACT_SCHEMA, "artifact schema")
    _equal(_string(artifact, "kind", "artifact manifest"), ARTIFACT_KIND, "artifact kind")
    for flag in ("qualification_complete", "decision_eligible", "benchmark_handoff_eligible"):
        _equal(_boolean(artifact, flag, "artifact manifest"), True, f"artifact {flag}")
    manifest_sha = _verify_canonical_record_digest(
        artifact, "manifest_sha256", "artifact manifest"
    )

    provenance = _object(artifact.get("provenance"), "artifact provenance")
    commit = _string(provenance, "repository_commit_sha", "artifact provenance")
    if COMMIT_SHA.fullmatch(commit) is None:
        raise PerformanceInputError("artifact repository commit must be lowercase 40-hex SHA")
    for key in ("github_run_id", "github_run_attempt"):
        value = _string(provenance, key, "artifact provenance")
        if POSITIVE_DECIMAL.fullmatch(value) is None:
            raise PerformanceInputError(f"artifact provenance {key} must be a positive decimal")
    for key in ("python_version", "rustc_version", "java_version"):
        _string(provenance, key, "artifact provenance")

    identities = _object(artifact.get("identities"), "artifact identities")
    _equal(
        set(identities),
        {"population_sha256", "set_evidence_sha256", "set_file_sha256", "admission_sha256"},
        "artifact identity key set",
    )
    for key, value in identities.items():
        _sha(value, f"artifact identities.{key}")

    inventory = _validate_artifact_inventory(root, artifact)
    return artifact, inventory, manifest_sha


def _verify_admission(
    root: Path, artifact: dict[str, Any], inventory: dict[str, dict[str, object]]
) -> tuple[dict[str, Any], str]:
    relative = "population-admission.json"
    if relative not in inventory:
        raise PerformanceInputError("artifact is missing population-admission.json")
    admission = _load_json(root / relative, "population admission")
    _equal(_integer(admission, "schema", "population admission"), ADMISSION_SCHEMA, "admission schema")
    _equal(_string(admission, "kind", "population admission"), ADMISSION_KIND, "admission kind")
    _equal(admission.get("policy"), REPRESENTATIVE_POLICY, "admission policy")
    for flag in ("decision_eligible", "benchmark_handoff_eligible"):
        _equal(_boolean(admission, flag, "population admission"), True, f"admission {flag}")
    _equal(admission.get("decision_scope"), "dimension-separated-only", "admission decision scope")
    _equal(
        _boolean(admission, "cross_dimension_score_allowed", "population admission"),
        False,
        "admission cross-dimension score guard",
    )
    _equal(
        _integer(admission, "member_count", "population admission"),
        EXPECTED_MEMBER_COUNT,
        "admission member count",
    )
    admission_sha = _verify_canonical_record_digest(
        admission, "admission_sha256", "population admission"
    )
    identities = _object(artifact.get("identities"), "artifact identities")
    _equal(admission_sha, identities.get("admission_sha256"), "artifact/admission digest")
    for admission_key, artifact_key in (
        ("population_sha256", "population_sha256"),
        ("set_evidence_sha256", "set_evidence_sha256"),
        ("set_file_sha256", "set_file_sha256"),
    ):
        _equal(
            _sha(admission.get(admission_key), f"admission {admission_key}"),
            identities.get(artifact_key),
            f"artifact/admission {admission_key}",
        )
    return admission, admission_sha


def _verify_set(
    root: Path,
    artifact: dict[str, Any],
    admission: dict[str, Any],
    inventory: dict[str, dict[str, object]],
    expected_target: dict[str, object],
    expected_server_sha: str,
) -> dict[str, Any]:
    relative = "corpus-set.json"
    if relative not in inventory:
        raise PerformanceInputError("artifact is missing corpus-set.json")
    path = root / relative
    set_record = _load_json(path, "corpus set")
    _equal(_integer(set_record, "schema", "corpus set"), SET_SCHEMA, "corpus set schema")
    _equal(_string(set_record, "kind", "corpus set"), SET_KIND, "corpus set kind")
    _equal(set_record.get("policy"), REPRESENTATIVE_POLICY, "corpus set policy")
    _equal(_boolean(set_record, "decision_eligible", "corpus set"), True, "corpus set decision eligibility")
    _equal(set_record.get("decision_scope"), "dimension-separated-only", "corpus set decision scope")
    _equal(
        _boolean(set_record, "cross_dimension_score_allowed", "corpus set"),
        False,
        "corpus set cross-dimension score guard",
    )
    _equal(_integer(set_record, "member_count", "corpus set"), EXPECTED_MEMBER_COUNT, "corpus set member count")

    raw_sha = _file_sha(path)
    _equal(raw_sha, admission.get("set_file_sha256"), "admission/raw corpus-set SHA-256")
    identities = _object(artifact.get("identities"), "artifact identities")
    _equal(raw_sha, identities.get("set_file_sha256"), "artifact/raw corpus-set SHA-256")

    evidence_sha = _verify_canonical_record_digest(
        set_record, "evidence_sha256", "corpus set"
    )
    _equal(evidence_sha, admission.get("set_evidence_sha256"), "admission/set evidence digest")
    _equal(evidence_sha, identities.get("set_evidence_sha256"), "artifact/set evidence digest")

    population_identity = _object(set_record.get("population_identity"), "population identity")
    _equal(population_identity.get("kind"), POPULATION_KIND, "population identity kind")
    population_sha = _canonical_digest(population_identity)
    _equal(population_sha, set_record.get("population_sha256"), "population identity digest")
    _equal(population_sha, admission.get("population_sha256"), "admission population digest")
    _equal(population_sha, identities.get("population_sha256"), "artifact population digest")

    _equal(set_record.get("target"), expected_target, "corpus-set target identity")
    _equal(population_identity.get("target"), expected_target, "population target identity")
    _equal(_sha(set_record.get("server_sha256"), "corpus set server_sha256"), expected_server_sha, "corpus-set server identity")
    _equal(_sha(population_identity.get("server_sha256"), "population server_sha256"), expected_server_sha, "population server identity")

    weighting = _object(set_record.get("weighting"), "corpus set weighting")
    expected_weighting = {
        "seed": "equal",
        "dimension": "report-separately",
        "section": "natural-within-selected-generated-chunks",
    }
    _equal(weighting, expected_weighting, "corpus set weighting")
    _equal(population_identity.get("weighting"), expected_weighting, "population weighting")
    _equal(set_record.get("section_lattice"), EXPECTED_LATTICE, "target section lattice")
    _equal(population_identity.get("section_lattice"), EXPECTED_LATTICE, "population target section lattice")
    return set_record


def _verify_plan(root: Path, admission: dict[str, Any], set_record: dict[str, Any]) -> dict[str, object]:
    path = root / "representative-plan.json"
    try:
        plan = section_representative_plan.load_plan(path)
    except (OSError, json.JSONDecodeError, section_representative_plan.PlanError) as error:
        raise PerformanceInputError(f"invalid representative plan: {error}") from error
    _equal(plan.get("policy"), REPRESENTATIVE_POLICY, "representative plan policy")
    _equal(plan.get("plan_sha256"), admission.get("plan_sha256"), "plan/admission digest")
    _equal(plan.get("plan_sha256"), set_record.get("plan_sha256"), "plan/set digest")
    return plan


def _parse_corpus_headers(path: Path) -> tuple[str, str, str]:
    try:
        with path.open("r", encoding="utf-8", newline="") as handle:
            lines = [handle.readline() for _ in range(3)]
    except (OSError, UnicodeDecodeError) as error:
        raise PerformanceInputError(f"could not read corpus headers {path}: {error}") from error
    if any(not line.endswith("\n") or "\r" in line for line in lines):
        raise PerformanceInputError(f"corpus {path} has noncanonical/missing header line endings")
    return tuple(line[:-1] for line in lines)  # type: ignore[return-value]


def _expected_target_header(target: dict[str, object]) -> str:
    return (
        "TARGET|"
        f"minecraft={target['minecraft_version']}|"
        f"protocol={target['protocol_version']}|"
        f"data={target['data_version']}|"
        f"state_count={target['state_count']}|"
        f"generation_sha256={target['state_data_generation_sha256']}"
    )


def _verify_members(
    root: Path,
    admission: dict[str, Any],
    set_record: dict[str, Any],
    plan: dict[str, object],
    inventory: dict[str, dict[str, object]],
    target: dict[str, object],
) -> list[dict[str, object]]:
    admission_members = _list(admission.get("members"), "admission members")
    set_members = _list(set_record.get("members"), "corpus set members")
    identity = _object(set_record.get("population_identity"), "population identity")
    identity_members = _list(identity.get("members"), "population identity members")
    seeds = _list(plan.get("seeds"), "representative plan seeds")
    if not (
        len(admission_members)
        == len(set_members)
        == len(identity_members)
        == len(seeds)
        == EXPECTED_MEMBER_COUNT
    ):
        raise PerformanceInputError("performance population must contain exactly four aligned members")

    expected_target_header = _expected_target_header(target)
    seen_corpora: set[str] = set()
    result: list[dict[str, object]] = []
    for seed_index in range(EXPECTED_MEMBER_COUNT):
        expected_seed = int(seeds[seed_index])
        admission_member = _object(admission_members[seed_index], f"admission member[{seed_index}]")
        set_member = _object(set_members[seed_index], f"set member[{seed_index}]")
        identity_member = _object(identity_members[seed_index], f"identity member[{seed_index}]")
        for label, member in (
            ("admission", admission_member),
            ("set", set_member),
            ("identity", identity_member),
        ):
            _equal(_integer(member, "seed_index", f"{label} member"), seed_index, f"{label} seed index")
            _equal(_integer(member, "seed", f"{label} member"), expected_seed, f"{label} seed")
        corpus_sha = _sha(admission_member.get("corpus_sha256"), f"admission member[{seed_index}] corpus")
        _equal(corpus_sha, set_member.get("corpus_sha256"), f"admission/set corpus seed {seed_index}")
        _equal(corpus_sha, identity_member.get("corpus_sha256"), f"admission/identity corpus seed {seed_index}")
        if corpus_sha in seen_corpora:
            raise PerformanceInputError("representative member corpus identities must be distinct")
        seen_corpora.add(corpus_sha)
        _sha(admission_member.get("server_properties_sha256"), f"admission member[{seed_index}] server properties")

        relative = f"seed-{seed_index}/member.corpus"
        if relative not in inventory:
            raise PerformanceInputError(f"artifact is missing {relative}")
        path = root / relative
        _equal(_file_sha(path), corpus_sha, f"member corpus raw SHA seed {seed_index}")
        _equal(inventory[relative]["sha256"], corpus_sha, f"artifact/member corpus SHA seed {seed_index}")

        magic, target_header, source_header = _parse_corpus_headers(path)
        _equal(magic, CORPUS_MAGIC, f"member corpus magic seed {seed_index}")
        _equal(target_header, expected_target_header, f"member corpus target seed {seed_index}")
        source_inventory = _sha(set_member.get("source_inventory_sha256"), f"set member[{seed_index}] source inventory")
        expected_source = (
            "SOURCE|kind=vanilla-save|"
            f"inventory_sha256={source_inventory}|"
            f"extractor={MEMBER_EXTRACTOR}"
        )
        _equal(source_header, expected_source, f"member corpus source header seed {seed_index}")
        result.append(
            {
                "seed_index": seed_index,
                "seed": expected_seed,
                "corpus_path": relative,
                "corpus_sha256": corpus_sha,
            }
        )
    return result


def _verify_dimensions(admission: dict[str, Any], set_record: dict[str, Any]) -> dict[str, dict[str, object]]:
    admission_dimensions = _object(admission.get("per_dimension"), "admission per_dimension")
    set_dimensions = _object(set_record.get("per_dimension"), "corpus set per_dimension")
    expected_keys = set(EXPECTED_LATTICE)
    _equal(set(admission_dimensions), expected_keys, "admission dimension set")
    _equal(set(set_dimensions), expected_keys, "corpus-set dimension set")
    result: dict[str, dict[str, object]] = {}
    for dimension in sorted(expected_keys):
        admitted = _object(admission_dimensions[dimension], f"admission {dimension}")
        structural = _object(set_dimensions[dimension], f"corpus set {dimension}")
        per_member_sections = EXPECTED_CHUNKS_PER_MEMBER_DIMENSION * EXPECTED_LATTICE[dimension]["section_count"]
        expected_sections = EXPECTED_MEMBER_COUNT * per_member_sections
        expected_cells = expected_sections * 4096
        for label, summary in (("admission", admitted), ("corpus set", structural)):
            _equal(_integer(summary, "section_count", f"{label} {dimension}"), expected_sections, f"{label} {dimension} section count")
            _equal(_integer(summary, "total_cells", f"{label} {dimension}"), expected_cells, f"{label} {dimension} total cells")
        _equal(structural.get("seed_weighting"), "equal", f"{dimension} seed weighting")
        _equal(_integer(structural, "member_count", f"corpus set {dimension}"), EXPECTED_MEMBER_COUNT, f"{dimension} member count")
        result[dimension] = {
            "section_count": expected_sections,
            "total_cells": expected_cells,
            "sections_per_seed": per_member_sections,
        }
    return result


def build_run_plan(
    artifact_root: Path,
    *,
    state_manifest_path: Path = DEFAULT_STATE_MANIFEST,
    lock_path: Path = DEFAULT_LOCK,
) -> dict[str, object]:
    root = artifact_root.resolve()
    if not root.is_dir():
        raise PerformanceInputError(f"artifact root is not a directory: {artifact_root}")
    state_manifest = _load_json(state_manifest_path, "committed state manifest")
    target = _expected_target(state_manifest)
    try:
        with lock_path.open("rb") as handle:
            lock = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise PerformanceInputError(f"could not load vanilla lock {lock_path}: {error}") from error
    runtime = _object(lock.get("runtime"), "vanilla lock runtime")
    server_sha = _sha(runtime.get("server_sha256"), "vanilla lock server_sha256")

    artifact, inventory, artifact_manifest_sha = _verify_artifact_manifest(root)
    admission, admission_sha = _verify_admission(root, artifact, inventory)
    set_record = _verify_set(root, artifact, admission, inventory, target, server_sha)
    plan = _verify_plan(root, admission, set_record)
    members = _verify_members(root, admission, set_record, plan, inventory, target)
    dimensions = _verify_dimensions(admission, set_record)

    provenance = _object(artifact.get("provenance"), "artifact provenance")
    run_plan: dict[str, object] = {
        "schema": RUN_PLAN_SCHEMA,
        "kind": RUN_PLAN_KIND,
        "benchmark_policy": RUN_POLICY,
        "qualification_ready": True,
        "target": target,
        "population_sha256": admission["population_sha256"],
        "admission_sha256": admission_sha,
        "set_evidence_sha256": admission["set_evidence_sha256"],
        "artifact_manifest_sha256": artifact_manifest_sha,
        "source_qualification": {
            "repository_commit_sha": provenance["repository_commit_sha"],
            "github_run_id": provenance["github_run_id"],
            "github_run_attempt": provenance["github_run_attempt"],
        },
        "decision_scope": "dimension-separated-only",
        "cross_dimension_score_allowed": False,
        "weighting": {
            "seed": "equal",
            "dimension": "report-separately",
            "section": "natural-within-selected-generated-chunks",
        },
        "members": members,
        "dimensions": dimensions,
        "candidates": [
            {"candidate": name, "production_candidate": production}
            for name, production in EXPECTED_CANDIDATES
        ],
    }
    run_plan["run_plan_sha256"] = _canonical_digest(run_plan)
    return run_plan


def _output_outside_root(output: Path, root: Path) -> None:
    resolved_root = root.resolve()
    resolved_output = output.resolve(strict=False)
    try:
        resolved_output.relative_to(resolved_root)
    except ValueError:
        return
    raise PerformanceInputError(
        "performance run plan must be written outside the immutable handoff directory"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--state-manifest", type=Path, default=DEFAULT_STATE_MANIFEST)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    args = parser.parse_args()
    try:
        _output_outside_root(args.output, args.artifact_root)
        plan = build_run_plan(
            args.artifact_root,
            state_manifest_path=args.state_manifest,
            lock_path=args.lock,
        )
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (OSError, PerformanceInputError) as error:
        print(f"section performance input error: {error}")
        return 1

    print(
        "section performance input: "
        f"population={plan['population_sha256']} "
        f"admission={plan['admission_sha256']} "
        f"run_plan={plan['run_plan_sha256']} PASS"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
