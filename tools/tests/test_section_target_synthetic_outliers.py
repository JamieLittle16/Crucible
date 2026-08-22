from __future__ import annotations

import unittest

from tools import section_target_synthetic_evidence as synthetic


def aggregates(
    *,
    control_values: list[int] | None = None,
    replacement_values: dict[str, list[int]] | None = None,
    promotion_values: dict[str, list[int]] | None = None,
) -> dict[str, object]:
    control_values = control_values or [100] * 25
    replacement_values = replacement_values or {}
    promotion_values = promotion_values or {}
    candidates: dict[str, object] = {}
    for candidate in synthetic.CANDIDATES:
        candidates[candidate] = {
            "replacement_p50_ps_per_op": {
                "fixture-replacement": synthetic.aggregate_int(
                    replacement_values.get(candidate, [100] * 5)
                )
            },
            "promotion_p99_ns": {
                "fixture-promotion": synthetic.aggregate_int(
                    promotion_values.get(candidate, [100] * 5)
                )
            },
        }
    return {
        "candidates": candidates,
        "global_control_p50_ps_per_op": synthetic.aggregate_int(control_values),
    }


class SyntheticOutlierGuardTests(unittest.TestCase):
    def test_aggregate_reports_central_and_max_deviation_separately(self) -> None:
        summary = synthetic.aggregate_int([100, 100, 100, 100, 1000])
        self.assertEqual(summary["median"], 100)
        self.assertEqual(summary["mad"], 0)
        self.assertEqual(summary["relative_mad_ppm"], 0)
        self.assertEqual(summary["max_deviation"], 900)
        self.assertEqual(summary["max_relative_deviation_ppm"], 9_000_000)

    def test_single_extreme_replacement_excursion_fails_even_when_mad_is_zero(self) -> None:
        result = synthetic.classify_noise(
            aggregates(replacement_values={"adaptive": [100, 100, 100, 100, 1000]}),
            smoke=False,
            rounds=5,
        )
        self.assertFalse(result["replacement_noise_eligible"])
        self.assertFalse(result["synthetic_evidence_eligible"])
        self.assertTrue(
            any("isolated excursion" in reason for reason in result["reasons"])
        )

    def test_single_extreme_promotion_excursion_fails_even_when_mad_is_zero(self) -> None:
        result = synthetic.classify_noise(
            aggregates(promotion_values={"packed-local": [100, 100, 100, 100, 1000]}),
            smoke=False,
            rounds=5,
        )
        self.assertFalse(result["promotion_tail_noise_eligible"])
        self.assertFalse(result["synthetic_evidence_eligible"])

    def test_single_extreme_control_excursion_fails_even_when_mad_is_zero(self) -> None:
        result = synthetic.classify_noise(
            aggregates(control_values=[100] * 24 + [1000]),
            smoke=False,
            rounds=5,
        )
        self.assertFalse(result["control_noise_eligible"])
        self.assertFalse(result["synthetic_evidence_eligible"])

    def test_looser_outlier_guard_does_not_turn_mad_gate_into_max_gate(self) -> None:
        result = synthetic.classify_noise(
            aggregates(
                control_values=[100] * 24 + [114],
                replacement_values={"adaptive": [100, 100, 100, 100, 125]},
                promotion_values={"packed-local": [100, 100, 100, 100, 140]},
            ),
            smoke=False,
            rounds=5,
        )
        self.assertTrue(result["protocol_eligible"])
        self.assertTrue(result["control_noise_eligible"])
        self.assertTrue(result["replacement_noise_eligible"])
        self.assertTrue(result["promotion_tail_noise_eligible"])
        self.assertTrue(result["synthetic_evidence_eligible"])

    def test_outlier_thresholds_are_exactly_three_times_mad_thresholds(self) -> None:
        result = synthetic.classify_noise(aggregates(), smoke=False, rounds=5)
        thresholds = result["thresholds_ppm"]
        self.assertEqual(
            thresholds["control_max_relative_deviation"],
            thresholds["control_relative_mad"] * synthetic.OUTLIER_GUARD_MULTIPLIER,
        )
        self.assertEqual(
            thresholds["replacement_p50_max_relative_deviation"],
            thresholds["replacement_p50_relative_mad"] * synthetic.OUTLIER_GUARD_MULTIPLIER,
        )
        self.assertEqual(
            thresholds["promotion_p99_max_relative_deviation"],
            thresholds["promotion_p99_relative_mad"] * synthetic.OUTLIER_GUARD_MULTIPLIER,
        )


if __name__ == "__main__":
    unittest.main()
