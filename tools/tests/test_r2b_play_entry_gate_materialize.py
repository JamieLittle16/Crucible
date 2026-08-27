from __future__ import annotations

import unittest

from tools import r1b_play_entry_followup_source_review as followup
from tools import r1b_play_entry_source_review as first_pass
from tools import r2b_play_entry_gate_materialize as materialize


class R2BPlayEntryGateMaterializeTests(unittest.TestCase):
    def test_every_historical_67_plan_candidate_has_explicit_sem_mapping(self) -> None:
        ids = [candidate.var_id for candidate in first_pass.CANDIDATES]
        ids.extend(candidate.var_id for candidate in followup.CANDIDATES)
        ids.extend(
            [
                "DISC-NET-R2B-PLAY-INVENTORY-SET-SYNCHRONIZER-001",
                "DISC-NET-R2B-PLAY-INVENTORY-SEND-ALL-001",
                "DISC-NET-R2B-PLAY-INVENTORY-SYNCHRONIZER-FIELD-001",
                "DISC-NET-R2B-PLAY-INVENTORY-SYNCHRONIZER-CONTRACT-001",
                "DISC-NET-R2B-PLAY-INVENTORY-MENU-CONSTRUCTOR-001",
            ]
        )
        for candidate_id in ids:
            with self.subTest(candidate_id=candidate_id):
                rules = materialize._rules_for_67(candidate_id)
                self.assertTrue(rules)
                self.assertTrue(all(rule.startswith("SEM-NET-R2B-PLAY-") for rule in rules))

    def test_unknown_67_candidate_fails_closed(self) -> None:
        with self.assertRaisesRegex(materialize.MaterializeError, "no explicit SEM mapping"):
            materialize._rules_for_67("DISC-NET-R1B-PLAY-MYSTERY-001")

    def test_wire_groups_cover_exact_reviewed_family_set(self) -> None:
        self.assertEqual(
            set(materialize.REVIEW_RULES_117),
            {
                "COMMAND_TREE",
                "CLOCK_FULL_SYNC",
                "RECIPE_BOOK_SETTINGS",
                "SYNCHRONIZED_RECIPES",
                "DIMENSION_TYPE",
                "DEFAULT_SPAWN",
                "INITIAL_INVENTORY",
            },
        )
        for group, rules in materialize.REVIEW_RULES_117.items():
            with self.subTest(group=group):
                self.assertTrue(rules)

    def test_final_seam_groups_are_only_the_frozen_two(self) -> None:
        self.assertEqual(
            set(materialize.FINAL_SEAM_RULES),
            {"GENERIC_REGISTRY_WIRE", "GLOBAL_POS_WIRE"},
        )

    def test_candidate_id_normalization_is_deterministic(self) -> None:
        self.assertEqual(
            materialize._var_id("DISC-NET-R1B-PLAY-LOGIN-CODEC-001"),
            "VAR-NET-R2B-PLAY-LOGIN-CODEC-001",
        )
        self.assertEqual(
            materialize._var_id("DISC-NET-R1B-PLAY-FOLLOWUP-CLOCK-FULL-SYNC-001"),
            "VAR-NET-R2B-PLAY-FOLLOWUP-CLOCK-FULL-SYNC-001",
        )
        self.assertEqual(
            materialize._var_id("DISC-NET-R2B-PLAY-FINAL-SEAM-001"),
            "VAR-NET-R2B-PLAY-FINAL-SEAM-001",
        )

    def test_unknown_candidate_prefix_fails_closed(self) -> None:
        with self.assertRaisesRegex(materialize.MaterializeError, "cannot canonicalize"):
            materialize._var_id("UNKNOWN-001")


if __name__ == "__main__":
    unittest.main()
