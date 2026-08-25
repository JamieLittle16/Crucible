from __future__ import annotations

import copy
import unittest
from pathlib import Path

from tools import r2_play_liveness_finalize as finalize
from tools import r2_play_liveness_source_review as review

REPO_ROOT = Path(__file__).resolve().parents[2]
LOCK = REPO_ROOT / "vanilla/vanilla.lock.toml"
SOURCE_SHA = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
FINGERPRINT = "java-token-v2-literal-sensitive"


def reviewed_worksheet() -> dict[str, object]:
    candidates = []
    for candidate in review.CANDIDATES:
        candidates.append(
            {
                "candidate_id": candidate.var_id,
                "source_identity": f"{candidate.type_name}#test()",
                "source": {
                    "type": candidate.type_name,
                    "signature": "test()",
                    "fingerprint_algorithm": FINGERPRINT,
                    "normalized_sha256": "a" * 64,
                    "body_sha256": "b" * 64,
                },
                "atlas_observed_hazards": [],
                "review_focus": list(candidate.review_focus),
                "decision": {
                    "source_inspected": True,
                    "accepted": True,
                    "hazards_reviewed": [],
                    "semantic_rules": list(finalize.EXPECTED_RULES[candidate.var_id]),
                    "followup_dependencies": [],
                    "note": "Exact pinned body reviewed for the bounded R2 liveness frontier.",
                },
            }
        )
    return {
        "schema": 1,
        "kind": review.WORKSHEET_KIND,
        "contains_official_source_text": False,
        "source_archive_sha256": SOURCE_SHA,
        "candidate_count": len(candidates),
        "candidates": candidates,
    }


class R2PlayLivenessFinalizeTests(unittest.TestCase):
    def test_reviewed_frontier_finalizes_static_wire_contract(self) -> None:
        records, contract, generated = finalize.finalize(reviewed_worksheet(), lock_path=LOCK)
        self.assertEqual(len(records), 8)
        self.assertEqual(contract["id"], finalize.CONTRACT_ID)
        packets = contract["packets"]
        self.assertEqual(
            [(packet["direction"], packet["id"]) for packet in packets],
            [("clientbound", 0x2C), ("serverbound", 0x1C)],
        )
        self.assertEqual(packets[0]["golden"]["body_hex"], "2c0102030405060708")
        self.assertEqual(packets[0]["golden"]["frame_hex"], "092c0102030405060708")
        self.assertEqual(packets[1]["golden"]["body_hex"], "1c0102030405060708")
        self.assertEqual(packets[1]["golden"]["frame_hex"], "091c0102030405060708")
        self.assertIn("pub const KEEP_ALIVE: i32 = 44;", generated)
        self.assertIn("pub const KEEP_ALIVE: i32 = 28;", generated)

    def test_source_rich_input_is_rejected(self) -> None:
        worksheet = reviewed_worksheet()
        worksheet["contains_official_source_text"] = True
        with self.assertRaises(finalize.FinalizeError):
            finalize.finalize(worksheet, lock_path=LOCK)

    def test_unreviewed_candidate_is_rejected(self) -> None:
        worksheet = reviewed_worksheet()
        worksheet["candidates"][0]["decision"]["source_inspected"] = False
        with self.assertRaises(finalize.FinalizeError):
            finalize.finalize(worksheet, lock_path=LOCK)

    def test_semantic_rule_drift_is_rejected(self) -> None:
        worksheet = reviewed_worksheet()
        worksheet["candidates"][0]["decision"]["semantic_rules"] = ["SEM-NET-R2-LIVE-009"]
        with self.assertRaises(finalize.FinalizeError):
            finalize.finalize(worksheet, lock_path=LOCK)

    def test_unreviewed_observed_hazard_is_rejected(self) -> None:
        worksheet = reviewed_worksheet()
        worksheet["candidates"][1]["atlas_observed_hazards"] = ["CODEC"]
        with self.assertRaises(finalize.FinalizeError):
            finalize.finalize(worksheet, lock_path=LOCK)

    def test_followup_dependency_keeps_frontier_open(self) -> None:
        worksheet = reviewed_worksheet()
        worksheet["candidates"][2]["decision"]["followup_dependencies"] = ["SomeDelegate#work()"]
        with self.assertRaises(finalize.FinalizeError):
            finalize.finalize(worksheet, lock_path=LOCK)

    def test_input_is_not_mutated(self) -> None:
        worksheet = reviewed_worksheet()
        original = copy.deepcopy(worksheet)
        finalize.finalize(worksheet, lock_path=LOCK)
        self.assertEqual(worksheet, original)


if __name__ == "__main__":
    unittest.main()
