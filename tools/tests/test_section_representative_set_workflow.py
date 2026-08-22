from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/section-representative-set-qualification.yml"


class RepresentativeSetWorkflowTests(unittest.TestCase):
    def text(self) -> str:
        return WORKFLOW.read_text(encoding="utf-8")

    def test_workflow_is_manual_qualification_not_pr_timing(self) -> None:
        text = self.text()
        self.assertIn("  workflow_dispatch:\n", text)
        self.assertNotIn("  pull_request:\n", text)
        self.assertNotIn("  schedule:\n", text)
        self.assertNotIn("--qualification", text)
        self.assertNotIn("--smoke", text)

    def test_all_four_frozen_members_run_the_full_individual_gate(self) -> None:
        text = self.text()
        self.assertIn("for seed_index in 0 1 2 3; do", text)
        for required in (
            "tools/official_representative_section_world.py",
            "tools/representative_section_corpus.py",
            "tools/section_corpus.py validate",
            "--corpus-check",
            "--corpus-decision-check",
            "purpose representative-member and is not decision-eligible",
        ):
            self.assertIn(required, text)

    def test_population_firewall_is_required_after_members(self) -> None:
        text = self.text()
        self.assertIn("tools/section_corpus_set.py", text)
        self.assertIn("record['member_count'] != 4", text)
        self.assertIn("record['decision_scope'] != 'dimension-separated-only'", text)
        self.assertIn("record['cross_dimension_score_allowed']", text)
        self.assertIn("len({member['corpus_sha256'] for member in record['members']}) != 4", text)
        self.assertIn("'minecraft:overworld', 'minecraft:the_nether', 'minecraft:the_end'", text)
        self.assertIn("'candidates' in record['aggregate']", text)
        self.assertIn("'cardinality_histogram' in record['aggregate']", text)

    def test_workflow_uses_exact_plan_and_pinned_actions(self) -> None:
        text = self.text()
        self.assertIn(
            "fecb9c9bc77aa9689ceaf6d88fa9af96019a48d9533269f3bd15824f7dfc7191",
            text,
        )
        self.assertIn("actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803", text)
        self.assertIn("actions/setup-java@0f481fcb613427c0f801b606911222b5b6f3083a", text)
        self.assertIn("actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02", text)
        self.assertIn("rustup toolchain install 1.97.1 --profile minimal", text)

    def test_complete_evidence_is_retained_for_target_hardware_handoff(self) -> None:
        text = self.text()
        self.assertIn("member.corpus", text)
        self.assertIn("world-evidence.json", text)
        self.assertIn("extraction-evidence.json", text)
        self.assertIn("corpus-manifest.json", text)
        self.assertIn("rust-import.json", text)
        self.assertIn("decision-rejection.log", text)
        self.assertIn("server.log", text)
        self.assertIn("corpus-set.json", text)
        self.assertIn("artifact-manifest.json", text)
        self.assertIn("retention-days: 30", text)


if __name__ == "__main__":
    unittest.main()
