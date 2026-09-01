from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_delegate_closure_source_review as closure
from tools import r2c_world_state_delegate_review_apply as apply_review


def pretty(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def record(candidate_id: str, identity: str, group_id: str, focus: str, hazard: str | None) -> dict[str, object]:
    owner, signature = identity.split("#", 1)
    excerpt = f"SECRET_{candidate_id}\n"
    return {
        "candidate_id": candidate_id,
        "source_identity": identity,
        "source": {
            "type": owner,
            "signature": signature,
            "fingerprint_algorithm": "java-token-v2-literal-sensitive",
            "normalized_sha256": hashlib.sha256(f"norm:{candidate_id}".encode()).hexdigest(),
            "body_sha256": hashlib.sha256(f"body:{candidate_id}".encode()).hexdigest(),
        },
        "source_location": {"path": f"src/{candidate_id}.java", "start_line": 1, "end_line": 2},
        "atlas_observed_hazards": [] if hazard is None else [hazard],
        "atlas_classifications": [],
        "calls": {
            "call_sites": 0,
            "resolved_targets": [],
            "unresolved_call_sites": 0,
            "top_unresolved_callees": [],
        },
        "group_ids": [group_id],
        "review_focus": [focus],
        "source_excerpt": excerpt,
        "source_excerpt_sha256": hashlib.sha256(excerpt.encode()).hexdigest(),
    }


def fixture() -> tuple[bytes, dict[str, object]]:
    plan = closure._load_plan()
    records = [
        record(
            "DISC-NET-R2C-WORLD-DELEGATE-0001",
            "net.minecraft.Biome#selected()",
            plan.groups[0].group_id,
            plan.groups[0].review_focus,
            "CODEC",
        ),
        record(
            "DISC-NET-R2C-WORLD-DELEGATE-0002",
            "net.minecraft.Biome#rejected()",
            plan.groups[0].group_id,
            plan.groups[0].review_focus,
            None,
        ),
        record(
            "DISC-NET-R2C-WORLD-DELEGATE-0003",
            "net.minecraft.Light#selected()",
            plan.groups[1].group_id,
            plan.groups[1].review_focus,
            None,
        ),
        record(
            "DISC-NET-R2C-WORLD-DELEGATE-0004",
            "net.minecraft.Light#rejected()",
            plan.groups[1].group_id,
            plan.groups[1].review_focus,
            None,
        ),
    ]
    payloads = closure._payloads(
        plan=plan,
        plan_sha256=apply_review._sha256_file(closure.DEFAULT_PLAN),
        parent_plan_sha256=apply_review._sha256_file(closure.DEFAULT_PARENT_PLAN),
        frontier_sha256=apply_review._sha256_file(closure.DEFAULT_FRONTIER),
        source_sha256=closure.EXPECTED_SOURCE_SHA256,
        records=records,
    )
    worksheet = payloads["worksheet.json"]
    worksheet_value = json.loads(worksheet)
    decisions: dict[str, object] = {
        "schema": 1,
        "kind": apply_review.DECISION_KIND,
        "review_id": closure.PLAN_ID,
        "commit_policy": apply_review.DECISION_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": closure.EXPECTED_SOURCE_SHA256,
        "plan_sha256": apply_review._sha256_file(closure.DEFAULT_PLAN),
        "generated_worksheet_sha256": hashlib.sha256(worksheet).hexdigest(),
        "review_pack_sha256": worksheet_value["review_pack_sha256"],
        "automatic_source_selection_forbidden": True,
        "unselected_candidate_policy": apply_review.UNSELECTED_POLICY,
        "groups": [
            {
                "group_id": plan.groups[0].group_id,
                "parent_group_id": plan.groups[0].parent_group_id,
                "source_inspected": True,
                "selected_candidate_ids": ["DISC-NET-R2C-WORLD-DELEGATE-0001"],
                "hazards_reviewed": ["CODEC"],
                "followup_dependencies": [],
                "semantic_observations": ["Reviewed biome wire delegate."],
            },
            {
                "group_id": plan.groups[1].group_id,
                "parent_group_id": plan.groups[1].parent_group_id,
                "source_inspected": True,
                "selected_candidate_ids": ["DISC-NET-R2C-WORLD-DELEGATE-0003"],
                "hazards_reviewed": [],
                "followup_dependencies": [],
                "semantic_observations": ["Reviewed light data delegate."],
            },
        ],
    }
    return worksheet, decisions


class R2cWorldStateDelegateReviewApplyTests(unittest.TestCase):
    def run_apply(self, root: Path, worksheet_bytes: bytes, decisions: dict[str, object]) -> dict[str, object]:
        worksheet = root / "worksheet.json"
        decision_path = root / "decisions.json"
        output = root / "completed.json"
        worksheet.write_bytes(worksheet_bytes)
        decision_path.write_bytes(pretty(decisions))
        summary = apply_review.apply(worksheet, decision_path, output)
        summary["value"] = json.loads(output.read_text())
        summary["text"] = output.read_text()
        return summary

    def test_explicit_selection_rejects_every_omitted_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions = fixture()
            result = self.run_apply(Path(temporary), worksheet, decisions)
            value = result["value"]
            self.assertEqual(result["selected_sources"], 2)
            self.assertEqual(result["rejected_sources"], 2)
            self.assertFalse(result["contains_official_source_text"])
            self.assertNotIn("source_excerpt", result["text"])
            self.assertNotIn("SECRET_", result["text"])
            first = value["groups"][0]
            self.assertTrue(first["source_inspected"])
            self.assertTrue(first["review_complete"])
            self.assertEqual(first["hazards_reviewed"], ["CODEC"])
            self.assertEqual(len(first["selected_source_identities"]), 1)
            self.assertEqual(len(first["rejected_source_identities"]), 1)

    def test_decisions_are_bound_to_exact_generated_worksheet(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions = fixture()
            decisions["generated_worksheet_sha256"] = "0" * 64
            root = Path(temporary)
            worksheet_path = root / "worksheet.json"
            decisions_path = root / "decisions.json"
            worksheet_path.write_bytes(worksheet)
            decisions_path.write_bytes(pretty(decisions))
            with self.assertRaisesRegex(apply_review.ApplyError, "decision provenance mismatch"):
                apply_review.apply(worksheet_path, decisions_path, root / "out.json")

    def test_unknown_selected_candidate_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions = fixture()
            decisions["groups"][0]["selected_candidate_ids"] = [
                "DISC-NET-R2C-WORLD-DELEGATE-9999"
            ]
            root = Path(temporary)
            worksheet_path = root / "worksheet.json"
            decisions_path = root / "decisions.json"
            worksheet_path.write_bytes(worksheet)
            decisions_path.write_bytes(pretty(decisions))
            with self.assertRaisesRegex(apply_review.ApplyError, "selected unknown candidate ids"):
                apply_review.apply(worksheet_path, decisions_path, root / "out.json")

    def test_selected_hazards_must_match_exactly(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions = fixture()
            decisions["groups"][0]["hazards_reviewed"] = []
            root = Path(temporary)
            worksheet_path = root / "worksheet.json"
            decisions_path = root / "decisions.json"
            worksheet_path.write_bytes(worksheet)
            decisions_path.write_bytes(pretty(decisions))
            with self.assertRaisesRegex(apply_review.ApplyError, "hazards_reviewed must exactly match"):
                apply_review.apply(worksheet_path, decisions_path, root / "out.json")

    def test_nonblank_worksheet_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions = fixture()
            value = json.loads(worksheet)
            value["groups"][0]["source_inspected"] = True
            changed = pretty(value)
            decisions["generated_worksheet_sha256"] = hashlib.sha256(changed).hexdigest()
            root = Path(temporary)
            worksheet_path = root / "worksheet.json"
            decisions_path = root / "decisions.json"
            worksheet_path.write_bytes(changed)
            decisions_path.write_bytes(pretty(decisions))
            with self.assertRaisesRegex(apply_review.ApplyError, "already been reviewed"):
                apply_review.apply(worksheet_path, decisions_path, root / "out.json")


if __name__ == "__main__":
    unittest.main()
