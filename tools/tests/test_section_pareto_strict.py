from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import section_policy_decision as policy
from tools import section_pareto_strict as strict
from tools import section_target_synthetic_evidence as synthetic
from tools.tests.test_section_pareto_decision import (
    GIT_SHA,
    QualificationFixture,
    target,
    write_json,
)


def timing_summary(*, sample_count: int, operations: int, elapsed_ns: int) -> dict[str, object]:
    return {
        "samples_ns": [elapsed_ns] * sample_count,
        "operations_per_sample": operations,
        "p50_ns": elapsed_ns,
        "p95_ns": elapsed_ns,
        "p99_ns": elapsed_ns,
        "max_ns": elapsed_ns,
        "p50_ps_per_op": elapsed_ns * 1_000 // operations,
    }


def representation(candidate: str) -> str:
    return {
        "direct-reference": "direct-reference",
        "direct": "direct-n",
        "adaptive": "local-8",
        "fast-local": "local-8",
        "packed-local": "packed-8",
    }[candidate]


def raw_child(candidate: str) -> dict[str, object]:
    settings = synthetic.expected_settings("qualification")
    timings: list[dict[str, object]] = []
    for pattern, cardinality in synthetic.QUALIFICATION_CASES:
        for workload in synthetic.REPLACEMENTS:
            timings.append(
                {
                    "workload": workload,
                    "pattern": pattern,
                    "pool_cardinality": cardinality,
                    "actual_cardinality": cardinality,
                    "representation": representation(candidate),
                    "unit": "replace",
                    "timing": timing_summary(
                        sample_count=settings["measured_samples"],
                        operations=settings["mutations"],
                        elapsed_ns=settings["mutations"],
                    ),
                }
            )
    for target_cardinality in synthetic.PROMOTION_TARGETS:
        timings.append(
            {
                "workload": f"promotion-to-{target_cardinality}",
                "pattern": "promotion-boundary",
                "pool_cardinality": target_cardinality,
                "actual_cardinality": target_cardinality,
                "representation": f"{representation(candidate)}->{representation(candidate)}",
                "unit": "single-replace",
                "timing": timing_summary(
                    sample_count=settings["promotion_samples"],
                    operations=1,
                    elapsed_ns=100,
                ),
            }
        )
    return {
        "schema": synthetic.SCHEMA,
        "harness_version": synthetic.HARNESS_VERSION,
        "scope": synthetic.SCOPE,
        "mode": "qualification",
        "candidate": candidate,
        "production_candidate": candidate != "direct-reference",
        "build_profile": synthetic.BUILD_PROFILE,
        "codegen_policy": synthetic.CODEGEN_POLICY,
        "commit_sha": GIT_SHA,
        "rustflags": "",
        "cargo_encoded_rustflags": "",
        "cpus_allowed_list": "0",
        "mems_allowed_list": "0",
        **target(),
        "settings": settings,
        "promotion_targets": list(synthetic.PROMOTION_TARGETS),
        "control": {
            "workload": synthetic.CONTROL_WORKLOAD,
            "unit": "iteration",
            "timing": timing_summary(
                sample_count=settings["measured_samples"],
                operations=settings["control_operations"],
                elapsed_ns=settings["control_operations"],
            ),
        },
        "timings": timings,
    }


def add_raw_synthetic_evidence(fixture: QualificationFixture) -> None:
    combined_path = fixture.root / "combined-orchestration.json"
    record = json.loads(combined_path.read_text(encoding="utf-8"))
    binary_sha = record["identities"]["benchmark_executable_sha256"]
    children: list[dict[str, object]] = []
    for scheduled in synthetic.schedule(5):
        raw = raw_child(scheduled.candidate)
        relative = Path("synthetic") / f"round-{scheduled.round_index:02d}" / (
            f"{scheduled.candidate}-p{scheduled.candidate_position}.json"
        )
        path = fixture.root / relative
        write_json(path, raw)
        children.append(
            {
                "round": scheduled.round_index,
                "candidate_position": scheduled.candidate_position,
                "candidate": scheduled.candidate,
                "benchmark_executable_sha256": binary_sha,
                "child_evidence_path": relative.as_posix(),
                "child_evidence_sha256": strict.pareto.sha256_file(path),
                **synthetic.child_summary(raw),
            }
        )
    aggregates = synthetic.aggregate_children(children)
    record["synthetic"]["children"] = children
    record["synthetic"]["aggregates"] = aggregates
    record["synthetic"]["noise_qualification"] = synthetic.classify_noise(
        aggregates, smoke=False, rounds=5
    )
    record["evidence_sha256"] = strict.canonical_digest(
        {key: value for key, value in record.items() if key != "evidence_sha256"}
    )
    write_json(combined_path, record)
    fixture.reseal_root_manifest()


def reseal_combined(fixture: QualificationFixture, record: dict[str, object]) -> None:
    record["evidence_sha256"] = strict.canonical_digest(
        {key: value for key, value in record.items() if key != "evidence_sha256"}
    )
    write_json(fixture.root / "combined-orchestration.json", record)
    fixture.reseal_root_manifest()


class StrictParetoTests(unittest.TestCase):
    def test_strict_analysis_reconciles_every_raw_synthetic_child(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = QualificationFixture(Path(raw))
            add_raw_synthetic_evidence(fixture)
            result = strict.analyze_strict(
                repo_root=fixture.repo,
                combined_artifact=fixture.root,
                correctness_paths=fixture.correctness_paths,
            )
            audit = result["strict_input_audit"]
            self.assertEqual(audit["child_count"], 25)
            self.assertEqual(audit["expected_replacement_records_per_child"], 88)
            self.assertEqual(audit["expected_promotion_records_per_child"], 9)
            self.assertEqual(
                audit["audit_sha256"],
                strict.canonical_digest(
                    {key: value for key, value in audit.items() if key != "audit_sha256"}
                ),
            )
            self.assertEqual(
                result["analysis_sha256"],
                strict.canonical_digest(
                    {key: value for key, value in result.items() if key != "analysis_sha256"}
                ),
            )
            # The merged explicit policy validator accepts the enriched analysis schema because
            # the strict audit is itself bound by the recomputed analysis digest.
            self.assertEqual(policy.validate_analysis(result), result["analysis_sha256"])

    def test_resealed_compact_summary_tamper_is_rejected_against_raw_child(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = QualificationFixture(Path(raw))
            add_raw_synthetic_evidence(fixture)
            path = fixture.root / "combined-orchestration.json"
            record = json.loads(path.read_text(encoding="utf-8"))
            record["synthetic"]["children"][0]["control_p50_ps_per_op"] += 1
            aggregates = synthetic.aggregate_children(record["synthetic"]["children"])
            record["synthetic"]["aggregates"] = aggregates
            record["synthetic"]["noise_qualification"] = synthetic.classify_noise(
                aggregates, smoke=False, rounds=5
            )
            reseal_combined(fixture, record)
            with self.assertRaises(strict.StrictParetoError):
                strict.analyze_strict(
                    repo_root=fixture.repo,
                    combined_artifact=fixture.root,
                    correctness_paths=fixture.correctness_paths,
                )

    def test_resealed_raw_surface_corruption_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = QualificationFixture(Path(raw))
            add_raw_synthetic_evidence(fixture)
            combined_path = fixture.root / "combined-orchestration.json"
            record = json.loads(combined_path.read_text(encoding="utf-8"))
            child = record["synthetic"]["children"][0]
            child_path = fixture.root / child["child_evidence_path"]
            raw_child_record = json.loads(child_path.read_text(encoding="utf-8"))
            raw_child_record["timings"].pop()
            write_json(child_path, raw_child_record)
            child["child_evidence_sha256"] = strict.pareto.sha256_file(child_path)
            reseal_combined(fixture, record)
            with self.assertRaises(strict.StrictParetoError):
                strict.analyze_strict(
                    repo_root=fixture.repo,
                    combined_artifact=fixture.root,
                    correctness_paths=fixture.correctness_paths,
                )

    def test_rotated_schedule_identity_is_enforced(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = QualificationFixture(Path(raw))
            add_raw_synthetic_evidence(fixture)
            path = fixture.root / "combined-orchestration.json"
            record = json.loads(path.read_text(encoding="utf-8"))
            record["synthetic"]["children"][0]["candidate_position"] = 4
            reseal_combined(fixture, record)
            with self.assertRaises(strict.StrictParetoError):
                strict.audit_raw_synthetic(
                    repo_root=fixture.repo, combined_artifact=fixture.root
                )

    def test_selection_ready_requires_an_actually_selectable_common_candidate(self) -> None:
        analysis = {
            "global": {
                "common_all_dimension_frontier": ["adaptive"],
                "material_benefit_vs_direct": {
                    "adaptive": {"material": False}
                },
            },
            "selection_blockers": [
                "explicit production-policy selection record not yet committed"
            ],
        }
        strict.tighten_selection_readiness(analysis)
        self.assertFalse(analysis["selection_ready"])
        self.assertEqual(analysis["selectable_common_frontier"], [])
        self.assertIn(
            "no common-frontier candidate clears the final complexity-selection gate",
            analysis["selection_blockers"],
        )

        analysis["global"]["common_all_dimension_frontier"] = ["direct"]
        analysis["global"]["material_benefit_vs_direct"] = {
            "direct": {"material": True}
        }
        strict.tighten_selection_readiness(analysis)
        self.assertTrue(analysis["selection_ready"])
        self.assertEqual(analysis["selectable_common_frontier"], ["direct"])

    def test_raw_audit_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = QualificationFixture(Path(raw))
            add_raw_synthetic_evidence(fixture)
            first = strict.audit_raw_synthetic(
                repo_root=fixture.repo, combined_artifact=fixture.root
            )
            second = strict.audit_raw_synthetic(
                repo_root=fixture.repo, combined_artifact=fixture.root
            )
            self.assertEqual(first, second)


if __name__ == "__main__":
    unittest.main()
