#!/usr/bin/env python3
"""Run the complete M0.3D measurement-to-Pareto qualification chain."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any

try:
    from tools import section_benchmark_pack as packs
    from tools import section_correctness_bundle as correctness
    from tools import section_pareto_decision as pareto
    from tools import section_target_combined as combined
    from tools import section_target_hardware as population
except ModuleNotFoundError:  # Direct execution from tools/.
    import section_benchmark_pack as packs  # type: ignore[no-redef]
    import section_correctness_bundle as correctness  # type: ignore[no-redef]
    import section_pareto_decision as pareto  # type: ignore[no-redef]
    import section_target_combined as combined  # type: ignore[no-redef]
    import section_target_hardware as population  # type: ignore[no-redef]

SCHEMA = 1
KIND = "section-m03d-qualification-session"
ARTIFACT_SCHEMA = 1
ARTIFACT_KIND = "section-m03d-qualification-artifact"
SESSION_NAME = "qualification-session.json"
ARTIFACT_NAME = "session-artifact-manifest.json"
PARETO_NAME = "pareto-analysis.json"
DECISION_SCOPE = "dimension-separated-only"
LOWER_SHA256 = re.compile(r"[0-9a-f]{64}\Z")


class FinalQualificationError(RuntimeError):
    """Raised when the final M0.3D evidence chain cannot proceed safely."""


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


def _sha256(value: object, label: str) -> str:
    if not isinstance(value, str) or LOWER_SHA256.fullmatch(value) is None:
        raise FinalQualificationError(f"{label} must be canonical lowercase SHA-256")
    return value


def _verify_digest_field(record: dict[str, Any], field: str, label: str) -> str:
    expected = _sha256(record.get(field), f"{label}.{field}")
    payload = dict(record)
    payload.pop(field)
    actual = canonical_digest(payload)
    if actual != expected:
        raise FinalQualificationError(
            f"{label} digest mismatch: expected {expected}, got {actual}"
        )
    return expected


def _is_within(path: Path, root: Path) -> bool:
    try:
        path.resolve().relative_to(root.resolve())
    except ValueError:
        return False
    return True


def _overlap(left: Path, right: Path) -> bool:
    return _is_within(left, right) or _is_within(right, left)


def _require_external_input(path: Path, repo_root: Path, label: str) -> Path:
    if path.is_symlink():
        raise FinalQualificationError(f"{label} root must not be a symlink: {path}")
    resolved = path.resolve()
    if _is_within(resolved, repo_root):
        raise FinalQualificationError(f"{label} must live outside the repository")
    if not resolved.is_dir():
        raise FinalQualificationError(f"{label} must be a real directory: {resolved}")
    return resolved


def _prepare_output_root(
    path: Path,
    repo_root: Path,
    representative_root: Path,
    correctness_bundle: Path,
) -> Path:
    resolved = path.resolve()
    if _is_within(resolved, repo_root):
        raise FinalQualificationError("qualification output root must live outside the repository")
    if _overlap(resolved, representative_root):
        raise FinalQualificationError(
            "qualification output root must be disjoint from the representative artifact"
        )
    if _overlap(resolved, correctness_bundle):
        raise FinalQualificationError(
            "qualification output root must be disjoint from the correctness bundle"
        )
    if resolved.exists():
        raise FinalQualificationError(
            "qualification output root must not already exist; use a fresh path for every session"
        )
    resolved.mkdir(parents=True)
    return resolved


def _require_scope(record: dict[str, Any], label: str) -> None:
    if record.get("decision_scope") != DECISION_SCOPE:
        raise FinalQualificationError(f"{label} decision scope drifted")
    if record.get("cross_dimension_score_allowed") is not False:
        raise FinalQualificationError(f"{label} illegally enables cross-dimension scoring")


def _json_text(value: object) -> str:
    return json.dumps(value, indent=2, sort_keys=True) + "\n"


def _artifact_manifest(
    output_root: Path, session_sha256: str, session_bytes: bytes
) -> dict[str, object]:
    files: list[dict[str, object]] = []
    manifest_path = output_root / ARTIFACT_NAME
    session_path = output_root / SESSION_NAME
    if session_path.exists() or session_path.is_symlink():
        raise FinalQualificationError("qualification session seal unexpectedly already exists")
    for path in sorted(output_root.rglob("*")):
        if path.is_symlink():
            raise FinalQualificationError(
                f"qualification output contains forbidden symlink: {path.relative_to(output_root)}"
            )
        if not path.is_file() or path == manifest_path:
            continue
        files.append(
            {
                "path": path.relative_to(output_root).as_posix(),
                "size": path.stat().st_size,
                "sha256": sha256_file(path),
            }
        )
    files.append(
        {
            "path": SESSION_NAME,
            "size": len(session_bytes),
            "sha256": hashlib.sha256(session_bytes).hexdigest(),
        }
    )
    files.sort(key=lambda entry: str(entry["path"]))
    manifest: dict[str, object] = {
        "schema": ARTIFACT_SCHEMA,
        "kind": ARTIFACT_KIND,
        "session_sha256": session_sha256,
        "files": files,
    }
    manifest["manifest_sha256"] = canonical_digest(manifest)
    return manifest


def _write_json(path: Path, value: object) -> None:
    path.write_text(_json_text(value), encoding="utf-8")


def _correctness_paths(bundle_root: Path) -> list[Path]:
    return [bundle_root / candidate / "full.json" for candidate in correctness.CANDIDATES]


def _revalidate_pack_manifest(
    pack_root: Path,
    *,
    expected_manifest_sha: str,
    expected_policy: str,
    expected_population_sha: str,
    expected_admission_sha: str,
    expected_source_artifact_sha: str,
) -> dict[str, Any]:
    """Reopen the generated pack manifest and require its frozen identity to be unchanged."""
    manifest_path = pack_root / "pack-manifest.json"
    if manifest_path.is_symlink() or not manifest_path.is_file():
        raise FinalQualificationError(
            "benchmark pack manifest must remain a real non-symlink file"
        )
    try:
        raw = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise FinalQualificationError(
            f"benchmark pack manifest cannot be reopened safely: {error}"
        ) from error
    if not isinstance(raw, dict):
        raise FinalQualificationError("benchmark pack manifest must contain a JSON object")

    _require_scope(raw, "benchmark pack manifest")
    reopened_sha = _verify_digest_field(
        raw, "manifest_sha256", "benchmark pack manifest"
    )
    if reopened_sha != expected_manifest_sha:
        raise FinalQualificationError(
            "benchmark pack manifest changed during qualification session"
        )
    if raw.get("policy") != expected_policy:
        raise FinalQualificationError("benchmark pack representative policy changed")
    if (
        _sha256(raw.get("population_sha256"), "benchmark pack population digest")
        != expected_population_sha
    ):
        raise FinalQualificationError("benchmark pack population identity changed")
    if (
        _sha256(raw.get("admission_sha256"), "benchmark pack admission digest")
        != expected_admission_sha
    ):
        raise FinalQualificationError("benchmark pack admission identity changed")
    if (
        _sha256(
            raw.get("source_artifact_manifest_sha256"),
            "benchmark pack source artifact manifest digest",
        )
        != expected_source_artifact_sha
    ):
        raise FinalQualificationError(
            "benchmark pack source artifact identity changed"
        )
    return raw


def run_qualification(
    *,
    repo_root: Path,
    representative_root: Path,
    correctness_bundle: Path,
    output_root: Path,
    cpu: int,
    rounds: int = 5,
    timeout_seconds: int = 900,
) -> dict[str, object]:
    """Run one controlled qualification session and seal its evidence state."""
    if rounds < 5 or rounds % 5 != 0:
        raise FinalQualificationError(
            "final qualification requires at least five rounds and a multiple of five"
        )
    if timeout_seconds <= 0:
        raise FinalQualificationError("child timeout must be positive")
    if cpu not in population.allowed_cpus():
        raise FinalQualificationError(
            f"requested CPU {cpu} is outside current process affinity"
        )

    repo_root = repo_root.resolve()
    head_sha = population.repository_identity(repo_root)
    representative_root = _require_external_input(
        representative_root, repo_root, "representative artifact"
    )
    correctness_bundle = _require_external_input(
        correctness_bundle, repo_root, "correctness bundle"
    )
    if _overlap(representative_root, correctness_bundle):
        raise FinalQualificationError(
            "representative artifact and correctness bundle must be disjoint evidence roots"
        )

    sealed_correctness = correctness.validate_sealed_bundle(
        repo_root=repo_root,
        bundle_root=correctness_bundle,
        expected_commit=head_sha,
    )
    correctness_sha = _sha256(
        sealed_correctness.get("bundle_sha256"), "correctness bundle digest"
    )

    output_root = _prepare_output_root(
        output_root, repo_root, representative_root, correctness_bundle
    )
    pack_root = output_root / "packs"
    pack_record = packs.build_packs(
        root=representative_root,
        output_dir=pack_root,
        state_manifest=repo_root / "vanilla/state-data/26.2-state-data-manifest.json",
        generated_rust=repo_root / "crates/data/helve-generated/src/lib.rs",
    )
    _require_scope(pack_record, "benchmark pack")
    pack_manifest_sha = _verify_digest_field(
        pack_record, "manifest_sha256", "benchmark pack"
    )
    source_artifact_sha = _sha256(
        pack_record.get("source_artifact_manifest_sha256"),
        "representative artifact manifest digest",
    )
    population_sha = _sha256(
        pack_record.get("population_sha256"), "representative population digest"
    )
    admission_sha = _sha256(
        pack_record.get("admission_sha256"), "representative admission digest"
    )
    representative_policy = pack_record.get("policy")
    if not isinstance(representative_policy, str) or not representative_policy:
        raise FinalQualificationError("representative policy identity is malformed")
    _revalidate_pack_manifest(
        pack_root,
        expected_manifest_sha=pack_manifest_sha,
        expected_policy=representative_policy,
        expected_population_sha=population_sha,
        expected_admission_sha=admission_sha,
        expected_source_artifact_sha=source_artifact_sha,
    )

    combined_root = output_root / "combined"
    combined_record = combined.orchestrate(
        repo_root=repo_root,
        pack_root=pack_root,
        output_dir=combined_root,
        cpu=cpu,
        rounds=rounds,
        smoke=False,
        timeout_seconds=timeout_seconds,
    )
    _require_scope(combined_record, "combined evidence")
    if combined_record.get("mode") != "qualification":
        raise FinalQualificationError("combined evidence is not qualification mode")
    if combined_record.get("qualification_complete") is not True:
        raise FinalQualificationError("combined evidence did not complete")
    if combined_record.get("decision_evidence_eligible") is not False:
        raise FinalQualificationError("combined measurement layer claimed final decision eligibility")
    combined_sha = _verify_digest_field(
        combined_record, "evidence_sha256", "combined evidence"
    )

    identities = combined_record.get("identities")
    if not isinstance(identities, dict):
        raise FinalQualificationError("combined evidence identities are missing")
    if identities.get("repository_commit_sha") != head_sha:
        raise FinalQualificationError("combined evidence source revision changed during session")

    combined_eligible = combined_record.get("combined_measurement_evidence_eligible") is True
    analysis: dict[str, object] | None = None
    analysis_sha: str | None = None
    if combined_eligible:
        analysis = pareto.analyze(
            repo_root=repo_root,
            combined_artifact=combined_root,
            correctness_paths=_correctness_paths(correctness_bundle),
        )
        _require_scope(analysis, "Pareto analysis")
        if analysis.get("analysis_complete") is not True:
            raise FinalQualificationError("Pareto analysis did not complete")
        if analysis.get("decision_evidence_eligible") is not False:
            raise FinalQualificationError("Pareto analysis claimed final decision eligibility")
        interpretation = analysis.get("interpretation")
        if not isinstance(interpretation, dict) or interpretation.get("winner_selected") is not False:
            raise FinalQualificationError("Pareto analysis pre-selected a production winner")
        analysis_sha = _verify_digest_field(
            analysis, "analysis_sha256", "Pareto analysis"
        )
        _write_json(output_root / PARETO_NAME, analysis)

    if population.repository_identity(repo_root) != head_sha:
        raise FinalQualificationError("repository changed during final qualification session")
    correctness_bundle = _require_external_input(
        correctness_bundle, repo_root, "correctness bundle"
    )
    representative_root = _require_external_input(
        representative_root, repo_root, "representative artifact"
    )
    reopened_correctness = correctness.validate_sealed_bundle(
        repo_root=repo_root,
        bundle_root=correctness_bundle,
        expected_commit=head_sha,
    )
    if reopened_correctness.get("bundle_sha256") != correctness_sha:
        raise FinalQualificationError("correctness bundle changed during qualification session")
    reopened_population = packs.admit_population(representative_root)
    if (
        reopened_population.population_sha256 != population_sha
        or reopened_population.admission_sha256 != admission_sha
        or reopened_population.artifact_manifest_sha256 != source_artifact_sha
        or reopened_population.policy != representative_policy
    ):
        raise FinalQualificationError(
            "representative population artifact changed during qualification session"
        )
    _revalidate_pack_manifest(
        pack_root,
        expected_manifest_sha=pack_manifest_sha,
        expected_policy=representative_policy,
        expected_population_sha=population_sha,
        expected_admission_sha=admission_sha,
        expected_source_artifact_sha=source_artifact_sha,
    )

    combined_manifest = json.loads(
        (combined_root / "artifact-manifest.json").read_text(encoding="utf-8")
    )
    if not isinstance(combined_manifest, dict):
        raise FinalQualificationError("combined artifact manifest is malformed")
    combined_manifest_sha = _verify_digest_field(
        combined_manifest, "manifest_sha256", "combined artifact manifest"
    )

    blockers: list[str] = []
    if not combined_eligible:
        raw_blockers = combined_record.get("decision_blockers")
        if isinstance(raw_blockers, list):
            blockers.extend(str(item) for item in raw_blockers)
        blockers.append("combined target-hardware measurement evidence did not pass eligibility")
    elif analysis is not None:
        raw_blockers = analysis.get("selection_blockers")
        if isinstance(raw_blockers, list):
            blockers.extend(str(item) for item in raw_blockers)

    decision_review_ready = bool(
        combined_eligible
        and analysis is not None
        and analysis.get("selection_ready") is True
    )
    if decision_review_ready:
        blockers = [
            blocker
            for blocker in blockers
            if blocker != "explicit production-policy selection record not yet committed"
        ]
        blockers.append("explicit human-reviewed production-policy selection remains required")

    session: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "session_complete": True,
        "measurement_evidence_eligible": combined_eligible,
        "pareto_analysis_complete": analysis is not None,
        "decision_review_ready": decision_review_ready,
        "decision_evidence_eligible": False,
        "production_policy_selected": False,
        "decision_scope": DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
        "repository_commit_sha": head_sha,
        "cpu": cpu,
        "rounds": rounds,
        "identities": {
            "correctness_bundle_sha256": correctness_sha,
            "representative_policy": representative_policy,
            "representative_artifact_manifest_sha256": source_artifact_sha,
            "population_sha256": population_sha,
            "population_admission_sha256": admission_sha,
            "pack_manifest_sha256": pack_manifest_sha,
            "combined_evidence_sha256": combined_sha,
            "combined_artifact_manifest_sha256": combined_manifest_sha,
            "pareto_analysis_sha256": analysis_sha,
        },
        "paths": {
            "packs": "packs",
            "combined": "combined",
            "pareto_analysis": None if analysis is None else PARETO_NAME,
        },
        "blockers": sorted(set(blockers)),
    }
    session["session_sha256"] = canonical_digest(session)
    session_text = _json_text(session)
    session_bytes = session_text.encode("utf-8")
    artifact = _artifact_manifest(
        output_root, str(session["session_sha256"]), session_bytes
    )
    _write_json(output_root / ARTIFACT_NAME, artifact)
    (output_root / SESSION_NAME).write_text(session_text, encoding="utf-8")
    return session


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--repo-root", type=Path, default=Path("."))
    result.add_argument("--representative-root", type=Path, required=True)
    result.add_argument("--correctness-bundle", type=Path, required=True)
    result.add_argument("--output-root", type=Path, required=True)
    result.add_argument("--cpu", type=int, required=True)
    result.add_argument("--rounds", type=int, default=5)
    result.add_argument("--child-timeout-seconds", type=int, default=900)
    return result


def main() -> int:
    args = parser().parse_args()
    try:
        session = run_qualification(
            repo_root=args.repo_root,
            representative_root=args.representative_root,
            correctness_bundle=args.correctness_bundle,
            output_root=args.output_root,
            cpu=args.cpu,
            rounds=args.rounds,
            timeout_seconds=args.child_timeout_seconds,
        )
    except (
        FinalQualificationError,
        correctness.CorrectnessBundleError,
        packs.PackError,
        combined.CombinedEvidenceError,
        population.QualificationError,
        pareto.ParetoEvidenceError,
        OSError,
        json.JSONDecodeError,
    ) as error:
        print(f"section M0.3D final qualification error: {error}")
        return 1

    print(
        "section M0.3D qualification: "
        f"measurement_eligible={session['measurement_evidence_eligible']} "
        f"pareto_complete={session['pareto_analysis_complete']} "
        f"decision_review_ready={session['decision_review_ready']} "
        f"session={session['session_sha256']}"
    )
    return 0 if session["decision_review_ready"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
