#!/usr/bin/env python3
"""Assemble dimension-separated section Pareto analysis from qualified evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

try:
    from tools import section_target_combined as combined
    from tools import section_target_hardware as population
    from tools import section_target_synthetic_evidence as synthetic
except ModuleNotFoundError:  # Direct execution from tools/.
    import section_target_combined as combined  # type: ignore[no-redef]
    import section_target_hardware as population  # type: ignore[no-redef]
    import section_target_synthetic_evidence as synthetic  # type: ignore[no-redef]

SCHEMA = 1
KIND = "section-pareto-decision-analysis"
DECISION_SCOPE = "dimension-separated-only"
FULL_TRACE_COUNT = 16
FULL_TRACE_OPERATIONS = 2_013_879
FULL_SYNTHETIC_OPERATIONS = 4_112
FULL_TRACE_FINGERPRINT = "6a4814a1551a9e5a"
CPU_MATERIAL_IMPROVEMENT_PPM = 50_000
MEMORY_MATERIAL_IMPROVEMENT_PPM = 100_000
LOWER_SHA256 = re.compile(r"[0-9a-f]{64}\Z")
FULL_SEM_IDS = (
    "SEM-WORLD-SECTION-001",
    "SEM-WORLD-SECTION-002",
    "SEM-WORLD-SECTION-005",
    "SEM-WORLD-SECTION-006",
    "SEM-WORLD-SECTION-007",
    "SEM-WORLD-SECTION-008",
    "SEM-WORLD-SECTION-009",
    "SEM-WORLD-SECTION-010",
    "SEM-WORLD-SECTION-011",
    "SEM-WORLD-SECTION-012",
    "SEM-WORLD-SECTION-013",
    "SEM-WORLD-SECTION-014",
)


class ParetoEvidenceError(RuntimeError):
    """Raised when evidence cannot safely enter section Pareto analysis."""


@dataclass(frozen=True)
class CorrectnessIdentity:
    candidate: str
    path: Path
    file_sha256: str
    commit_sha: str
    target: dict[str, object]
    evidence_id: str


@dataclass(frozen=True)
class MetricRegistry:
    population_latency: tuple[str, ...]
    population_memory: tuple[str, ...]
    synthetic_replacements: tuple[str, ...]
    synthetic_promotions: tuple[str, ...]

    @property
    def all_latency(self) -> tuple[str, ...]:
        return self.population_latency + self.synthetic_replacements + self.synthetic_promotions

    @property
    def all_memory(self) -> tuple[str, ...]:
        return self.population_memory

    @property
    def all_metrics(self) -> tuple[str, ...]:
        return self.all_latency + self.all_memory


def canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ParetoEvidenceError(f"{path} must contain a JSON object")
    return value


def require_bool(record: dict[str, Any], key: str, expected: bool, label: str) -> None:
    if record.get(key) is not expected:
        raise ParetoEvidenceError(f"{label}.{key} must be {expected}")


def require_int(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ParetoEvidenceError(f"{label} must be an integer")
    return value


def require_sha(value: object, label: str) -> str:
    if not isinstance(value, str) or LOWER_SHA256.fullmatch(value) is None:
        raise ParetoEvidenceError(f"{label} must be canonical lowercase SHA-256")
    return value


def verify_digest_field(record: dict[str, Any], field: str, label: str) -> str:
    expected = require_sha(record.get(field), f"{label}.{field}")
    payload = dict(record)
    payload.pop(field)
    actual = canonical_digest(payload)
    if actual != expected:
        raise ParetoEvidenceError(
            f"{label} digest mismatch: expected {expected}, got {actual}"
        )
    return expected


def safe_relative_path(raw: object, label: str) -> Path:
    if not isinstance(raw, str) or not raw:
        raise ParetoEvidenceError(f"{label} must be a non-empty relative path")
    path = Path(raw)
    if path.is_absolute() or ".." in path.parts or path.as_posix() != raw:
        raise ParetoEvidenceError(f"{label} is not a safe canonical relative path")
    return path


def target_contract(repo_root: Path) -> dict[str, object]:
    manifest = load_json(repo_root / "vanilla/state-data/26.2-state-data-manifest.json")
    target = manifest.get("target")
    if not isinstance(target, dict):
        raise ParetoEvidenceError("state-data manifest target is malformed")
    result = {
        "minecraft_version": target.get("minecraft_version"),
        "protocol_version": target.get("protocol_version"),
        "data_version": target.get("data_version"),
        "state_count": manifest.get("state_count"),
        "state_data_generation_sha256": manifest.get("generation_digest"),
        "state_data_input_sha256": manifest.get("input_digest"),
    }
    if result["minecraft_version"] != "26.2":
        raise ParetoEvidenceError("Pareto analysis target must be Minecraft 26.2")
    require_sha(result["state_data_generation_sha256"], "target generation digest")
    require_sha(result["state_data_input_sha256"], "target input digest")
    return result


def validate_root_artifact(root: Path) -> tuple[dict[str, Any], dict[str, dict[str, object]], str]:
    manifest_path = root / "artifact-manifest.json"
    manifest = load_json(manifest_path)
    if manifest.get("schema") != combined.ARTIFACT_SCHEMA or manifest.get("kind") != combined.ARTIFACT_KIND:
        raise ParetoEvidenceError("combined artifact schema/kind mismatch")
    manifest_sha = verify_digest_field(manifest, "manifest_sha256", "combined artifact")
    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        raise ParetoEvidenceError("combined artifact file inventory is missing")
    indexed: dict[str, dict[str, object]] = {}
    for index, raw in enumerate(files):
        if not isinstance(raw, dict):
            raise ParetoEvidenceError(f"combined artifact file {index} is malformed")
        relative = safe_relative_path(raw.get("path"), f"artifact file {index}.path")
        key = relative.as_posix()
        if key in indexed:
            raise ParetoEvidenceError(f"duplicate artifact path: {key}")
        expected_size = require_int(raw.get("size"), f"artifact {key}.size")
        expected_sha = require_sha(raw.get("sha256"), f"artifact {key}.sha256")
        path = root / relative
        if not path.is_file():
            raise ParetoEvidenceError(f"artifact file is missing: {key}")
        if path.stat().st_size != expected_size or sha256_file(path) != expected_sha:
            raise ParetoEvidenceError(f"artifact file identity mismatch: {key}")
        indexed[key] = dict(raw)
    required = {
        "combined-orchestration.json",
        "population/orchestration.json",
        "population/artifact-manifest.json",
        "population/benchmark-executable",
    }
    missing = required - set(indexed)
    if missing:
        raise ParetoEvidenceError(f"combined artifact omitted required files: {sorted(missing)}")
    return manifest, indexed, manifest_sha


def validate_combined_record(
    root: Path,
    manifest: dict[str, Any],
    indexed: dict[str, dict[str, object]],
) -> tuple[dict[str, Any], str]:
    record = load_json(root / "combined-orchestration.json")
    if record.get("schema") != combined.SCHEMA or record.get("kind") != combined.KIND:
        raise ParetoEvidenceError("combined orchestration schema/kind mismatch")
    evidence_sha = verify_digest_field(record, "evidence_sha256", "combined orchestration")
    if manifest.get("combined_evidence_sha256") != evidence_sha:
        raise ParetoEvidenceError("artifact manifest is not bound to combined orchestration")
    if record.get("mode") != "qualification":
        raise ParetoEvidenceError("Pareto analysis refuses non-qualification combined evidence")
    require_bool(record, "qualification_complete", True, "combined")
    require_bool(record, "population_evidence_eligible", True, "combined")
    require_bool(record, "synthetic_evidence_eligible", True, "combined")
    require_bool(record, "combined_measurement_evidence_eligible", True, "combined")
    if record.get("decision_evidence_eligible") is not False:
        raise ParetoEvidenceError("pre-Pareto combined evidence must not claim final decision eligibility")
    if record.get("decision_scope") != DECISION_SCOPE:
        raise ParetoEvidenceError("combined decision scope drifted")
    require_bool(record, "cross_dimension_score_allowed", False, "combined")
    rounds = require_int(record.get("rounds"), "combined.rounds")
    if rounds < 5 or rounds % 5 != 0:
        raise ParetoEvidenceError("combined qualification requires >=5 balanced rounds, multiple of five")
    if record.get("candidates") != list(population.CANDIDATES):
        raise ParetoEvidenceError("combined candidate registry drifted")
    if record.get("production_candidates") != list(population.PRODUCTION_CANDIDATES):
        raise ParetoEvidenceError("combined production-candidate registry drifted")
    if record.get("dimensions") != list(population.DIMENSIONS):
        raise ParetoEvidenceError("combined dimension registry drifted")

    identities = record.get("identities")
    if not isinstance(identities, dict):
        raise ParetoEvidenceError("combined identities are missing")
    if identities.get("representative_policy") != population.REPRESENTATIVE_POLICY:
        raise ParetoEvidenceError("combined representative policy drifted")
    for field in (
        "repository_commit_sha",
        "benchmark_executable_sha256",
        "pack_manifest_sha256",
        "population_sha256",
        "population_admission_sha256",
        "population_orchestration_evidence_sha256",
        "population_artifact_manifest_sha256",
    ):
        require_sha(identities.get(field), f"combined.identities.{field}")

    executable_entry = indexed["population/benchmark-executable"]
    if executable_entry["sha256"] != identities["benchmark_executable_sha256"]:
        raise ParetoEvidenceError("combined executable identity disagrees with artifact inventory")
    return record, evidence_sha


def validate_population_nested(root: Path, combined_record: dict[str, Any]) -> dict[str, Any]:
    record = load_json(root / "population/orchestration.json")
    if record.get("schema") != population.SCHEMA or record.get("kind") != population.KIND:
        raise ParetoEvidenceError("nested population orchestration schema/kind mismatch")
    evidence_sha = verify_digest_field(record, "evidence_sha256", "population orchestration")
    require_bool(record, "qualification_complete", True, "population")
    require_bool(record, "population_evidence_eligible", True, "population")
    if record.get("mode") != "qualification":
        raise ParetoEvidenceError("nested population orchestration is not qualification evidence")
    if record.get("decision_scope") != DECISION_SCOPE:
        raise ParetoEvidenceError("nested population decision scope drifted")
    require_bool(record, "cross_dimension_score_allowed", False, "population")
    if record.get("rounds") != combined_record.get("rounds") or record.get("cpu") != combined_record.get("cpu"):
        raise ParetoEvidenceError("population round/CPU identity disagrees with combined evidence")
    if record.get("candidates") != list(population.CANDIDATES):
        raise ParetoEvidenceError("population candidate registry drifted")
    if record.get("dimensions") != list(population.DIMENSIONS):
        raise ParetoEvidenceError("population dimension registry drifted")

    combined_ids = combined_record["identities"]
    ids = record.get("identities")
    if not isinstance(ids, dict):
        raise ParetoEvidenceError("population identities are missing")
    expected_pairs = (
        ("repository_commit_sha", "repository_commit_sha"),
        ("benchmark_executable_sha256", "benchmark_executable_sha256"),
        ("pack_manifest_sha256", "pack_manifest_sha256"),
        ("representative_policy", "representative_policy"),
        ("population_sha256", "population_sha256"),
        ("admission_sha256", "population_admission_sha256"),
    )
    for population_key, combined_key in expected_pairs:
        if ids.get(population_key) != combined_ids.get(combined_key):
            raise ParetoEvidenceError(f"population identity drift at {population_key}")
    if evidence_sha != combined_ids["population_orchestration_evidence_sha256"]:
        raise ParetoEvidenceError("combined record references the wrong population orchestration")
    return record


def validate_population_artifact(root: Path, combined_record: dict[str, Any]) -> None:
    manifest = load_json(root / "population/artifact-manifest.json")
    if manifest.get("schema") != population.ARTIFACT_SCHEMA or manifest.get("kind") != population.ARTIFACT_KIND:
        raise ParetoEvidenceError("nested population artifact schema/kind mismatch")
    manifest_sha = verify_digest_field(manifest, "manifest_sha256", "population artifact")
    if manifest_sha != combined_record["identities"]["population_artifact_manifest_sha256"]:
        raise ParetoEvidenceError("combined record references the wrong population artifact manifest")
    expected_orchestration = combined_record["identities"]["population_orchestration_evidence_sha256"]
    if manifest.get("orchestration_sha256") != expected_orchestration:
        raise ParetoEvidenceError("population artifact is not bound to population orchestration")
    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        raise ParetoEvidenceError("population artifact inventory is missing")
    seen: set[str] = set()
    population_root = root / "population"
    for index, raw in enumerate(files):
        if not isinstance(raw, dict):
            raise ParetoEvidenceError(f"population artifact file {index} malformed")
        relative = safe_relative_path(raw.get("path"), f"population artifact file {index}")
        key = relative.as_posix()
        if key in seen:
            raise ParetoEvidenceError(f"duplicate population artifact path: {key}")
        seen.add(key)
        path = population_root / relative
        expected_size = require_int(raw.get("size"), f"population artifact {key}.size")
        expected_sha = require_sha(raw.get("sha256"), f"population artifact {key}.sha256")
        if not path.is_file() or path.stat().st_size != expected_size or sha256_file(path) != expected_sha:
            raise ParetoEvidenceError(f"population artifact file identity mismatch: {key}")


def validate_correctness(
    paths: list[Path],
    *,
    expected_commit: str,
    target: dict[str, object],
) -> dict[str, CorrectnessIdentity]:
    if len(paths) != len(population.PRODUCTION_CANDIDATES):
        raise ParetoEvidenceError("exactly four full correctness files are required")
    records: dict[str, CorrectnessIdentity] = {}
    expected_target = {
        "minecraft_version": target["minecraft_version"],
        "protocol_version": target["protocol_version"],
        "data_version": target["data_version"],
        "state_count": target["state_count"],
        "state_data_generation_sha256": target["state_data_generation_sha256"],
        "state_data_input_sha256": target["state_data_input_sha256"],
    }
    for path in paths:
        raw = load_json(path)
        if raw.get("schema") != 1 or raw.get("qualification") != "section" or raw.get("mode") != "full":
            raise ParetoEvidenceError(f"{path} is not full section correctness evidence")
        if raw.get("commit_sha") != expected_commit:
            raise ParetoEvidenceError(f"{path} correctness commit differs from measurement commit")
        for key, value in expected_target.items():
            if raw.get(key) != value:
                raise ParetoEvidenceError(f"{path} correctness target drift at {key}")
        if raw.get("trace_schema") != 1 or raw.get("sem_ids") != list(FULL_SEM_IDS):
            raise ParetoEvidenceError(f"{path} correctness trace/SEM surface drifted")
        evidence = raw.get("records")
        if not isinstance(evidence, list) or len(evidence) != 1 or not isinstance(evidence[0], dict):
            raise ParetoEvidenceError(f"{path} must contain exactly one candidate record")
        record = evidence[0]
        candidate = record.get("candidate")
        if candidate not in population.PRODUCTION_CANDIDATES:
            raise ParetoEvidenceError(f"{path} has unknown production candidate {candidate!r}")
        if candidate in records:
            raise ParetoEvidenceError(f"duplicate correctness candidate: {candidate}")
        expected_id = f"EQUIV-WORLD-SECTION-FULL-{str(candidate).upper().replace('-', '_')}"
        if record.get("id") != expected_id:
            raise ParetoEvidenceError(f"{path} correctness evidence ID mismatch")
        if require_int(record.get("trace_count"), f"{candidate}.trace_count") != FULL_TRACE_COUNT:
            raise ParetoEvidenceError(f"{candidate} full trace count drifted")
        if require_int(record.get("trace_operations"), f"{candidate}.trace_operations") != FULL_TRACE_OPERATIONS:
            raise ParetoEvidenceError(f"{candidate} full trace operation count drifted")
        if require_int(record.get("synthetic_operations"), f"{candidate}.synthetic_operations") != FULL_SYNTHETIC_OPERATIONS:
            raise ParetoEvidenceError(f"{candidate} full synthetic operation count drifted")
        if record.get("trace_fingerprint_fnv1a64") != FULL_TRACE_FINGERPRINT:
            raise ParetoEvidenceError(f"{candidate} full trace fingerprint drifted")
        records[str(candidate)] = CorrectnessIdentity(
            candidate=str(candidate),
            path=path,
            file_sha256=sha256_file(path),
            commit_sha=expected_commit,
            target=expected_target,
            evidence_id=expected_id,
        )
    if set(records) != set(population.PRODUCTION_CANDIDATES):
        raise ParetoEvidenceError("correctness evidence does not cover every production candidate")
    return records


def validate_population_aggregates(record: dict[str, Any]) -> dict[str, Any]:
    aggregates = record.get("aggregates")
    if not isinstance(aggregates, dict):
        raise ParetoEvidenceError("population aggregates are missing")
    dimensions = aggregates.get("dimensions")
    if not isinstance(dimensions, dict) or set(dimensions) != set(population.DIMENSIONS):
        raise ParetoEvidenceError("population aggregate dimensions drifted")
    for dimension in population.DIMENSIONS:
        raw_dimension = dimensions[dimension]
        if not isinstance(raw_dimension, dict):
            raise ParetoEvidenceError(f"population aggregate {dimension} malformed")
        candidates = raw_dimension.get("candidates")
        if not isinstance(candidates, dict) or set(candidates) != set(population.CANDIDATES):
            raise ParetoEvidenceError(f"population aggregate candidate set drifted for {dimension}")
        for candidate, raw_candidate in candidates.items():
            if not isinstance(raw_candidate, dict):
                raise ParetoEvidenceError(f"population aggregate {dimension}/{candidate} malformed")
            workloads = raw_candidate.get("workloads_p50_ps_per_op")
            if not isinstance(workloads, dict) or set(workloads) != set(population.WORKLOADS):
                raise ParetoEvidenceError(f"population workload set drifted for {dimension}/{candidate}")
            for workload, summary in workloads.items():
                validate_aggregate_summary(summary, f"population {dimension}/{candidate}/{workload}")
            validate_aggregate_summary(raw_candidate.get("rss_loaded_delta_kib"), f"population {dimension}/{candidate}/rss")
            validate_aggregate_summary(raw_candidate.get("construction_p99_ns"), f"population {dimension}/{candidate}/construction")
    return aggregates


def validate_aggregate_summary(raw: object, label: str) -> dict[str, int]:
    if not isinstance(raw, dict):
        raise ParetoEvidenceError(f"{label} aggregate must be an object")
    result = {
        key: require_int(raw.get(key), f"{label}.{key}")
        for key in ("count", "median", "mad", "relative_mad_ppm", "min", "max")
    }
    if result["count"] <= 0 or result["min"] > result["median"] or result["median"] > result["max"]:
        raise ParetoEvidenceError(f"{label} aggregate ordering/count is invalid")
    return result


def population_child_diagnostics(
    root: Path,
    record: dict[str, Any],
    target: dict[str, object],
) -> dict[str, dict[str, dict[str, object]]]:
    children = record.get("children")
    if not isinstance(children, list):
        raise ParetoEvidenceError("population child index is missing")
    expected_rounds = require_int(record.get("rounds"), "population.rounds")
    grouped: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for index, raw in enumerate(children):
        if not isinstance(raw, dict):
            raise ParetoEvidenceError(f"population child {index} malformed")
        dimension = raw.get("dimension")
        candidate = raw.get("candidate")
        if dimension not in population.DIMENSIONS or candidate not in population.CANDIDATES:
            raise ParetoEvidenceError("population child dimension/candidate identity invalid")
        relative = safe_relative_path(raw.get("child_evidence_path"), f"population child {index} path")
        child_path = root / "population" / relative
        if sha256_file(child_path) != require_sha(raw.get("child_evidence_sha256"), f"population child {index} SHA"):
            raise ParetoEvidenceError(f"population child evidence changed: {relative}")
        child = load_json(child_path)
        if child.get("candidate") != candidate or child.get("dimension") != dimension:
            raise ParetoEvidenceError("population raw child identity disagrees with orchestration")
        if child.get("commit_sha") != record["identities"]["repository_commit_sha"]:
            raise ParetoEvidenceError("population raw child commit identity drifted")
        for key, value in target.items():
            if child.get(key) != value:
                raise ParetoEvidenceError(f"population raw child target drift at {key}")
        memory = child.get("memory")
        representations = child.get("representations")
        if not isinstance(memory, dict) or not isinstance(representations, dict):
            raise ParetoEvidenceError("population raw child memory/representation evidence missing")
        deterministic = {
            "section_count": require_int(child.get("section_count"), "population child section_count"),
            "logical_owned_bytes": require_int(memory.get("logical_owned_bytes"), "population child logical_owned_bytes"),
            "max_owned_bytes": require_int(memory.get("max_owned_bytes"), "population child max_owned_bytes"),
            "construction_transitions": require_int(memory.get("construction_transitions"), "population child construction_transitions"),
            "logical_backing_allocations": require_int(memory.get("logical_backing_allocations"), "population child logical_backing_allocations"),
            "representations": {str(key): require_int(value, f"representation {key}") for key, value in representations.items()},
        }
        grouped.setdefault((str(dimension), str(candidate)), []).append(deterministic)

    result: dict[str, dict[str, dict[str, object]]] = {dimension: {} for dimension in population.DIMENSIONS}
    for dimension in population.DIMENSIONS:
        for candidate in population.CANDIDATES:
            entries = grouped.get((dimension, candidate), [])
            if len(entries) != expected_rounds:
                raise ParetoEvidenceError(
                    f"population child count mismatch for {dimension}/{candidate}: {len(entries)}"
                )
            first = entries[0]
            if any(entry != first for entry in entries[1:]):
                raise ParetoEvidenceError(
                    f"deterministic population evidence drifted across rounds for {dimension}/{candidate}"
                )
            if sum(first["representations"].values()) != first["section_count"]:  # type: ignore[union-attr]
                raise ParetoEvidenceError(f"representation census does not recompose for {dimension}/{candidate}")
            result[dimension][candidate] = first
    return result


def normalized_synthetic_metrics(record: dict[str, Any]) -> tuple[dict[str, dict[str, int]], tuple[str, ...], tuple[str, ...]]:
    synthetic_block = record.get("synthetic")
    if not isinstance(synthetic_block, dict):
        raise ParetoEvidenceError("combined synthetic block is missing")
    aggregates = synthetic_block.get("aggregates")
    if not isinstance(aggregates, dict):
        raise ParetoEvidenceError("synthetic aggregates are missing")
    candidates = aggregates.get("candidates")
    if not isinstance(candidates, dict) or set(candidates) != set(synthetic.CANDIDATES):
        raise ParetoEvidenceError("synthetic aggregate candidate set drifted")

    normalized: dict[str, dict[str, int]] = {}
    replacement_registry: tuple[str, ...] | None = None
    promotion_registry: tuple[str, ...] | None = None
    for candidate in synthetic.CANDIDATES:
        raw = candidates[candidate]
        if not isinstance(raw, dict):
            raise ParetoEvidenceError(f"synthetic candidate aggregate malformed: {candidate}")
        replacements = raw.get("replacement_p50_ps_per_op")
        promotions = raw.get("promotion_p99_ns")
        if not isinstance(replacements, dict) or not isinstance(promotions, dict):
            raise ParetoEvidenceError(f"synthetic candidate metric maps missing: {candidate}")
        replacement_values: dict[str, int] = {}
        representation_diagnostics: dict[str, str] = {}
        for key, summary in replacements.items():
            if not isinstance(key, str):
                raise ParetoEvidenceError("synthetic replacement key must be a string")
            parts = key.split("|")
            if len(parts) != 5:
                raise ParetoEvidenceError(f"synthetic replacement key has wrong shape: {key}")
            workload, pattern, pool, actual, representation = parts
            normalized_key = f"replace:{workload}|{pattern}|{pool}|{actual}"
            if normalized_key in replacement_values:
                raise ParetoEvidenceError(f"duplicate normalized replacement key: {normalized_key}")
            replacement_values[normalized_key] = synthetic_summary_median(summary, f"{candidate}/{key}")
            representation_diagnostics[normalized_key] = representation
        promotion_values: dict[str, int] = {}
        for key, summary in promotions.items():
            if not isinstance(key, str):
                raise ParetoEvidenceError("synthetic promotion key must be a string")
            parts = key.split("|", maxsplit=1)
            if len(parts) != 2:
                raise ParetoEvidenceError(f"synthetic promotion key has wrong shape: {key}")
            workload, _representation = parts
            normalized_key = f"promotion:{workload}"
            if normalized_key in promotion_values:
                raise ParetoEvidenceError(f"duplicate normalized promotion key: {normalized_key}")
            promotion_values[normalized_key] = synthetic_summary_median(summary, f"{candidate}/{key}")
        current_replacements = tuple(sorted(replacement_values))
        current_promotions = tuple(sorted(promotion_values))
        if replacement_registry is None:
            replacement_registry = current_replacements
            promotion_registry = current_promotions
        elif current_replacements != replacement_registry or current_promotions != promotion_registry:
            raise ParetoEvidenceError("normalized synthetic metric surface drifted between candidates")
        normalized[candidate] = {**replacement_values, **promotion_values}
        normalized[candidate]["__representation_count"] = len(representation_diagnostics)

    assert replacement_registry is not None and promotion_registry is not None
    expected_promotions = tuple(sorted(f"promotion:promotion-to-{target}" for target in synthetic.PROMOTION_TARGETS))
    if promotion_registry != expected_promotions:
        raise ParetoEvidenceError("synthetic promotion boundary registry drifted")
    return normalized, replacement_registry, promotion_registry


def synthetic_summary_median(raw: object, label: str) -> int:
    if not isinstance(raw, dict):
        raise ParetoEvidenceError(f"synthetic aggregate {label} malformed")
    value = require_int(raw.get("median"), f"synthetic {label}.median")
    if value < 0:
        raise ParetoEvidenceError(f"synthetic aggregate {label} has negative median")
    return value


def build_metric_registry(
    replacement_keys: tuple[str, ...], promotion_keys: tuple[str, ...]
) -> MetricRegistry:
    population_latency = tuple(f"population:{workload}" for workload in population.STEADY_WORKLOADS) + (
        "population:construction-p99",
    )
    population_memory = (
        "memory:rss-loaded-delta-kib",
        "memory:logical-owned-bytes",
        "memory:max-owned-bytes",
    )
    return MetricRegistry(
        population_latency=population_latency,
        population_memory=population_memory,
        synthetic_replacements=replacement_keys,
        synthetic_promotions=promotion_keys,
    )


def build_vectors(
    population_aggregates: dict[str, Any],
    deterministic: dict[str, dict[str, dict[str, object]]],
    synthetic_metrics: dict[str, dict[str, int]],
    registry: MetricRegistry,
) -> dict[str, dict[str, dict[str, int]]]:
    result: dict[str, dict[str, dict[str, int]]] = {}
    aggregate_dimensions = population_aggregates["dimensions"]
    for dimension in population.DIMENSIONS:
        by_candidate: dict[str, dict[str, int]] = {}
        aggregate_candidates = aggregate_dimensions[dimension]["candidates"]
        for candidate in population.CANDIDATES:
            source = aggregate_candidates[candidate]
            vector: dict[str, int] = {}
            for workload in population.STEADY_WORKLOADS:
                vector[f"population:{workload}"] = require_int(
                    source["workloads_p50_ps_per_op"][workload]["median"],
                    f"{dimension}/{candidate}/{workload} median",
                )
            vector["population:construction-p99"] = require_int(
                source["construction_p99_ns"]["median"],
                f"{dimension}/{candidate}/construction median",
            )
            vector["memory:rss-loaded-delta-kib"] = require_int(
                source["rss_loaded_delta_kib"]["median"],
                f"{dimension}/{candidate}/rss median",
            )
            deterministic_record = deterministic[dimension][candidate]
            vector["memory:logical-owned-bytes"] = int(deterministic_record["logical_owned_bytes"])
            vector["memory:max-owned-bytes"] = int(deterministic_record["max_owned_bytes"])
            for key in registry.synthetic_replacements + registry.synthetic_promotions:
                vector[key] = synthetic_metrics[candidate][key]
            if set(vector) != set(registry.all_metrics):
                raise ParetoEvidenceError(f"metric vector shape drifted for {dimension}/{candidate}")
            if any(value < 0 for value in vector.values()):
                raise ParetoEvidenceError(f"negative lower-is-better metric for {dimension}/{candidate}")
            by_candidate[candidate] = vector
        result[dimension] = by_candidate
    return result


def strictly_dominates(left: dict[str, int], right: dict[str, int]) -> bool:
    if set(left) != set(right):
        raise ParetoEvidenceError("cannot compare Pareto vectors with different metric registries")
    weak = all(left[key] <= right[key] for key in left)
    strict = any(left[key] < right[key] for key in left)
    return weak and strict


def dimension_frontier(vectors: dict[str, dict[str, int]]) -> tuple[list[str], dict[str, list[str]]]:
    production = tuple(population.PRODUCTION_CANDIDATES)
    dominators: dict[str, list[str]] = {}
    frontier: list[str] = []
    for candidate in production:
        candidate_dominators = [
            other
            for other in production
            if other != candidate and strictly_dominates(vectors[other], vectors[candidate])
        ]
        dominators[candidate] = sorted(candidate_dominators)
        if not candidate_dominators:
            frontier.append(candidate)
    return sorted(frontier), dominators


def global_dominators(
    vectors: dict[str, dict[str, dict[str, int]]]
) -> dict[str, list[str]]:
    result: dict[str, list[str]] = {}
    for candidate in population.PRODUCTION_CANDIDATES:
        valid: list[str] = []
        for other in population.PRODUCTION_CANDIDATES:
            if other == candidate:
                continue
            if all(
                strictly_dominates(vectors[dimension][other], vectors[dimension][candidate])
                for dimension in population.DIMENSIONS
            ):
                valid.append(other)
        result[candidate] = sorted(valid)
    return result


def improvement_ppm(baseline: int, candidate: int) -> int:
    if baseline <= 0:
        return 0
    return (baseline - candidate) * 1_000_000 // baseline


def material_benefit(
    vectors: dict[str, dict[str, dict[str, int]]], registry: MetricRegistry
) -> dict[str, object]:
    result: dict[str, object] = {}
    for candidate in population.PRODUCTION_CANDIDATES:
        if candidate == "direct":
            result[candidate] = {
                "baseline": True,
                "material": True,
                "best_latency_improvement_ppm": 0,
                "best_memory_improvement_ppm": 0,
                "qualifying_metrics": [],
            }
            continue
        best_latency = 0
        best_memory = 0
        qualifying: list[dict[str, object]] = []
        for dimension in population.DIMENSIONS:
            baseline = vectors[dimension]["direct"]
            candidate_vector = vectors[dimension][candidate]
            for metric in registry.all_latency:
                gain = improvement_ppm(baseline[metric], candidate_vector[metric])
                best_latency = max(best_latency, gain)
                if gain >= CPU_MATERIAL_IMPROVEMENT_PPM:
                    qualifying.append({"dimension": dimension, "metric": metric, "improvement_ppm": gain})
            for metric in registry.all_memory:
                gain = improvement_ppm(baseline[metric], candidate_vector[metric])
                best_memory = max(best_memory, gain)
                if gain >= MEMORY_MATERIAL_IMPROVEMENT_PPM:
                    qualifying.append({"dimension": dimension, "metric": metric, "improvement_ppm": gain})
        result[candidate] = {
            "baseline": False,
            "material": bool(qualifying),
            "best_latency_improvement_ppm": best_latency,
            "best_memory_improvement_ppm": best_memory,
            "qualifying_metrics": sorted(
                qualifying,
                key=lambda item: (str(item["dimension"]), str(item["metric"])),
            ),
        }
    return result


def analyze(
    *,
    repo_root: Path,
    combined_artifact: Path,
    correctness_paths: list[Path],
) -> dict[str, object]:
    repo_root = repo_root.resolve()
    root = combined_artifact.resolve()
    if not root.is_dir():
        raise ParetoEvidenceError(f"combined artifact is not a directory: {root}")
    target = target_contract(repo_root)
    artifact, indexed, artifact_sha = validate_root_artifact(root)
    combined_record, combined_sha = validate_combined_record(root, artifact, indexed)
    population_record = validate_population_nested(root, combined_record)
    validate_population_artifact(root, combined_record)
    correctness = validate_correctness(
        correctness_paths,
        expected_commit=str(combined_record["identities"]["repository_commit_sha"]),
        target=target,
    )
    population_aggregates = validate_population_aggregates(population_record)
    deterministic = population_child_diagnostics(root, population_record, target)
    synthetic_metrics, replacement_keys, promotion_keys = normalized_synthetic_metrics(combined_record)
    registry = build_metric_registry(replacement_keys, promotion_keys)
    vectors = build_vectors(
        population_aggregates, deterministic, synthetic_metrics, registry
    )

    dimension_results: dict[str, object] = {}
    for dimension in population.DIMENSIONS:
        frontier, dominators = dimension_frontier(vectors[dimension])
        dimension_results[dimension] = {
            "production_pareto_frontier": frontier,
            "dominators": dominators,
            "metrics": vectors[dimension],
            "diagnostics": deterministic[dimension],
        }
    all_dimension_dominators = global_dominators(vectors)
    strictly_dominated_candidates = sorted(
        candidate for candidate, dominators in all_dimension_dominators.items() if dominators
    )
    survivors = sorted(
        candidate
        for candidate in population.PRODUCTION_CANDIDATES
        if candidate not in strictly_dominated_candidates
    )
    common_frontier = sorted(
        set(population.PRODUCTION_CANDIDATES).intersection(
            *(set(dimension_results[dimension]["production_pareto_frontier"]) for dimension in population.DIMENSIONS)  # type: ignore[index]
        )
    )
    material = material_benefit(vectors, registry)
    blockers: list[str] = ["explicit production-policy selection record not yet committed"]
    if not common_frontier:
        blockers.append("no single production candidate lies on every standard-dimension frontier")
    unjustified = [
        candidate
        for candidate in survivors
        if candidate != "direct" and not bool(material[candidate]["material"])  # type: ignore[index]
    ]
    if unjustified:
        blockers.append(
            "Pareto survivors without material complexity justification: " + ", ".join(sorted(unjustified))
        )

    correctness_files = {
        candidate: {
            "path": identity.path.name,
            "sha256": identity.file_sha256,
            "evidence_id": identity.evidence_id,
        }
        for candidate, identity in sorted(correctness.items())
    }
    analysis: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "analysis_complete": True,
        "selection_ready": bool(common_frontier),
        "decision_evidence_eligible": False,
        "selection_blockers": blockers,
        "decision_scope": DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
        "identities": {
            "repository_commit_sha": combined_record["identities"]["repository_commit_sha"],
            "benchmark_executable_sha256": combined_record["identities"]["benchmark_executable_sha256"],
            "representative_policy": combined_record["identities"]["representative_policy"],
            "population_sha256": combined_record["identities"]["population_sha256"],
            "population_admission_sha256": combined_record["identities"]["population_admission_sha256"],
            "combined_evidence_sha256": combined_sha,
            "combined_artifact_manifest_sha256": artifact_sha,
            "correctness": correctness_files,
            "target": target,
        },
        "hardware": {
            "cpu": combined_record["cpu"],
            "cpu_topology": combined_record["cpu_topology"],
            "rounds": combined_record["rounds"],
        },
        "metric_registry": {
            "lower_is_better": list(registry.all_metrics),
            "population_latency": list(registry.population_latency),
            "population_memory": list(registry.population_memory),
            "synthetic_replacements": list(registry.synthetic_replacements),
            "synthetic_promotions": list(registry.synthetic_promotions),
            "diagnostic_not_dominance_axes": [
                "construction_transitions",
                "logical_backing_allocations",
                "representations",
            ],
        },
        "materiality_thresholds_ppm": {
            "cpu_latency_tail": CPU_MATERIAL_IMPROVEMENT_PPM,
            "rss_logical_memory": MEMORY_MATERIAL_IMPROVEMENT_PPM,
        },
        "dimensions": dimension_results,
        "global": {
            "strictly_dominated_candidates": strictly_dominated_candidates,
            "global_dominators": all_dimension_dominators,
            "pareto_survivors": survivors,
            "common_all_dimension_frontier": common_frontier,
            "material_benefit_vs_direct": material,
        },
        "interpretation": {
            "direct_reference_selectable": False,
            "cross_dimension_weighting_used": False,
            "mathematical_dominance_is_not_complexity_justification": True,
            "winner_selected": False,
        },
    }
    analysis["analysis_sha256"] = canonical_digest(analysis)
    return analysis


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--repo-root", type=Path, default=Path("."))
    result.add_argument("--combined-artifact", type=Path, required=True)
    result.add_argument("--correctness", type=Path, action="append", required=True)
    result.add_argument("--output", type=Path, required=True)
    return result


def main() -> int:
    args = parser().parse_args()
    try:
        record = analyze(
            repo_root=args.repo_root,
            combined_artifact=args.combined_artifact,
            correctness_paths=args.correctness,
        )
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (ParetoEvidenceError, OSError, json.JSONDecodeError) as error:
        print(f"section Pareto analysis error: {error}")
        return 1
    print(
        "section Pareto analysis: "
        f"survivors={record['global']['pareto_survivors']} "  # type: ignore[index]
        f"common_frontier={record['global']['common_all_dimension_frontier']} "  # type: ignore[index]
        f"analysis={record['analysis_sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
