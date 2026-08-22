#!/usr/bin/env python3
"""Validate and seal an explicit production section-policy choice."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path
from typing import Any

try:
    from tools import section_pareto_decision as pareto
    from tools import section_target_hardware as population
except ModuleNotFoundError:  # Direct execution from tools/.
    import section_pareto_decision as pareto  # type: ignore[no-redef]
    import section_target_hardware as population  # type: ignore[no-redef]

SCHEMA = 1
SPEC_SCHEMA = 1
SPEC_KIND = "section-production-policy-spec"
KIND = "section-production-policy-decision"
LOWER_SHA256 = re.compile(r"[0-9a-f]{64}\Z")


class PolicyDecisionError(RuntimeError):
    """Raised when an explicit policy choice is unsupported by Pareto evidence."""


def canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise PolicyDecisionError(f"{path} must contain a JSON object")
    return value


def nonempty_text(value: object, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise PolicyDecisionError(f"{label} must be non-empty text")
    return value.strip()


def validate_analysis(record: dict[str, Any]) -> str:
    if record.get("schema") != pareto.SCHEMA or record.get("kind") != pareto.KIND:
        raise PolicyDecisionError("input is not a section Pareto decision analysis")
    digest = record.get("analysis_sha256")
    if not isinstance(digest, str) or LOWER_SHA256.fullmatch(digest) is None:
        raise PolicyDecisionError("Pareto analysis digest is malformed")
    payload = dict(record)
    payload.pop("analysis_sha256")
    if canonical_digest(payload) != digest:
        raise PolicyDecisionError("Pareto analysis digest does not recompute")
    if record.get("analysis_complete") is not True:
        raise PolicyDecisionError("Pareto analysis is incomplete")
    if record.get("selection_ready") is not True:
        raise PolicyDecisionError("Pareto analysis is not ready for an explicit selection")
    if record.get("decision_evidence_eligible") is not False:
        raise PolicyDecisionError("pre-selection Pareto analysis must not claim final decision eligibility")
    if record.get("decision_scope") != pareto.DECISION_SCOPE:
        raise PolicyDecisionError("Pareto decision scope drifted")
    if record.get("cross_dimension_score_allowed") is not False:
        raise PolicyDecisionError("Pareto analysis illegally enables cross-dimension scoring")
    interpretation = record.get("interpretation")
    if not isinstance(interpretation, dict):
        raise PolicyDecisionError("Pareto interpretation block is missing")
    if interpretation.get("direct_reference_selectable") is not False:
        raise PolicyDecisionError("Pareto analysis made the reference oracle selectable")
    if interpretation.get("cross_dimension_weighting_used") is not False:
        raise PolicyDecisionError("Pareto analysis used forbidden cross-dimension weighting")
    if interpretation.get("winner_selected") is not False:
        raise PolicyDecisionError("Pareto analyzer must not pre-select the production winner")
    return digest


def validate_policy_spec(spec: dict[str, Any]) -> tuple[str, str, dict[str, str]]:
    expected_keys = {
        "schema",
        "kind",
        "default_candidate",
        "selection_rationale",
        "nonselected_survivor_rationales",
    }
    if set(spec) != expected_keys:
        raise PolicyDecisionError(
            "policy specification fields drifted; expected exactly " + ", ".join(sorted(expected_keys))
        )
    if spec.get("schema") != SPEC_SCHEMA or spec.get("kind") != SPEC_KIND:
        raise PolicyDecisionError("policy specification schema/kind mismatch")
    selected = spec.get("default_candidate")
    if selected not in population.PRODUCTION_CANDIDATES:
        raise PolicyDecisionError(f"default candidate is not a production candidate: {selected!r}")
    rationale = nonempty_text(spec.get("selection_rationale"), "selection rationale")
    raw_rationales = spec.get("nonselected_survivor_rationales")
    if not isinstance(raw_rationales, dict):
        raise PolicyDecisionError("nonselected survivor rationales must be an object")
    rationales: dict[str, str] = {}
    for candidate, text in raw_rationales.items():
        if candidate not in population.PRODUCTION_CANDIDATES:
            raise PolicyDecisionError(f"rationale names unknown production candidate: {candidate!r}")
        rationales[str(candidate)] = nonempty_text(text, f"rationale for {candidate}")
    return str(selected), rationale, rationales


def select(
    *, analysis: dict[str, Any], policy_spec: dict[str, Any]
) -> dict[str, object]:
    analysis_sha = validate_analysis(analysis)
    selected, selection_rationale, supplied_rationales = validate_policy_spec(policy_spec)

    global_record = analysis.get("global")
    dimensions = analysis.get("dimensions")
    if not isinstance(global_record, dict) or not isinstance(dimensions, dict):
        raise PolicyDecisionError("Pareto analysis lacks global/dimension result blocks")
    survivors = global_record.get("pareto_survivors")
    dominated = global_record.get("strictly_dominated_candidates")
    common = global_record.get("common_all_dimension_frontier")
    dominators = global_record.get("global_dominators")
    material = global_record.get("material_benefit_vs_direct")
    if not isinstance(survivors, list) or not isinstance(dominated, list) or not isinstance(common, list):
        raise PolicyDecisionError("Pareto survivor/dominated/common-frontier sets are malformed")
    if not isinstance(dominators, dict) or not isinstance(material, dict):
        raise PolicyDecisionError("Pareto dominator/materiality blocks are malformed")
    production_set = set(population.PRODUCTION_CANDIDATES)
    if set(survivors) | set(dominated) != production_set or set(survivors) & set(dominated):
        raise PolicyDecisionError("Pareto survivors/dominated sets do not partition production candidates")
    if selected not in survivors or selected in dominated:
        raise PolicyDecisionError(f"selected candidate is globally rejected: {selected}")
    if selected not in common:
        raise PolicyDecisionError(
            f"selected candidate is not on every standard-dimension frontier: {selected}"
        )

    frontier_proof: dict[str, bool] = {}
    for dimension in population.DIMENSIONS:
        raw = dimensions.get(dimension)
        if not isinstance(raw, dict):
            raise PolicyDecisionError(f"Pareto analysis omitted dimension {dimension}")
        frontier = raw.get("production_pareto_frontier")
        if not isinstance(frontier, list):
            raise PolicyDecisionError(f"dimension frontier is malformed: {dimension}")
        on_frontier = selected in frontier
        frontier_proof[dimension] = on_frontier
        if not on_frontier:
            raise PolicyDecisionError(
                f"selected candidate is absent from {dimension} Pareto frontier"
            )

    selected_materiality = material.get(selected)
    if not isinstance(selected_materiality, dict):
        raise PolicyDecisionError("selected candidate lacks materiality evidence")
    if selected != "direct" and selected_materiality.get("material") is not True:
        raise PolicyDecisionError(
            f"complex selected candidate does not clear the materiality gate: {selected}"
        )

    nonselected_survivors = sorted(set(survivors) - {selected})
    if set(supplied_rationales) != set(nonselected_survivors):
        missing = sorted(set(nonselected_survivors) - set(supplied_rationales))
        extra = sorted(set(supplied_rationales) - set(nonselected_survivors))
        raise PolicyDecisionError(
            f"survivor rationale coverage mismatch; missing={missing} extra={extra}"
        )

    dominated_record: dict[str, object] = {}
    for candidate in sorted(dominated):
        raw = dominators.get(candidate)
        if not isinstance(raw, list) or not raw:
            raise PolicyDecisionError(f"dominated candidate lacks a valid dominator: {candidate}")
        if any(item not in population.PRODUCTION_CANDIDATES for item in raw):
            raise PolicyDecisionError(f"dominator registry contains a non-production candidate: {candidate}")
        dominated_record[candidate] = {"dominators": sorted(raw), "classification": "strictly-dominated"}

    identities = analysis.get("identities")
    hardware = analysis.get("hardware")
    thresholds = analysis.get("materiality_thresholds_ppm")
    if not isinstance(identities, dict) or not isinstance(hardware, dict) or not isinstance(thresholds, dict):
        raise PolicyDecisionError("Pareto provenance/materiality metadata is incomplete")

    decision: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "decision_evidence_eligible": True,
        "production_policy_selected": True,
        "production_pruning_ready": True,
        "decision_scope": pareto.DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
        "pareto_analysis_sha256": analysis_sha,
        "identities": identities,
        "hardware": hardware,
        "materiality_thresholds_ppm": thresholds,
        "selected": {
            "default_candidate": selected,
            "selection_rationale": selection_rationale,
            "all_dimension_frontier_membership": frontier_proof,
            "material_benefit_vs_direct": selected_materiality,
        },
        "nonselected": {
            "strictly_dominated": dominated_record,
            "pareto_survivors": {
                candidate: {
                    "classification": "nonselected-pareto-survivor",
                    "rationale": supplied_rationales[candidate],
                    "material_benefit_vs_direct": material[candidate],
                }
                for candidate in nonselected_survivors
            },
        },
        "reference_oracle": {
            "candidate": "direct-reference",
            "production_candidate": False,
            "retained_for_qualification": True,
        },
        "pruning_contract": {
            "selected_production_candidate": selected,
            "nonselected_production_candidates": sorted(production_set - {selected}),
            "candidate_registry_update_required": True,
            "experiment_log_update_required": True,
            "experimental_knowledge_must_be_retained": True,
        },
    }
    decision["decision_sha256"] = canonical_digest(decision)
    return decision


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--analysis", type=Path, required=True)
    result.add_argument("--policy", type=Path, required=True)
    result.add_argument("--output", type=Path, required=True)
    return result


def main() -> int:
    args = parser().parse_args()
    try:
        decision = select(
            analysis=load_json(args.analysis), policy_spec=load_json(args.policy)
        )
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(decision, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (PolicyDecisionError, OSError, json.JSONDecodeError) as error:
        print(f"section production-policy decision error: {error}")
        return 1
    print(
        "section production-policy decision: "
        f"default={decision['selected']['default_candidate']} "  # type: ignore[index]
        f"decision={decision['decision_sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
