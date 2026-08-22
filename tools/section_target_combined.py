#!/usr/bin/env python3
"""Combine population and synthetic section evidence in one controlled hardware session."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any

try:
    from tools import section_target_hardware as population
    from tools import section_target_synthetic_evidence as synthetic
except ModuleNotFoundError:  # Direct execution from tools/.
    import section_target_hardware as population  # type: ignore[no-redef]
    import section_target_synthetic_evidence as synthetic  # type: ignore[no-redef]

SCHEMA = 1
KIND = "section-target-combined-orchestration"
ARTIFACT_SCHEMA = 1
ARTIFACT_KIND = "section-target-combined-artifact"
DECISION_SCOPE = "dimension-separated-only"


class CombinedEvidenceError(RuntimeError):
    """Raised when population and synthetic evidence cannot be combined safely."""


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
        raise CombinedEvidenceError(f"{path} must contain a JSON object")
    return value


def verify_digest_field(record: dict[str, Any], field: str, label: str) -> str:
    expected = record.get(field)
    if not isinstance(expected, str) or len(expected) != 64:
        raise CombinedEvidenceError(f"{label}.{field} is not a SHA-256 digest")
    payload = dict(record)
    payload.pop(field)
    actual = canonical_digest(payload)
    if actual != expected:
        raise CombinedEvidenceError(
            f"{label} digest mismatch: expected {expected}, got {actual}"
        )
    return expected


def runtime_environment(source: dict[str, str]) -> dict[str, str]:
    blocked = population.forbidden_environment(source)
    if blocked:
        raise CombinedEvidenceError(
            "combined qualification refuses compiler/profile overrides: "
            + ", ".join(blocked)
        )
    environment = dict(source)
    for key in list(environment):
        if key.startswith("CARGO_PROFILE_RELEASE_"):
            environment.pop(key, None)
        elif key.startswith("CARGO_TARGET_") and key.endswith("_RUSTFLAGS"):
            environment.pop(key, None)
    for key in (
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTC_BOOTSTRAP",
        "CARGO_BUILD_TARGET",
        "CARGO_BUILD_RUSTFLAGS",
    ):
        environment.pop(key, None)
    environment["RUSTFLAGS"] = ""
    environment["CARGO_ENCODED_RUSTFLAGS"] = ""
    return environment


def validate_population_record(
    record: dict[str, Any],
    *,
    head_sha: str,
    cpu: int,
    rounds: int,
    smoke: bool,
    binary_path: Path,
) -> str:
    if record.get("schema") != population.SCHEMA or record.get("kind") != population.KIND:
        raise CombinedEvidenceError("population orchestration schema/kind mismatch")
    expected_mode = "smoke" if smoke else "qualification"
    if record.get("mode") != expected_mode or record.get("rounds") != rounds:
        raise CombinedEvidenceError("population orchestration mode/round mismatch")
    if record.get("qualification_complete") is not True:
        raise CombinedEvidenceError("population orchestration is incomplete")
    if record.get("cpu") != cpu:
        raise CombinedEvidenceError("population orchestration CPU mismatch")
    if record.get("decision_scope") != DECISION_SCOPE:
        raise CombinedEvidenceError("population decision scope drifted")
    if record.get("cross_dimension_score_allowed") is not False:
        raise CombinedEvidenceError("population orchestration enabled cross-dimension scoring")
    if record.get("decision_evidence_eligible") is not False:
        raise CombinedEvidenceError("population-only evidence cannot claim final decision eligibility")
    if not isinstance(record.get("population_evidence_eligible"), bool):
        raise CombinedEvidenceError("population eligibility flag is malformed")

    identities = record.get("identities")
    if not isinstance(identities, dict):
        raise CombinedEvidenceError("population identities are missing")
    if identities.get("repository_commit_sha") != head_sha:
        raise CombinedEvidenceError("population source commit mismatch")
    binary_sha = identities.get("benchmark_executable_sha256")
    if not isinstance(binary_sha, str) or sha256_file(binary_path) != binary_sha:
        raise CombinedEvidenceError("retained benchmark executable identity mismatch")

    build = record.get("build")
    if not isinstance(build, dict):
        raise CombinedEvidenceError("population build provenance is missing")
    if build.get("offline") is not True or build.get("profile") != population.BUILD_PROFILE:
        raise CombinedEvidenceError("population build was not the admitted controlled release build")
    if build.get("codegen_policy") != population.CODEGEN_POLICY:
        raise CombinedEvidenceError("population codegen policy drifted")

    return verify_digest_field(record, "evidence_sha256", "population orchestration")


def validate_population_artifact(
    artifact: dict[str, Any], *, population_evidence_sha256: str, population_dir: Path
) -> str:
    if artifact.get("schema") != population.ARTIFACT_SCHEMA or artifact.get("kind") != population.ARTIFACT_KIND:
        raise CombinedEvidenceError("population artifact schema/kind mismatch")
    if artifact.get("orchestration_sha256") != population_evidence_sha256:
        raise CombinedEvidenceError("population artifact is not bound to orchestration evidence")
    manifest_sha = verify_digest_field(artifact, "manifest_sha256", "population artifact")
    files = artifact.get("files")
    if not isinstance(files, list) or not files:
        raise CombinedEvidenceError("population artifact file inventory is missing")
    observed: set[str] = set()
    for index, raw in enumerate(files):
        if not isinstance(raw, dict):
            raise CombinedEvidenceError(f"population artifact file {index} is malformed")
        relative = raw.get("path")
        expected_sha = raw.get("sha256")
        expected_size = raw.get("size")
        if not isinstance(relative, str) or not relative or Path(relative).is_absolute():
            raise CombinedEvidenceError("population artifact contains an unsafe path")
        if relative in observed:
            raise CombinedEvidenceError("population artifact contains duplicate file paths")
        observed.add(relative)
        path = population_dir / relative
        if not path.is_file():
            raise CombinedEvidenceError(f"population artifact file is missing: {relative}")
        if path.stat().st_size != expected_size or sha256_file(path) != expected_sha:
            raise CombinedEvidenceError(f"population artifact file identity mismatch: {relative}")
    if "benchmark-executable" not in observed or "orchestration.json" not in observed:
        raise CombinedEvidenceError("population artifact omitted required evidence files")
    return manifest_sha


def run_synthetic_child(
    *,
    scheduled: synthetic.ScheduledSyntheticChild,
    binary: Path,
    binary_sha256: str,
    repo_root: Path,
    output_dir: Path,
    environment: dict[str, str],
    head_sha: str,
    cpu: int,
    target: dict[str, object],
    smoke: bool,
    timeout_seconds: int,
) -> dict[str, object]:
    if population.repository_identity(repo_root) != head_sha:
        raise CombinedEvidenceError("repository changed before synthetic child")
    before_binary_sha = sha256_file(binary)
    if before_binary_sha != binary_sha256:
        raise CombinedEvidenceError("retained benchmark executable changed before synthetic child")

    child_dir = output_dir / "synthetic" / f"round-{scheduled.round_index:02d}"
    child_dir.mkdir(parents=True, exist_ok=True)
    stem = f"{scheduled.candidate}-p{scheduled.candidate_position}"
    json_path = child_dir / f"{stem}.json"
    stdout_path = child_dir / f"{stem}.stdout"
    stderr_path = child_dir / f"{stem}.stderr"
    if any(path.exists() for path in (json_path, stdout_path, stderr_path)):
        raise CombinedEvidenceError("synthetic child evidence path already exists")

    command = [
        "taskset",
        "-c",
        str(cpu),
        str(binary),
        "--synthetic-candidate",
        scheduled.candidate,
        "--synthetic-target-smoke" if smoke else "--synthetic-target-qualification",
        "--output",
        str(json_path),
    ]
    before = population.environment_snapshot(cpu)
    start = time.monotonic_ns()
    result = subprocess.run(
        command,
        cwd=repo_root,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout_seconds,
        check=False,
    )
    elapsed_ns = time.monotonic_ns() - start
    after = population.environment_snapshot(cpu)
    stdout_path.write_text(result.stdout, encoding="utf-8")
    stderr_path.write_text(result.stderr, encoding="utf-8")
    if result.returncode != 0:
        raise CombinedEvidenceError(f"synthetic benchmark child failed: {' '.join(command)}")
    if not json_path.is_file():
        raise CombinedEvidenceError("successful synthetic child did not emit evidence JSON")

    if population.repository_identity(repo_root) != head_sha:
        raise CombinedEvidenceError("repository changed during synthetic child")
    after_binary_sha = sha256_file(binary)
    if after_binary_sha != binary_sha256 or after_binary_sha != before_binary_sha:
        raise CombinedEvidenceError("retained benchmark executable changed during synthetic child")

    record = load_json(json_path)
    expectation = synthetic.ChildExpectation(
        candidate=scheduled.candidate,
        mode="smoke" if smoke else "qualification",
        head_sha=head_sha,
        cpu=cpu,
        target=target,
    )
    synthetic.validate_child(record, expectation)
    normalized = synthetic.child_summary(record)
    return {
        "round": scheduled.round_index,
        "candidate_position": scheduled.candidate_position,
        "candidate": scheduled.candidate,
        "benchmark_executable_sha256": binary_sha256,
        "child_evidence_path": json_path.relative_to(output_dir).as_posix(),
        "child_evidence_sha256": sha256_file(json_path),
        "stdout_sha256": sha256_file(stdout_path),
        "stderr_sha256": sha256_file(stderr_path),
        "elapsed_ns": elapsed_ns,
        "environment_before": before,
        "environment_after": after,
        **normalized,
    }


def decision_blockers(population_eligible: bool, synthetic_eligible: bool) -> list[str]:
    blockers: list[str] = []
    if not population_eligible:
        blockers.append("population evidence did not pass protocol/noise eligibility")
    if not synthetic_eligible:
        blockers.append("synthetic mechanism evidence did not pass protocol/noise eligibility")
    blockers.append("dimension-separated Pareto selection record not assembled")
    return blockers


def artifact_manifest(output_dir: Path, evidence_sha256: str) -> dict[str, object]:
    files: list[dict[str, object]] = []
    root_manifest = output_dir / "artifact-manifest.json"
    for path in sorted(output_dir.rglob("*")):
        if not path.is_file() or path == root_manifest:
            continue
        files.append(
            {
                "path": path.relative_to(output_dir).as_posix(),
                "size": path.stat().st_size,
                "sha256": sha256_file(path),
            }
        )
    manifest: dict[str, object] = {
        "schema": ARTIFACT_SCHEMA,
        "kind": ARTIFACT_KIND,
        "combined_evidence_sha256": evidence_sha256,
        "files": files,
    }
    manifest["manifest_sha256"] = canonical_digest(manifest)
    return manifest


def orchestrate(
    *,
    repo_root: Path,
    pack_root: Path,
    output_dir: Path,
    cpu: int,
    rounds: int,
    smoke: bool,
    timeout_seconds: int,
) -> dict[str, object]:
    if shutil.which("taskset") is None:
        raise CombinedEvidenceError("combined qualification requires taskset")
    repo_root = repo_root.resolve()
    pack_root = pack_root.resolve()
    output_dir = output_dir.resolve()
    population.require_external_evidence_paths(repo_root, pack_root, output_dir)
    if cpu not in population.allowed_cpus():
        raise CombinedEvidenceError(f"requested CPU {cpu} is outside current process affinity")
    if rounds <= 0:
        raise CombinedEvidenceError("round count must be positive")

    head_sha = population.repository_identity(repo_root)
    pack_set = population.verify_pack_set(pack_root, repo_root)
    output_dir.mkdir(parents=True, exist_ok=True)
    population_dir = output_dir / "population"

    population_record = population.orchestrate(
        repo_root=repo_root,
        pack_root=pack_root,
        output_dir=population_dir,
        cpu=cpu,
        rounds=rounds,
        smoke=smoke,
        timeout_seconds=timeout_seconds,
    )
    binary = population_dir / "benchmark-executable"
    population_evidence_sha = validate_population_record(
        population_record,
        head_sha=head_sha,
        cpu=cpu,
        rounds=rounds,
        smoke=smoke,
        binary_path=binary,
    )
    population_artifact = load_json(population_dir / "artifact-manifest.json")
    population_artifact_sha = validate_population_artifact(
        population_artifact,
        population_evidence_sha256=population_evidence_sha,
        population_dir=population_dir,
    )
    binary_sha = sha256_file(binary)

    environment = runtime_environment(dict(os.environ))
    scheduled = synthetic.schedule(rounds)
    synthetic_children = [
        run_synthetic_child(
            scheduled=item,
            binary=binary,
            binary_sha256=binary_sha,
            repo_root=repo_root,
            output_dir=output_dir,
            environment=environment,
            head_sha=head_sha,
            cpu=cpu,
            target=pack_set.target,
            smoke=smoke,
            timeout_seconds=timeout_seconds,
        )
        for item in scheduled
    ]

    if population.repository_identity(repo_root) != head_sha:
        raise CombinedEvidenceError("repository changed during combined evidence session")
    if sha256_file(binary) != binary_sha:
        raise CombinedEvidenceError("retained benchmark executable changed after combined session")
    for entry in pack_set.entries.values():
        if sha256_file(entry.path) != entry.sha256:
            raise CombinedEvidenceError(f"benchmark pack changed during combined session: {entry.dimension}")

    synthetic_aggregates = synthetic.aggregate_children(synthetic_children)
    synthetic_noise = synthetic.classify_noise(
        synthetic_aggregates, smoke=smoke, rounds=rounds
    )
    population_eligible = bool(population_record["population_evidence_eligible"])
    synthetic_eligible = bool(synthetic_noise["synthetic_evidence_eligible"])
    measurement_eligible = population_eligible and synthetic_eligible

    combined: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "mode": "smoke" if smoke else "qualification",
        "qualification_complete": True,
        "population_evidence_eligible": population_eligible,
        "synthetic_evidence_eligible": synthetic_eligible,
        "combined_measurement_evidence_eligible": measurement_eligible,
        "decision_evidence_eligible": False,
        "decision_blockers": decision_blockers(population_eligible, synthetic_eligible),
        "decision_scope": DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
        "rounds": rounds,
        "cpu": cpu,
        "cpu_topology": population.cpu_topology(cpu),
        "candidates": list(population.CANDIDATES),
        "production_candidates": list(population.PRODUCTION_CANDIDATES),
        "dimensions": list(population.DIMENSIONS),
        "identities": {
            "repository_commit_sha": head_sha,
            "benchmark_executable_sha256": binary_sha,
            "pack_manifest_sha256": pack_set.manifest_sha256,
            "representative_policy": pack_set.policy,
            "population_sha256": pack_set.population_sha256,
            "population_admission_sha256": pack_set.admission_sha256,
            "population_orchestration_evidence_sha256": population_evidence_sha,
            "population_artifact_manifest_sha256": population_artifact_sha,
        },
        "population": {
            "evidence_path": "population/orchestration.json",
            "artifact_manifest_path": "population/artifact-manifest.json",
            "aggregates": population_record["aggregates"],
            "noise_qualification": population_record["noise_qualification"],
        },
        "synthetic": {
            "schedule": [
                {
                    "round": item.round_index,
                    "candidate_position": item.candidate_position,
                    "candidate": item.candidate,
                }
                for item in scheduled
            ],
            "children": synthetic_children,
            "aggregates": synthetic_aggregates,
            "noise_qualification": synthetic_noise,
        },
    }
    combined["evidence_sha256"] = canonical_digest(combined)
    (output_dir / "combined-orchestration.json").write_text(
        json.dumps(combined, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    artifact = artifact_manifest(output_dir, str(combined["evidence_sha256"]))
    (output_dir / "artifact-manifest.json").write_text(
        json.dumps(artifact, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return combined


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--repo-root", type=Path, default=Path("."))
    result.add_argument("--pack-root", type=Path, required=True)
    result.add_argument("--output-dir", type=Path, required=True)
    result.add_argument("--cpu", type=int, required=True)
    result.add_argument("--rounds", type=int)
    result.add_argument("--child-timeout-seconds", type=int, default=900)
    mode = result.add_mutually_exclusive_group(required=True)
    mode.add_argument("--smoke", action="store_true")
    mode.add_argument("--qualification", action="store_true")
    return result


def main() -> int:
    args = parser().parse_args()
    rounds = args.rounds if args.rounds is not None else (1 if args.smoke else 5)
    try:
        result = orchestrate(
            repo_root=args.repo_root,
            pack_root=args.pack_root,
            output_dir=args.output_dir,
            cpu=args.cpu,
            rounds=rounds,
            smoke=args.smoke,
            timeout_seconds=args.child_timeout_seconds,
        )
    except (
        CombinedEvidenceError,
        population.QualificationError,
        synthetic.SyntheticEvidenceError,
        OSError,
        json.JSONDecodeError,
        subprocess.TimeoutExpired,
    ) as error:
        print(f"section combined target-hardware qualification error: {error}")
        return 1
    print(
        "section combined target-hardware orchestration: "
        f"mode={result['mode']} rounds={result['rounds']} "
        f"population_eligible={result['population_evidence_eligible']} "
        f"synthetic_eligible={result['synthetic_evidence_eligible']} "
        f"combined_measurement_eligible={result['combined_measurement_evidence_eligible']} "
        f"decision_eligible={result['decision_evidence_eligible']} "
        f"evidence={result['evidence_sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
