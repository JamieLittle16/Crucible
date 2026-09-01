from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_delegate_closure_source_review as closure
from tools import r2c_world_state_delegate_review_apply as apply_review
from tools import r2c_world_state_delegate_review_complete as complete_review


def pretty(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def record(candidate_id: str, identity: str, group_id: str, focus: str) -> dict[str, object]:
    owner, signature = identity.split("#", 1)
    excerpt = f"SECRET_SOURCE_{candidate_id}\n"
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
        "atlas_observed_hazards": [],
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


def fixture(root: Path) -> tuple[Path, Path, Path, Path, Path]:
    plan = closure._load_plan()
    records = [
        record(
            "DISC-NET-R2C-WORLD-DELEGATE-0001",
            "net.minecraft.Biome#wire()",
            plan.groups[0].group_id,
            plan.groups[0].review_focus,
        ),
        record(
            "DISC-NET-R2C-WORLD-DELEGATE-0002",
            "net.minecraft.Light#data()",
            plan.groups[1].group_id,
            plan.groups[1].review_focus,
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
    worksheet_value = json.loads(payloads["worksheet.json"])
    decisions = {
        "schema": 1,
        "kind": apply_review.DECISION_KIND,
        "review_id": closure.PLAN_ID,
        "commit_policy": apply_review.DECISION_COMMIT_POLICY,
        "contains_official_source_text": False,
        "production_admitted": False,
        "source_archive_sha256": closure.EXPECTED_SOURCE_SHA256,
        "plan_sha256": apply_review._sha256_file(closure.DEFAULT_PLAN),
        "generated_worksheet_sha256": hashlib.sha256(payloads["worksheet.json"]).hexdigest(),
        "review_pack_sha256": worksheet_value["review_pack_sha256"],
        "automatic_source_selection_forbidden": True,
        "unselected_candidate_policy": apply_review.UNSELECTED_POLICY,
        "groups": [
            {
                "group_id": plan.groups[0].group_id,
                "parent_group_id": plan.groups[0].parent_group_id,
                "source_inspected": True,
                "selected_candidate_ids": ["DISC-NET-R2C-WORLD-DELEGATE-0001"],
                "hazards_reviewed": [],
                "followup_dependencies": [],
                "semantic_observations": ["Reviewed biome delegate."],
            },
            {
                "group_id": plan.groups[1].group_id,
                "parent_group_id": plan.groups[1].parent_group_id,
                "source_inspected": True,
                "selected_candidate_ids": ["DISC-NET-R2C-WORLD-DELEGATE-0002"],
                "hazards_reviewed": [],
                "followup_dependencies": [],
                "semantic_observations": ["Reviewed light delegate."],
            },
        ],
    }
    pack = root / "review-pack.json"
    worksheet = root / "worksheet.json"
    manifest = root / "manifest.json"
    decision_path = root / "decisions.json"
    bundle = root / "delegate-review.tar.gz"
    pack.write_bytes(payloads["review-pack.json"])
    worksheet.write_bytes(payloads["worksheet.json"])
    manifest.write_bytes(payloads["manifest.json"])
    decision_path.write_bytes(pretty(decisions))
    closure._write_archive(bundle, payloads)
    return pack, worksheet, manifest, decision_path, bundle


class R2cWorldStateDelegateReviewCompleteTests(unittest.TestCase):
    def test_completion_publishes_only_source_free_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack, worksheet, manifest, decisions, _bundle = fixture(root)
            output = root / "completed-review"
            summary = complete_review.complete(pack, worksheet, manifest, decisions, output)

            self.assertEqual(summary["selected_sources"], 2)
            self.assertEqual(summary["rejected_sources"], 0)
            self.assertFalse(summary["contains_official_source_text"])
            self.assertTrue((output / complete_review.COMPLETED_WORKSHEET).is_file())
            self.assertTrue((output / complete_review.REVIEW_RESULT).is_file())
            self.assertEqual(
                sorted(path.name for path in output.iterdir()),
                sorted([complete_review.COMPLETED_WORKSHEET, complete_review.REVIEW_RESULT]),
            )
            combined = "\n".join(path.read_text() for path in output.iterdir())
            self.assertNotIn("source_excerpt", combined)
            self.assertNotIn("SECRET_SOURCE", combined)

    def test_bundle_completion_extracts_ephemerally_and_publishes_source_free_only(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            _pack, _worksheet, _manifest, decisions, bundle = fixture(root)
            output = root / "completed-review"
            summary = complete_review.complete_bundle(bundle, decisions, output)

            self.assertEqual(summary["selected_sources"], 2)
            self.assertFalse(summary["contains_official_source_text"])
            self.assertEqual(
                sorted(path.name for path in output.iterdir()),
                sorted([complete_review.COMPLETED_WORKSHEET, complete_review.REVIEW_RESULT]),
            )
            combined = "\n".join(path.read_text() for path in output.iterdir())
            self.assertNotIn("source_excerpt", combined)
            self.assertNotIn("SECRET_SOURCE", combined)
            self.assertFalse(any(path.name.startswith(".r2c-delegate-bundle-") for path in root.iterdir()))

    def test_bundle_with_extra_member_is_rejected_without_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack, worksheet, manifest, decisions, _bundle = fixture(root)
            invalid = root / "invalid.tar.gz"
            closure._write_archive(
                invalid,
                {
                    "review-pack.json": pack.read_bytes(),
                    "worksheet.json": worksheet.read_bytes(),
                    "manifest.json": manifest.read_bytes(),
                    "unexpected.json": b"{}\n",
                },
            )
            output = root / "completed-review"

            with self.assertRaisesRegex(complete_review.CompleteError, "contain exactly"):
                complete_review.complete_bundle(invalid, decisions, output)
            self.assertFalse(output.exists())
            self.assertFalse(any(path.name.startswith(".r2c-delegate-bundle-") for path in root.iterdir()))

    def test_failed_finalization_leaves_no_partial_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack, worksheet, manifest, decisions, _bundle = fixture(root)
            manifest_value = json.loads(manifest.read_text())
            manifest_value["files"][0]["sha256"] = "0" * 64
            manifest.write_bytes(pretty(manifest_value))
            output = root / "completed-review"

            with self.assertRaisesRegex(complete_review.CompleteError, "manifest metadata mismatch"):
                complete_review.complete(pack, worksheet, manifest, decisions, output)
            self.assertFalse(output.exists())
            self.assertFalse(any(path.name.startswith(".r2c-delegate-review-") for path in root.iterdir()))

    def test_existing_output_directory_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack, worksheet, manifest, decisions, _bundle = fixture(root)
            output = root / "completed-review"
            output.mkdir()
            with self.assertRaisesRegex(complete_review.CompleteError, "must not already exist"):
                complete_review.complete(pack, worksheet, manifest, decisions, output)


if __name__ == "__main__":
    unittest.main()
