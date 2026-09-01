from __future__ import annotations

import unittest

from tools import r2c_world_state_admission_prepare as prepare


class R2cWorldStateAdmissionDelegateVarIdTests(unittest.TestCase):
    def test_primary_candidate_mapping_is_unchanged(self) -> None:
        self.assertEqual(
            prepare._var_id("DISC-NET-R2C-WORLD-0203"),
            "VAR-NET-R2C-WORLD-0203",
        )

    def test_delegate_candidate_has_disjoint_stable_var_namespace(self) -> None:
        self.assertEqual(
            prepare._var_id("DISC-NET-R2C-WORLD-DELEGATE-0001"),
            "VAR-NET-R2C-WORLD-DELEGATE-0001",
        )
        self.assertNotEqual(
            prepare._var_id("DISC-NET-R2C-WORLD-DELEGATE-0001"),
            prepare._var_id("DISC-NET-R2C-WORLD-0001"),
        )

    def test_delegate_suffix_is_exactly_four_decimal_digits(self) -> None:
        for candidate_id in (
            "DISC-NET-R2C-WORLD-DELEGATE-1",
            "DISC-NET-R2C-WORLD-DELEGATE-00001",
            "DISC-NET-R2C-WORLD-DELEGATE-00A1",
            "DISC-NET-R2C-WORLD-DELEGATE-",
        ):
            with self.subTest(candidate_id=candidate_id), self.assertRaisesRegex(
                prepare.PrepareError, "delegate candidate id has non-canonical suffix"
            ):
                prepare._var_id(candidate_id)

    def test_unrelated_candidate_namespace_still_fails_closed(self) -> None:
        with self.assertRaisesRegex(prepare.PrepareError, "cannot derive stable VAR id"):
            prepare._var_id("DISC-NET-R2B-WORLD-DELEGATE-0001")


if __name__ == "__main__":
    unittest.main()
