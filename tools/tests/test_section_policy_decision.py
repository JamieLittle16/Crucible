from __future__ import annotations

import copy
import unittest

from tools import section_pareto_decision as pareto
from tools import section_policy_decision as policy
from tools import section_target_hardware as population


def analysis_fixture() -> dict[str, object]:
    dimensions: dict[str, object] = {}
    for dimension in population.DIMENSIONS:
        dimensions[dimension] = {
            "production_pareto_frontier": ["adaptive", "fast-local", "packed-local"],
            "dominators": {
                "direct": ["adaptive"],
                "adaptive": [],
                "fast-local": [],
                "packed-local": [],
            },
            "metrics": {},
            "diagnostics": {},
        }
    material = {
        "direct": {
            "baseline": True,
            "material": True,
            "best_latency_improvement_ppm": 0,
            "best_memory_improvement_ppm": 0,
            "qualifying_metrics": [],
        },
        "adaptive": {
            "baseline": False,
            "material": True,
            "best_latency_improvement_ppm": 100_000,
            "best_memory_improvement_ppm": 0,
            "qualifying_metrics": [
                {
                    "dimension": "minecraft:overworld",
                    "metric": "population:random-read",
                    "improvement_ppm": 100_000,
                }
            ],
        },
        "fast-local": {
            "baseline": False,
            "material": True,
            "best_latency_improvement_ppm": 0,
            "best_memory_improvement_ppm": 200_000,
            "qualifying_metrics": [
                {
                    "dimension": "minecraft:overworld",
                    "metric": "memory:logical-owned-bytes",
                    "improvement_ppm": 200_000,
                }
            ],
        },
        "packed-local": {
            "baseline": False,
            "material": True,
            "best_latency_improvement_ppm": 0,
            "best_memory_improvement_ppm": 300_000,
            "qualifying_metrics": [
                {
                    "dimension": "minecraft:overworld",
                    "metric": "memory:logical-owned-bytes",
                    "improvement_ppm": 300_000,
                }
            ],
        },
    }
    record: dict[str, object] = {
        "schema": pareto.SCHEMA,
        "kind": pareto.KIND,
        "analysis_complete": True,
        "selection_ready": True,
        "decision_evidence_eligible": False,
        "selection_blockers": ["explicit production-policy selection record not yet committed"],
        "decision_scope": pareto.DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
        "identities": {
            "repository_commit_sha": "c" * 40,
            "benchmark_executable_sha256": "d" * 64,
            "representative_policy": population.REPRESENTATIVE_POLICY,
            "population_sha256": "e" * 64,
            "population_admission_sha256": "f" * 64,
            "combined_evidence_sha256": "1" * 64,
            "combined_artifact_manifest_sha256": "2" * 64,
            "correctness": {},
            "target": {"minecraft_version": "26.2"},
        },
        "hardware": {"cpu": 2, "rounds": 5},
        "metric_registry": {},
        "materiality_thresholds_ppm": {
            "cpu_latency_tail": pareto.CPU_MATERIAL_IMPROVEMENT_PPM,
            "rss_logical_memory": pareto.MEMORY_MATERIAL_IMPROVEMENT_PPM,
        },
        "dimensions": dimensions,
        "global": {
            "strictly_dominated_candidates": ["direct"],
            "global_dominators": {
                "direct": ["adaptive"],
                "adaptive": [],
                "fast-local": [],
                "packed-local": [],
            },
            "pareto_survivors": ["adaptive", "fast-local", "packed-local"],
            "common_all_dimension_frontier": [
                "adaptive",
                "fast-local",
                "packed-local",
            ],
            "material_benefit_vs_direct": material,
        },
        "interpretation": {
            "direct_reference_selectable": False,
            "cross_dimension_weighting_used": False,
            "mathematical_dominance_is_not_complexity_justification": True,
            "winner_selected": False,
        },
    }
    record["analysis_sha256"] = policy.canonical_digest(record)
    return record


def spec(selected: str = "adaptive") -> dict[str, object]:
    survivors = {"adaptive", "fast-local", "packed-local"}
    return {
        "schema": policy.SPEC_SCHEMA,
        "kind": policy.SPEC_KIND,
        "default_candidate": selected,
        "selection_rationale": f"Select {selected} as the first strict-fidelity default.",
        "nonselected_survivor_rationales": {
            candidate: f"Retain {candidate} in the experiment record but not the first default."
            for candidate in sorted(survivors - {selected})
        },
    }


class PolicyDecisionTests(unittest.TestCase):
    def test_valid_common_frontier_material_candidate_can_be_selected(self) -> None:
        decision = policy.select(analysis=analysis_fixture(), policy_spec=spec("adaptive"))
        self.assertTrue(decision["decision_evidence_eligible"])
        self.assertTrue(decision["production_policy_selected"])
        self.assertTrue(decision["production_pruning_ready"])
        self.assertEqual(decision["selected"]["default_candidate"], "adaptive")
        self.assertEqual(
            decision["decision_sha256"],
            policy.canonical_digest(
                {key: value for key, value in decision.items() if key != "decision_sha256"}
            ),
        )

    def test_direct_baseline_can_be_selected_without_complexity_materiality(self) -> None:
        analysis = analysis_fixture()
        global_record = analysis["global"]
        global_record["strictly_dominated_candidates"] = []
        global_record["pareto_survivors"] = list(population.PRODUCTION_CANDIDATES)
        global_record["common_all_dimension_frontier"] = list(
            population.PRODUCTION_CANDIDATES
        )
        global_record["global_dominators"]["direct"] = []
        for dimension in population.DIMENSIONS:
            analysis["dimensions"][dimension]["production_pareto_frontier"] = list(
                population.PRODUCTION_CANDIDATES
            )
        analysis["analysis_sha256"] = policy.canonical_digest(
            {key: value for key, value in analysis.items() if key != "analysis_sha256"}
        )
        direct_spec = {
            "schema": policy.SPEC_SCHEMA,
            "kind": policy.SPEC_KIND,
            "default_candidate": "direct",
            "selection_rationale": "Prefer the simplest direct mechanism when evidence does not justify complexity.",
            "nonselected_survivor_rationales": {
                candidate: "The trade-off is retained but not selected for the first default."
                for candidate in population.PRODUCTION_CANDIDATES
                if candidate != "direct"
            },
        }
        decision = policy.select(analysis=analysis, policy_spec=direct_spec)
        self.assertEqual(decision["selected"]["default_candidate"], "direct")

    def test_reference_oracle_is_never_selectable(self) -> None:
        broken = spec("adaptive")
        broken["default_candidate"] = "direct-reference"
        with self.assertRaises(policy.PolicyDecisionError):
            policy.select(analysis=analysis_fixture(), policy_spec=broken)

    def test_globally_dominated_candidate_is_rejected(self) -> None:
        broken = spec("adaptive")
        broken["default_candidate"] = "direct"
        broken["nonselected_survivor_rationales"] = {
            candidate: "survivor rationale"
            for candidate in ("adaptive", "fast-local", "packed-local")
        }
        with self.assertRaises(policy.PolicyDecisionError):
            policy.select(analysis=analysis_fixture(), policy_spec=broken)

    def test_candidate_missing_one_dimension_frontier_is_rejected(self) -> None:
        analysis = analysis_fixture()
        analysis["dimensions"]["minecraft:the_end"]["production_pareto_frontier"].remove(
            "adaptive"
        )
        analysis["analysis_sha256"] = policy.canonical_digest(
            {key: value for key, value in analysis.items() if key != "analysis_sha256"}
        )
        with self.assertRaises(policy.PolicyDecisionError):
            policy.select(analysis=analysis, policy_spec=spec("adaptive"))

    def test_complex_candidate_without_material_gain_is_rejected(self) -> None:
        analysis = analysis_fixture()
        analysis["global"]["material_benefit_vs_direct"]["adaptive"]["material"] = False
        analysis["global"]["material_benefit_vs_direct"]["adaptive"][
            "qualifying_metrics"
        ] = []
        analysis["analysis_sha256"] = policy.canonical_digest(
            {key: value for key, value in analysis.items() if key != "analysis_sha256"}
        )
        with self.assertRaises(policy.PolicyDecisionError):
            policy.select(analysis=analysis, policy_spec=spec("adaptive"))

    def test_every_nonselected_survivor_requires_exact_rationale_coverage(self) -> None:
        broken = spec("adaptive")
        broken["nonselected_survivor_rationales"].pop("fast-local")
        with self.assertRaises(policy.PolicyDecisionError):
            policy.select(analysis=analysis_fixture(), policy_spec=broken)

        broken = spec("adaptive")
        broken["nonselected_survivor_rationales"]["direct"] = "not a survivor"
        with self.assertRaises(policy.PolicyDecisionError):
            policy.select(analysis=analysis_fixture(), policy_spec=broken)

    def test_cross_dimension_scoring_or_analysis_digest_tamper_is_rejected(self) -> None:
        analysis = analysis_fixture()
        analysis["cross_dimension_score_allowed"] = True
        analysis["analysis_sha256"] = policy.canonical_digest(
            {key: value for key, value in analysis.items() if key != "analysis_sha256"}
        )
        with self.assertRaises(policy.PolicyDecisionError):
            policy.select(analysis=analysis, policy_spec=spec())

        analysis = analysis_fixture()
        analysis["analysis_sha256"] = "0" * 64
        with self.assertRaises(policy.PolicyDecisionError):
            policy.select(analysis=analysis, policy_spec=spec())

    def test_survivor_dominated_partition_cannot_be_relabelled(self) -> None:
        analysis = analysis_fixture()
        analysis["global"]["pareto_survivors"].append("direct")
        analysis["analysis_sha256"] = policy.canonical_digest(
            {key: value for key, value in analysis.items() if key != "analysis_sha256"}
        )
        with self.assertRaises(policy.PolicyDecisionError):
            policy.select(analysis=analysis, policy_spec=spec())

    def test_policy_schema_is_closed_to_hidden_weighting_fields(self) -> None:
        broken = spec()
        broken["dimension_weights"] = {"minecraft:overworld": 1}
        with self.assertRaises(policy.PolicyDecisionError):
            policy.select(analysis=analysis_fixture(), policy_spec=broken)


if __name__ == "__main__":
    unittest.main()
