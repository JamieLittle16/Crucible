#!/usr/bin/env python3
"""Run the section Pareto analyzer with strict raw synthetic-evidence reconciliation."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

try:
    from tools import section_pareto_decision as pareto
    from tools import section_target_hardware as population
    from tools import section_target_synthetic_evidence as synthetic
except ModuleNotFoundError:  # Direct execution from tools/.
    import section_pareto_decision as pareto  # type: ignore[no-redef]
    import section_target_hardware as population  # type: ignore[no-redef]
    import section_target_synthetic_evidence as synthetic  # type: ignore[no-redef]

SCHEMA = 1
KIND = "section-pareto-strict-input-audit"


class StrictParetoError(RuntimeError):
    """Raised when compact Pareto inputs cannot be reconciled to retained raw evidence."""


def canonical_digest(value: object) -> str:
    return pareto.canonical_digest(value)


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise StrictParetoError(f"{path} must contain a JSON object")
    return value


def safe_relative(raw: object, label: str) -> Path:
    try:
        return pareto.safe_relative(raw, label)
    except pareto.ParetoEvidenceError as error:
        raise StrictParetoError(str(error)) from error


def expected_target(repo_root: Path) -> dict[str, object]:
    try:
        return pareto.target_contract(repo_root)
    except pareto.ParetoEvidenceError as error:
        raise StrictParetoError(str(error)) from error


def compact_summary(child: dict[str, Any]) -> dict[str, object]:
    keys = (
        "control_p50_ps_per_op",
        "replacement_p50_ps_per_op",
        "promotion_p99_ns",
    )
    result: dict[str, object] = {}
    for key in keys:
        if key not in child:
            raise StrictParetoError(f"compact synthetic child omitted {key}")
        result[key] = child[key]
    return result


def audit_raw_synthetic(
    *, repo_root: Path, combined_artifact: Path
) -> dict[str, object]:
    repo_root = repo_root.resolve()
    root = combined_artifact.resolve()
    record = load_json(root / "combined-orchestration.json")
    if record.get("schema") != 1 or record.get("kind") != "section-target-combined-orchestration":
        raise StrictParetoError("combined orchestration schema/kind mismatch")
    if record.get("mode") != "qualification":
        raise StrictParetoError("strict Pareto input audit requires qualification evidence")
    if record.get("decision_scope") != pareto.DECISION_SCOPE:
        raise StrictParetoError("combined decision scope drifted")
    if record.get("cross_dimension_score_allowed") is not False:
        raise StrictParetoError("combined evidence enabled cross-dimension scoring")

    rounds = record.get("rounds")
    cpu = record.get("cpu")
    identities = record.get("identities")
    synthetic_block = record.get("synthetic")
    if isinstance(rounds, bool) or not isinstance(rounds, int) or rounds < 5 or rounds % 5:
        raise StrictParetoError("strict audit requires at least five rounds and a multiple of five")
    if isinstance(cpu, bool) or not isinstance(cpu, int):
        raise StrictParetoError("combined CPU identity is malformed")
    if not isinstance(identities, dict):
        raise StrictParetoError("combined identities are missing")
    if not isinstance(synthetic_block, dict):
        raise StrictParetoError("combined synthetic block is missing")
    children = synthetic_block.get("children")
    if not isinstance(children, list):
        raise StrictParetoError("combined synthetic children are missing")

    schedule = synthetic.schedule(rounds)
    if len(children) != len(schedule):
        raise StrictParetoError(
            f"synthetic child count mismatch: expected {len(schedule)}, got {len(children)}"
        )
    target = expected_target(repo_root)
    head_sha = identities.get("repository_commit_sha")
    binary_sha = identities.get("benchmark_executable_sha256")
    try:
        pareto.git_sha(head_sha, "combined repository commit")
        pareto.sha256(binary_sha, "combined benchmark executable")
    except pareto.ParetoEvidenceError as error:
        raise StrictParetoError(str(error)) from error

    observed_paths: set[str] = set()
    raw_identities: list[dict[str, object]] = []
    exact_replacement_surface = {
        (workload, pattern, cardinality)
        for pattern, cardinality in synthetic.QUALIFICATION_CASES
        for workload in synthetic.REPLACEMENTS
    }
    exact_promotions = set(synthetic.PROMOTIONS)

    for index, (child, scheduled) in enumerate(zip(children, schedule, strict=True)):
        if not isinstance(child, dict):
            raise StrictParetoError(f"synthetic child {index} is malformed")
        if child.get("round") != scheduled.round_index:
            raise StrictParetoError(f"synthetic child {index} round schedule drifted")
        if child.get("candidate_position") != scheduled.candidate_position:
            raise StrictParetoError(f"synthetic child {index} candidate position drifted")
        if child.get("candidate") != scheduled.candidate:
            raise StrictParetoError(f"synthetic child {index} candidate schedule drifted")
        if child.get("benchmark_executable_sha256") != binary_sha:
            raise StrictParetoError(f"synthetic child {index} executable identity drifted")

        relative = safe_relative(child.get("child_evidence_path"), f"synthetic child {index} path")
        relative_text = relative.as_posix()
        if relative_text in observed_paths:
            raise StrictParetoError(f"duplicate raw synthetic child path: {relative_text}")
        observed_paths.add(relative_text)
        path = root / relative
        if not path.is_file():
            raise StrictParetoError(f"raw synthetic child is missing: {relative_text}")
        expected_sha = child.get("child_evidence_sha256")
        try:
            expected_sha = pareto.sha256(expected_sha, f"synthetic child {index} SHA")
        except pareto.ParetoEvidenceError as error:
            raise StrictParetoError(str(error)) from error
        actual_sha = pareto.sha256_file(path)
        if actual_sha != expected_sha:
            raise StrictParetoError(
                f"raw synthetic child identity mismatch: {relative_text}"
            )
        raw = load_json(path)
        expectation = synthetic.ChildExpectation(
            candidate=scheduled.candidate,
            mode="qualification",
            head_sha=str(head_sha),
            cpu=cpu,
            target=target,
        )
        try:
            synthetic.validate_child(raw, expectation)
            summary = synthetic.child_summary(raw)
        except synthetic.SyntheticEvidenceError as error:
            raise StrictParetoError(
                f"raw synthetic child failed validation ({relative_text}): {error}"
            ) from error
        if summary != compact_summary(child):
            raise StrictParetoError(
                f"compact synthetic summary disagrees with raw child: {relative_text}"
            )

        timings = raw.get("timings")
        if not isinstance(timings, list):
            raise StrictParetoError(f"raw synthetic timings missing: {relative_text}")
        replacements: set[tuple[str, str, int]] = set()
        promotions: set[str] = set()
        for timing in timings:
            if not isinstance(timing, dict):
                raise StrictParetoError(f"raw synthetic timing malformed: {relative_text}")
            workload = timing.get("workload")
            if workload in synthetic.REPLACEMENTS:
                pattern = timing.get("pattern")
                pool = timing.get("pool_cardinality")
                if not isinstance(pattern, str) or isinstance(pool, bool) or not isinstance(pool, int):
                    raise StrictParetoError(f"raw replacement identity malformed: {relative_text}")
                replacements.add((str(workload), pattern, pool))
            elif workload in synthetic.PROMOTIONS:
                promotions.add(str(workload))
        if replacements != exact_replacement_surface:
            raise StrictParetoError(
                f"raw replacement surface drifted after validation: {relative_text}"
            )
        if promotions != exact_promotions:
            raise StrictParetoError(
                f"raw promotion surface drifted after validation: {relative_text}"
            )
        raw_identities.append(
            {
                "round": scheduled.round_index,
                "candidate_position": scheduled.candidate_position,
                "candidate": scheduled.candidate,
                "path": relative_text,
                "sha256": actual_sha,
                "summary_sha256": canonical_digest(summary),
            }
        )

    audit: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "rounds": rounds,
        "child_count": len(children),
        "candidate_order": list(synthetic.CANDIDATES),
        "qualification_case_count": len(synthetic.QUALIFICATION_CASES),
        "replacement_workload_count": len(synthetic.REPLACEMENTS),
        "expected_replacement_records_per_child": len(exact_replacement_surface),
        "expected_promotion_records_per_child": len(exact_promotions),
        "raw_children": raw_identities,
    }
    audit["audit_sha256"] = canonical_digest(audit)
    return audit


def tighten_selection_readiness(analysis: dict[str, Any]) -> None:
    global_record = analysis.get("global")
    if not isinstance(global_record, dict):
        raise StrictParetoError("Pareto analysis lacks global result block")
    common = global_record.get("common_all_dimension_frontier")
    material = global_record.get("material_benefit_vs_direct")
    if not isinstance(common, list) or not isinstance(material, dict):
        raise StrictParetoError("Pareto common-frontier/materiality result is malformed")
    selectable: list[str] = []
    for candidate in common:
        if candidate not in population.PRODUCTION_CANDIDATES:
            raise StrictParetoError(f"common frontier contains non-production candidate: {candidate}")
        candidate_material = material.get(candidate)
        if not isinstance(candidate_material, dict):
            raise StrictParetoError(f"materiality missing for common-frontier candidate: {candidate}")
        if candidate == "direct" or candidate_material.get("material") is True:
            selectable.append(str(candidate))
    analysis["selection_ready"] = bool(selectable)
    analysis["selectable_common_frontier"] = sorted(selectable)
    blockers = analysis.get("selection_blockers")
    if not isinstance(blockers, list) or not all(isinstance(item, str) for item in blockers):
        raise StrictParetoError("Pareto selection blockers are malformed")
    blocker = "no common-frontier candidate clears the final complexity-selection gate"
    filtered = [item for item in blockers if item != blocker]
    if not selectable:
        filtered.append(blocker)
    analysis["selection_blockers"] = filtered


def analyze_strict(
    *, repo_root: Path, combined_artifact: Path, correctness_paths: list[Path]
) -> dict[str, object]:
    audit = audit_raw_synthetic(
        repo_root=repo_root, combined_artifact=combined_artifact
    )
    try:
        analysis = pareto.analyze(
            repo_root=repo_root,
            combined_artifact=combined_artifact,
            correctness_paths=correctness_paths,
        )
    except pareto.ParetoEvidenceError as error:
        raise StrictParetoError(str(error)) from error
    analysis.pop("analysis_sha256", None)
    analysis["strict_input_audit"] = audit
    tighten_selection_readiness(analysis)
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
        record = analyze_strict(
            repo_root=args.repo_root,
            combined_artifact=args.combined_artifact,
            correctness_paths=args.correctness,
        )
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (StrictParetoError, OSError, json.JSONDecodeError) as error:
        print(f"strict section Pareto analysis error: {error}")
        return 1
    print(
        "strict section Pareto analysis: "
        f"selectable={record['selectable_common_frontier']} "
        f"analysis={record['analysis_sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
