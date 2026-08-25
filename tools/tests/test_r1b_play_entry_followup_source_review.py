from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import r1b_play_entry_followup_source_review as review


class R1BPlayEntryFollowupSourceReviewTests(unittest.TestCase):
    def test_plan_is_bounded_and_effective_selectors_are_unique(self) -> None:
        ids = [candidate.var_id for candidate in review.CANDIDATES]
        selectors = [review.selector_key(candidate) for candidate in review.CANDIDATES]
        self.assertEqual(len(ids), 35)
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(len(selectors), len(set(selectors)))

    def test_plan_contains_only_material_second_pass_surface(self) -> None:
        ids = {candidate.var_id for candidate in review.CANDIDATES}
        required = {
            "DISC-NET-R1B-PLAY-FOLLOWUP-COMMANDS-SEND-001",
            "DISC-NET-R1B-PLAY-FOLLOWUP-RECIPE-ADD-CODEC-001",
            "DISC-NET-R1B-PLAY-FOLLOWUP-CLOCK-FULL-SYNC-001",
            "DISC-NET-R1B-PLAY-FOLLOWUP-ACTIVE-EFFECTS-001",
            "DISC-NET-R1B-PLAY-FOLLOWUP-INIT-MENU-001",
            "DISC-NET-R1B-PLAY-FOLLOWUP-CREATE-SPAWN-INFO-001",
            "DISC-NET-R1B-PLAY-FOLLOWUP-PLAYER-INFO-ACTIONS-001",
            "DISC-NET-R1B-PLAY-FOLLOWUP-RELATIVE-CODEC-001",
            "DISC-NET-R1B-PLAY-FOLLOWUP-BROADCAST-ALL-001",
        }
        self.assertTrue(required <= ids)
        self.assertFalse(any("SCOREBOARD" in candidate_id for candidate_id in ids))
        self.assertFalse(any("UPDATE-MOB-EFFECT" in candidate_id for candidate_id in ids))

    def test_ambiguous_or_semantically_selected_overloads_are_exact(self) -> None:
        candidates = {candidate.var_id: candidate for candidate in review.CANDIDATES}
        expected = {
            "DISC-NET-R1B-PLAY-FOLLOWUP-COMMANDS-SEND-001":
                "sendCommands(final ServerPlayer player)",
            "DISC-NET-R1B-PLAY-FOLLOWUP-BORDER-FROM-STATE-001":
                "ClientboundInitializeBorderPacket(final WorldBorder border)",
            "DISC-NET-R1B-PLAY-FOLLOWUP-ACTIVE-EFFECTS-001":
                "sendActiveEffects(final LivingEntity livingEntity , final ServerGamePacketListenerImpl connection)",
            "DISC-NET-R1B-PLAY-FOLLOWUP-SPAWN-INFO-WRITE-001":
                "write(final RegistryFriendlyByteBuf output)",
            "DISC-NET-R1B-PLAY-FOLLOWUP-BROADCAST-ALL-001":
                "broadcastAll(final Packet < ? > packet)",
        }
        for candidate_id, signature in expected.items():
            self.assertEqual(candidates[candidate_id].exact_signature, signature)

    def test_plan_identity_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "plan.json"
            plan = json.loads(review.DEFAULT_PLAN.read_text(encoding="utf-8"))
            plan["id"] = "WRONG"
            path.write_text(json.dumps(plan), encoding="utf-8")
            with self.assertRaises(review.FollowupReviewError):
                review.load_plan(path)

    def test_source_rich_output_is_rejected_inside_repository(self) -> None:
        with tempfile.TemporaryDirectory(dir=review.REPO_ROOT) as temporary:
            with self.assertRaises(review.FollowupReviewError):
                review._external_fresh_dir(Path(temporary) / "followup")

    def test_disposition_vocabulary_matches_first_pass(self) -> None:
        self.assertEqual(
            set(review.ROUTE_DISPOSITIONS),
            {
                "MANDATORY",
                "CONDITIONAL",
                "DEFAULT_EMPTY",
                "INTERNAL_ONLY",
                "DELEGATED_REVIEW_REQUIRED",
            },
        )


if __name__ == "__main__":
    unittest.main()
