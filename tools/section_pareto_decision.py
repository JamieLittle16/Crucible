#!/usr/bin/env python3
"""Assemble dimension-separated section Pareto analysis from qualified evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any

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
LOWER_GIT_SHA1 = re.compile(r"[0-9a-f]{40}\Z")
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
    evidence_id: str


@dataclass(frozen=True)
class MetricRegistry:
    population_latency: tuple[str, ...]
    population_memory: tuple[str, ...]
    synthetic_replacements: tuple[str, ...]
    synthetic_promotions: tuple[str, ...]

    @property
    def latency(self) -> tuple[str, ...]:
        return self.population_latency + self.synthetic_replacements + self.synthetic_promotions

    @property
    def memory(self) -> tuple[str, ...]:
        return self.population_memory

    @property
    def all(self) -> tuple[str, ...]:
        return self.latency + self.memory


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


def integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ParetoEvidenceError(f"{label} must be an integer")
    return value


def sha256(value: object, label: str) -> str:
    if not isinstance(value, str) or LOWER_SHA256.fullmatch(value) is None:
        raise ParetoEvidenceError(f"{label} must be canonical lowercase SHA-256")
    return value


def git_sha(value: object, label: str) -> str:
    if not isinstance(value, str) or LOWER_GIT_SHA1.fullmatch(value) is None:
        raise ParetoEvidenceError(f"{label} must be a canonical 40-hex Git commit SHA")
    return value


def require_flag(record: dict[str, Any], key: str, expected: bool, label: str) -> None:
    if record.get(key) is not expected:
        raise ParetoEvidenceError(f"{label}.{key} must be {expected}")


def verify_digest(record: dict[str, Any], field: str, label: str) -> str:
    expected = sha256(record.get(field), f"{label}.{field}")
    payload = dict(record)
    payload.pop(field)
    actual = canonical_digest(payload)
    if actual != expected:
        raise ParetoEvidenceError(
            f"{label} digest mismatch: expected {expected}, got {actual}"
        )
    return expected


def safe_relative(raw: object, label: str) -> Path:
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
    sha256(result["state_data_generation_sha256"], "target generation digest")
    sha256(result["state_data_input_sha256"], "target input digest")
    return result


def validate_root_artifact(
    root: Path,
) -> tuple[dict[str, Any], dict[str, dict[str, object]], str]:
    manifest = load_json(root / "artifact-manifest.json")
    if (
        manifest.get("schema") != combined.ARTIFACT_SCHEMA
        or manifest.get("kind") != combined.ARTIFACT_KIND
    ):
        raise ParetoEvidenceError("combined artifact schema/kind mismatch")
    manifest_sha = verify_digest(manifest, "manifest_sha256", "combined artifact")
    raw_files = manifest.get("files")
    if not isinstance(raw_files, list) or not raw_files:
        raise ParetoEvidenceError("combined artifact file inventory is missing")
    files: dict[str, dict[str, object]] = {}
    for index, raw in enumerate(raw_files):
        if not isinstance(raw, dict):
            raise ParetoEvidenceError(f"combined artifact file {index} is malformed")
        relative = safe_relative(raw.get("path"), f"artifact file {index}.path")
        key = relative.as_posix()
        if key in files:
            raise ParetoEvidenceError(f"duplicate artifact path: {key}")
        expected_size = integer(raw.get("size"), f"artifact {key}.size")
        expected_sha = sha256(raw.get("sha256"), f"artifact {key}.sha256")
        path = root / relative
        if not path.is_file():
            raise ParetoEvidenceError(f"artifact file is missing: {key}")
        if path.stat().st_size != expected_size or sha256_file(path) != expected_sha:
            raise ParetoEvidenceError(f"artifact file identity mismatch: {key}")
        files[key] = dict(raw)
    required = {
        "combined-orchestration.json",
        "population/orchestration.json",
        "population/artifact-manifest.json",
        "population/benchmark-executable",
    }
    missing = required - set(files)
    if missing:
        raise ParetoEvidenceError(f"combined artifact omitted required files: {sorted(missing)}")
    return manifest, files, manifest_sha


def validate_combined(
    root: Path,
    manifest: dict[str, Any],
    files: dict[str, dict[str, object]],
) -> tuple[dict[str, Any], str]:
    record = load_json(root / "combined-orchestration.json")
    if record.get("schema") != combined.SCHEMA or record.get("kind") != combined.KIND:
        raise ParetoEvidenceError("combined orchestration schema/kind mismatch")
    evidence_sha = verify_digest(record, "evidence_sha256", "combined orchestration")
    if manifest.get("combined_evidence_sha256") != evidence_sha:
        raise ParetoEvidenceError("artifact manifest is not bound to combined orchestration")
    if record.get("mode") != "qualification":
        raise ParetoEvidenceError("Pareto analysis refuses non-qualification combined evidence")
    for key in (
        "qualification_complete",
        "population_evidence_eligible",
        "synthetic_evidence_eligible",
        "combined_measurement_evidence_eligible",
    ):
        require_flag(record, key, True, "combined")
    require_flag(record, "decision_evidence_eligible", False, "combined")
    require_flag(record, "cross_dimension_score_allowed", False, "combined")
    if record.get("decision_scope") != DECISION_SCOPE:
        raise ParetoEvidenceError("combined decision scope drifted")
    rounds = integer(record.get("rounds"), "combined.rounds")
    if rounds < 5 or rounds % 5 != 0:
        raise ParetoEvidenceError("combined qualification requires >=5 balanced rounds, multiple of five")
    if record.get("candidates") != list(population.CANDIDATES):
        raise ParetoEvidenceError("combined candidate registry drifted")
    if record.get("production_candidates") != list(population.PRODUCTION_CANDIDATES):
        raise ParetoEvidenceError("combined production candidate registry drifted")
    if record.get("dimensions") != list(population.DIMENSIONS):
        raise ParetoEvidenceError("combined dimension registry drifted")

    ids = record.get("identities")
    if not isinstance(ids, dict):
        raise ParetoEvidenceError("combined identities are missing")
    git_sha(ids.get("repository_commit_sha"), "combined repository commit")
    if ids.get("representative_policy") != population.REPRESENTATIVE_POLICY:
        raise ParetoEvidenceError("combined representative policy drifted")
    for field in (
        "benchmark_executable_sha256",
        "pack_manifest_sha256",
        "population_sha256",
        "population_admission_sha256",
        "population_orchestration_evidence_sha256",
        "population_artifact_manifest_sha256",
    ):
        sha256(ids.get(field), f"combined.identities.{field}")
    if files["population/benchmark-executable"]["sha256"] != ids["benchmark_executable_sha256"]:
        raise ParetoEvidenceError("combined executable identity disagrees with artifact inventory")
    return record, evidence_sha


def validate_population_nested(root: Path, combined_record: dict[str, Any]) -> dict[str, Any]:
    record = load_json(root / "population/orchestration.json")
    if record.get("schema") != population.SCHEMA or record.get("kind") != population.KIND:
        raise ParetoEvidenceError("nested population orchestration schema/kind mismatch")
    evidence_sha = verify_digest(record, "evidence_sha256", "population orchestration")
    require_flag(record, "qualification_complete", True, "population")
    require_flag(record, "population_evidence_eligible", True, "population")
    require_flag(record, "decision_evidence_eligible", False, "population")
    require_flag(record, "cross_dimension_score_allowed", False, "population")
    if record.get("mode") != "qualification" or record.get("decision_scope") != DECISION_SCOPE:
        raise ParetoEvidenceError("nested population mode/decision scope drifted")
    if record.get("rounds") != combined_record.get("rounds") or record.get("cpu") != combined_record.get("cpu"):
        raise ParetoEvidenceError("population round/CPU identity disagrees with combined evidence")
    if record.get("candidates") != list(population.CANDIDATES):
        raise ParetoEvidenceError("population candidate registry drifted")
    if record.get("production_candidates") != list(population.PRODUCTION_CANDIDATES):
        raise ParetoEvidenceError("population production candidate registry drifted")
    if record.get("dimensions") != list(population.DIMENSIONS):
        raise ParetoEvidenceError("population dimension registry drifted")

    ids = record.get("identities")
    combined_ids = combined_record["identities"]
    if not isinstance(ids, dict):
        raise ParetoEvidenceError("population identities are missing")
    pairs = (
        ("repository_commit_sha", "repository_commit_sha"),
        ("benchmark_executable_sha256", "benchmark_executable_sha256"),
        ("pack_manifest_sha256", "pack_manifest_sha256"),
        ("representative_policy", "representative_policy"),
        ("population_sha256", "population_sha256"),
        ("admission_sha256", "population_admission_sha256"),
    )
    for population_key, combined_key in pairs:
        if ids.get(population_key) != combined_ids.get(combined_key):
            raise ParetoEvidenceError(f"population identity drift at {population_key}")
    if evidence_sha != combined_ids["population_orchestration_evidence_sha256"]:
        raise ParetoEvidenceError("combined record references the wrong population orchestration")

    raw_children = record.get("children")
    if not isinstance(raw_children, list):
        raise ParetoEvidenceError("population children are missing")
    recomputed = population.aggregate_children(raw_children)
    if record.get("aggregates") != recomputed:
        raise ParetoEvidenceError("population aggregate table does not recompute from child evidence")
    recomputed_noise = population.classify_noise(
        recomputed, smoke=False, rounds=integer(record.get("rounds"), "population rounds")
    )
    if record.get("noise_qualification") != recomputed_noise:
        raise ParetoEvidenceError("population noise qualification does not recompute")
    if recomputed_noise.get("population_evidence_eligible") is not True:
        raise ParetoEvidenceError("population evidence is not noise/protocol eligible")
    return record


def validate_population_manifest(root: Path, combined_record: dict[str, Any]) -> None:
    manifest = load_json(root / "population/artifact-manifest.json")
    if (
        manifest.get("schema") != population.ARTIFACT_SCHEMA
        or manifest.get("kind") != population.ARTIFACT_KIND
    ):
        raise ParetoEvidenceError("nested population artifact schema/kind mismatch")
    manifest_sha = verify_digest(manifest, "manifest_sha256", "population artifact")
    if manifest_sha != combined_record["identities"]["population_artifact_manifest_sha256"]:
        raise ParetoEvidenceError("combined record references the wrong population artifact manifest")
    if (
        manifest.get("orchestration_sha256")
        != combined_record["identities"]["population_orchestration_evidence_sha256"]
    ):
        raise ParetoEvidenceError("population artifact is not bound to population orchestration")
    raw_files = manifest.get("files")
    if not isinstance(raw_files, list) or not raw_files:
        raise ParetoEvidenceError("population artifact inventory is missing")
    seen: set[str] = set()
    population_root = root / "population"
    for index, raw in enumerate(raw_files):
        if not isinstance(raw, dict):
            raise ParetoEvidenceError(f"population artifact file {index} malformed")
        relative = safe_relative(raw.get("path"), f"population artifact file {index}")
        key = relative.as_posix()
        if key in seen:
            raise ParetoEvidenceError(f"duplicate population artifact path: {key}")
        seen.add(key)
        path = population_root / relative
        expected_size = integer(raw.get("size"), f"population artifact {key}.size")
        expected_sha = sha256(raw.get("sha256"), f"population artifact {key}.sha256")
        if not path.is_file() or path.stat().st_size != expected_size or sha256_file(path) != expected_sha:
            raise ParetoEvidenceError(f"population artifact file identity mismatch: {key}")


def validate_synthetic(combined_record: dict[str, Any]) -> dict[str, Any]:
    block = combined_record.get("synthetic")
    if not isinstance(block, dict):
        raise ParetoEvidenceError("combined synthetic evidence block is missing")
    children = block.get("children")
    if not isinstance(children, list):
        raise ParetoEvidenceError("combined synthetic child summaries are missing")
    recomputed = synthetic.aggregate_children(children)
    if block.get("aggregates") != recomputed:
        raise ParetoEvidenceError("synthetic aggregate table does not recompute from child evidence")
    noise = synthetic.classify_noise(
        recomputed,
        smoke=False,
        rounds=integer(combined_record.get("rounds"), "combined rounds"),
    )
    if block.get("noise_qualification") != noise:
        raise ParetoEvidenceError("synthetic noise qualification does not recompute")
    if noise.get("synthetic_evidence_eligible") is not True:
        raise ParetoEvidenceError("synthetic evidence is not noise/protocol eligible")
    return recomputed


def validate_correctness(
    paths: list[Path], *, expected_commit: str, target: dict[str, object]
) -> dict[str, CorrectnessIdentity]:
    if len(paths) != len(population.PRODUCTION_CANDIDATES):
        raise ParetoEvidenceError("exactly four full correctness files are required")
    records: dict[str, CorrectnessIdentity] = {}
    for path in paths:
        raw = load_json(path)
        if raw.get("schema") != 1 or raw.get("qualification") != "section" or raw.get("mode") != "full":
            raise ParetoEvidenceError(f"{path} is not full section correctness evidence")
        if raw.get("commit_sha") != expected_commit:
            raise ParetoEvidenceError(f"{path} correctness commit differs from measurement commit")
        git_sha(raw.get("commit_sha"), f"{path} correctness commit")
        for key, value in target.items():
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
        candidate = str(candidate)
        if candidate in records:
            raise ParetoEvidenceError(f"duplicate correctness candidate: {candidate}")
        expected_id = f"EQUIV-WORLD-SECTION-FULL-{candidate.upper().replace('-', '_')}"
        checks = (
            (record.get("trace_count"), FULL_TRACE_COUNT, "trace count"),
            (record.get("trace_operations"), FULL_TRACE_OPERATIONS, "trace operations"),
            (record.get("synthetic_operations"), FULL_SYNTHETIC_OPERATIONS, "synthetic operations"),
        )
        if record.get("id") != expected_id:
            raise ParetoEvidenceError(f"{candidate} correctness evidence ID drifted")
        for observed, expected, label in checks:
            if integer(observed, f"{candidate} {label}") != expected:
                raise ParetoEvidenceError(f"{candidate} full {label} drifted")
        if record.get("trace_fingerprint_fnv1a64") != FULL_TRACE_FINGERPRINT:
            raise ParetoEvidenceError(f"{candidate} full trace fingerprint drifted")
        records[candidate] = CorrectnessIdentity(
            candidate=candidate,
            path=path,
            file_sha256=sha256_file(path),
            evidence_id=expected_id,
        )
    if set(records) != set(population.PRODUCTION_CANDIDATES):
        raise ParetoEvidenceError("correctness evidence does not cover every production candidate")
    return records


def population_deterministic_diagnostics(
    root: Path,
    record: dict[str, Any],
    target: dict[str, object],
) -> dict[str, dict[str, dict[str, object]]]:
    children = record["children"]
    rounds = integer(record.get("rounds"), "population rounds")
    grouped: dict[tuple[str, str], list[dict[str, object]]] = {}
    for index, raw in enumerate(children):
        if not isinstance(raw, dict):
            raise ParetoEvidenceError(f"population child {index} malformed")
        dimension = raw.get("dimension")
        candidate = raw.get("candidate")
        if dimension not in population.DIMENSIONS or candidate not in population.CANDIDATES:
            raise ParetoEvidenceError(f"population child {index} has invalid dimension/candidate")
        relative = safe_relative(raw.get("child_evidence_path"), f"population child {index} path")
        path = root / "population" / relative
        expected_sha = sha256(raw.get("child_evidence_sha256"), f"population child {index} SHA")
        if not path.is_file() or sha256_file(path) != expected_sha:
            raise ParetoEvidenceError(f"population child evidence changed: {relative}")
        child = load_json(path)
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
        deterministic: dict[str, object] = {
            "section_count": integer(child.get("section_count"), "population child section_count"),
            "logical_owned_bytes": integer(memory.get("logical_owned_bytes"), "logical_owned_bytes"),
            "max_owned_bytes": integer(memory.get("max_owned_bytes"), "max_owned_bytes"),
            "construction_transitions": integer(memory.get("construction_transitions"), "construction_transitions"),
            "logical_backing_allocations": integer(
                memory.get("logical_backing_allocations"), "logical_backing_allocations"
            ),
            "representations": {
                str(name): integer(count, f"representation {name}")
                for name, count in representations.items()
            },
        }
        grouped.setdefault((str(dimension), str(candidate)), []).append(deterministic)

    result: dict[str, dict[str, dict[str, object]]] = {
        dimension: {} for dimension in population.DIMENSIONS
    }
    for dimension in population.DIMENSIONS:
        for candidate in population.CANDIDATES:
            entries = grouped.get((dimension, candidate), [])
            if len(entries) != rounds:
                raise ParetoEvidenceError(
                    f"population child count mismatch for {dimension}/{candidate}: {len(entries)}"
                )
            first = entries[0]
            if any(entry != first for entry in entries[1:]):
                raise ParetoEvidenceError(
                    f"deterministic population evidence drifted across rounds for {dimension}/{candidate}"
                )
            representations = first["representations"]
            if not isinstance(representations, dict):
                raise ParetoEvidenceError("internal representation census shape failure")
            if sum(integer(value, "representation count") for value in representations.values()) != first["section_count"]:
                raise ParetoEvidenceError(
                    f"representation census does not recompose for {dimension}/{candidate}"
                )
            result[dimension][candidate] = first
    return result


def synthetic_medians(
    aggregates: dict[str, Any],
) -> tuple[dict[str, dict[str, int]], tuple[str, ...], tuple[str, ...]]:
    raw_candidates = aggregates.get("candidates")
    if not isinstance(raw_candidates, dict) or set(raw_candidates) != set(synthetic.CANDIDATES):
        raise ParetoEvidenceError("synthetic aggregate candidate set drifted")
    result: dict[str, dict[str, int]] = {}
    replacement_registry: tuple[str, ...] | None = None
    promotion_registry: tuple[str, ...] | None = None
    for candidate in synthetic.CANDIDATES:
        raw = raw_candidates[candidate]
        if not isinstance(raw, dict):
            raise ParetoEvidenceError(f"synthetic candidate aggregate malformed: {candidate}")
        replacements = raw.get("replacement_p50_ps_per_op")
        promotions = raw.get("promotion_p99_ns")
        if not isinstance(replacements, dict) or not isinstance(promotions, dict):
            raise ParetoEvidenceError(f"synthetic metric maps missing: {candidate}")
        values: dict[str, int] = {}
        replacement_keys: list[str] = []
        promotion_keys: list[str] = []
        for key, summary in replacements.items():
            if not isinstance(key, str):
                raise ParetoEvidenceError("synthetic replacement key must be a string")
            parts = key.split("|")
            if len(parts) != 5:
                raise ParetoEvidenceError(f"synthetic replacement key has wrong shape: {key}")
            workload, pattern, pool, actual, _representation = parts
            normalized = f"replace:{workload}|{pattern}|{pool}|{actual}"
            if normalized in values:
                raise ParetoEvidenceError(f"duplicate normalized replacement key: {normalized}")
            values[normalized] = synthetic_median(summary, f"{candidate}/{key}")
            replacement_keys.append(normalized)
        for key, summary in promotions.items():
            if not isinstance(key, str):
                raise ParetoEvidenceError("synthetic promotion key must be a string")
            parts = key.split("|", maxsplit=1)
            if len(parts) != 2:
                raise ParetoEvidenceError(f"synthetic promotion key has wrong shape: {key}")
            normalized = f"promotion:{parts[0]}"
            if normalized in values:
                raise ParetoEvidenceError(f"duplicate normalized promotion key: {normalized}")
            values[normalized] = synthetic_median(summary, f"{candidate}/{key}")
            promotion_keys.append(normalized)
        current_replacements = tuple(sorted(replacement_keys))
        current_promotions = tuple(sorted(promotion_keys))
        if replacement_registry is None:
            replacement_registry = current_replacements
            promotion_registry = current_promotions
        elif (
            current_replacements != replacement_registry
            or current_promotions != promotion_registry
        ):
            raise ParetoEvidenceError("normalized synthetic metric surface drifted between candidates")
        result[candidate] = values
    assert replacement_registry is not None and promotion_registry is not None
    expected_promotions = tuple(
        sorted(f"promotion:promotion-to-{target}" for target in synthetic.PROMOTION_TARGETS)
    )
    if promotion_registry != expected_promotions:
        raise ParetoEvidenceError("synthetic promotion boundary registry drifted")
    return result, replacement_registry, promotion_registry


def synthetic_median(raw: object, label: str) -> int:
    if not isinstance(raw, dict):
        raise ParetoEvidenceError(f"synthetic aggregate {label} malformed")
    value = integer(raw.get("median"), f"synthetic {label}.median")
    if value < 0:
        raise ParetoEvidenceError(f"synthetic aggregate {label} has negative median")
    return value


def metric_registry(
    replacement_keys: tuple[str, ...], promotion_keys: tuple[str, ...]
) -> MetricRegistry:
    return MetricRegistry(
        population_latency=tuple(
            f"population:{workload}" for workload in population.STEADY_WORKLOADS
        )
        + ("population:construction-p99",),
        population_memory=(
            "memory:rss-loaded-delta-kib",
            "memory:logical-owned-bytes",
            "memory:max-owned-bytes",
        ),
        synthetic_replacements=replacement_keys,
        synthetic_promotions=promotion_keys,
    )


def build_vectors(
    population_record: dict[str, Any],
    deterministic: dict[str, dict[str, dict[str, object]]],
    synthetic_values: dict[str, dict[str, int]],
    registry: MetricRegistry,
) -> dict[str, dict[str, dict[str, int]]]:
    dimensions = population_record["aggregates"]["dimensions"]
    result: dict[str, dict[str, dict[str, int]]] = {}
    for dimension in population.DIMENSIONS:
        result[dimension] = {}
        for candidate in population.CANDIDATES:
            source = dimensions[dimension]["candidates"][candidate]
            vector: dict[str, int] = {
                f"population:{workload}": integer(
                    source["workloads_p50_ps_per_op"][workload]["median"],
                    f"{dimension}/{candidate}/{workload} median",
                )
                for workload in population.STEADY_WORKLOADS
            }
            vector["population:construction-p99"] = integer(
                source["construction_p99_ns"]["median"],
                f"{dimension}/{candidate}/construction median",
            )
            vector["memory:rss-loaded-delta-kib"] = integer(
                source["rss_loaded_delta_kib"]["median"],
                f"{dimension}/{candidate}/rss median",
            )
            deterministic_record = deterministic[dimension][candidate]
            vector["memory:logical-owned-bytes"] = integer(
                deterministic_record["logical_owned_bytes"], "logical owned bytes"
            )
            vector["memory:max-owned-bytes"] = integer(
                deterministic_record["max_owned_bytes"], "max owned bytes"
            )
            vector.update(synthetic_values[candidate])
            if set(vector) != set(registry.all):
                raise ParetoEvidenceError(
                    f"metric vector shape drifted for {dimension}/{candidate}"
                )
            if any(value < 0 for value in vector.values()):
                raise ParetoEvidenceError(
                    f"negative decision metric for {dimension}/{candidate}"
                )
            result[dimension][candidate] = vector
    return result


def strictly_dominates(left: dict[str, int], right: dict[str, int]) -> bool:
    if set(left) != set(right):
        raise ParetoEvidenceError("cannot compare vectors with different metric registries")
    return all(left[key] <= right[key] for key in left) and any(
        left[key] < right[key] for key in left
    )


def dimension_frontier(
    vectors: dict[str, dict[str, int]],
) -> tuple[list[str], dict[str, list[str]]]:
    dominators: dict[str, list[str]] = {}
    frontier: list[str] = []
    for candidate in population.PRODUCTION_CANDIDATES:
        found = sorted(
            other
            for other in population.PRODUCTION_CANDIDATES
            if other != candidate
            and strictly_dominates(vectors[other], vectors[candidate])
        )
        dominators[candidate] = found
        if not found:
            frontier.append(candidate)
    return sorted(frontier), dominators


def all_dimension_dominators(
    vectors: dict[str, dict[str, dict[str, int]]]
) -> dict[str, list[str]]:
    result: dict[str, list[str]] = {}
    for candidate in population.PRODUCTION_CANDIDATES:
        result[candidate] = sorted(
            other
            for other in population.PRODUCTION_CANDIDATES
            if other != candidate
            and all(
                strictly_dominates(
                    vectors[dimension][other], vectors[dimension][candidate]
                )
                for dimension in population.DIMENSIONS
            )
        )
    return result


def improvement_ppm(baseline: int, observed: int) -> int:
    if baseline <= 0:
        return 0
    return (baseline - observed) * 1_000_000 // baseline


def materiality(
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
            base = vectors[dimension]["direct"]
            candidate_values = vectors[dimension][candidate]
            for metric in registry.latency:
                gain = improvement_ppm(base[metric], candidate_values[metric])
                best_latency = max(best_latency, gain)
                if gain >= CPU_MATERIAL_IMPROVEMENT_PPM:
                    qualifying.append(
                        {"dimension": dimension, "metric": metric, "improvement_ppm": gain}
                    )
            for metric in registry.memory:
                gain = improvement_ppm(base[metric], candidate_values[metric])
                best_memory = max(best_memory, gain)
                if gain >= MEMORY_MATERIAL_IMPROVEMENT_PPM:
                    qualifying.append(
                        {"dimension": dimension, "metric": metric, "improvement_ppm": gain}
                    )
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
    *, repo_root: Path, combined_artifact: Path, correctness_paths: list[Path]
) -> dict[str, object]:
    repo_root = repo_root.resolve()
    root = combined_artifact.resolve()
    if not root.is_dir():
        raise ParetoEvidenceError(f"combined artifact is not a directory: {root}")
    target = target_contract(repo_root)
    artifact, files, artifact_sha = validate_root_artifact(root)
    combined_record, combined_sha = validate_combined(root, artifact, files)
    population_record = validate_population_nested(root, combined_record)
    validate_population_manifest(root, combined_record)
    synthetic_aggregates = validate_synthetic(combined_record)
    correctness = validate_correctness(
        correctness_paths,
        expected_commit=str(combined_record["identities"]["repository_commit_sha"]),
        target=target,
    )
    deterministic = population_deterministic_diagnostics(
        root, population_record, target
    )
    synthetic_values, replacement_keys, promotion_keys = synthetic_medians(
        synthetic_aggregates
    )
    registry = metric_registry(replacement_keys, promotion_keys)
    vectors = build_vectors(
        population_record, deterministic, synthetic_values, registry
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
    global_dominators = all_dimension_dominators(vectors)
    dominated = sorted(
        candidate for candidate, found in global_dominators.items() if found
    )
    survivors = sorted(
        candidate
        for candidate in population.PRODUCTION_CANDIDATES
        if candidate not in dominated
    )
    frontier_sets = [
        set(dimension_results[dimension]["production_pareto_frontier"])  # type: ignore[index]
        for dimension in population.DIMENSIONS
    ]
    common_frontier = sorted(set.intersection(*frontier_sets))
    benefit = materiality(vectors, registry)
    blockers = ["explicit production-policy selection record not yet committed"]
    if not common_frontier:
        blockers.append(
            "no single production candidate lies on every standard-dimension frontier"
        )
    unjustified = [
        candidate
        for candidate in survivors
        if candidate != "direct" and not bool(benefit[candidate]["material"])  # type: ignore[index]
    ]
    if unjustified:
        blockers.append(
            "Pareto survivors without material complexity justification: "
            + ", ".join(sorted(unjustified))
        )

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
            "correctness": {
                candidate: {
                    "path": identity.path.name,
                    "sha256": identity.file_sha256,
                    "evidence_id": identity.evidence_id,
                }
                for candidate, identity in sorted(correctness.items())
            },
            "target": target,
        },
        "hardware": {
            "cpu": combined_record["cpu"],
            "cpu_topology": combined_record["cpu_topology"],
            "rounds": combined_record["rounds"],
        },
        "metric_registry": {
            "lower_is_better": list(registry.all),
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
            "strictly_dominated_candidates": dominated,
            "global_dominators": global_dominators,
            "pareto_survivors": survivors,
            "common_all_dimension_frontier": common_frontier,
            "material_benefit_vs_direct": benefit,
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
    global_record = record["global"]
    assert isinstance(global_record, dict)
    print(
        "section Pareto analysis: "
        f"survivors={global_record['pareto_survivors']} "
        f"common_frontier={global_record['common_all_dimension_frontier']} "
        f"analysis={record['analysis_sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
