from __future__ import annotations

import unittest

from tools import section_target_hardware as target


def aggregate_fixture(
    *,
    control_values: list[int] | None = None,
    workload_values: dict[tuple[str, str, str], list[int]] | None = None,
    rss_values: dict[tuple[str, str], list[int]] | None = None,
) -> dict[str, object]:
    workload_values = workload_values or {}
    rss_values = rss_values or {}
    dimensions: dict[str, object] = {}
    for dimension in target.DIMENSIONS:
        candidates: dict[str, object] = {}
        for candidate in target.CANDIDATES:
            candidates[candidate] = {
                "workloads_p50_ps_per_op": {
                    workload: target.aggregate_int(
                        workload_values.get((dimension, candidate, workload), [100] * 5)
                    )
                    for workload in target.WORKLOADS
                },
                "rss_loaded_delta_kib": target.aggregate_int(
                    rss_values.get((dimension, candidate), [100] * 5)
                ),
                "construction_p99_ns": target.aggregate_int([100] * 5),
            }
        dimensions[dimension] = {"candidates": candidates}
    return {
        "dimensions": dimensions,
        "global_control_p50_ps_per_op": target.aggregate_int(
            control_values or [100] * (5 * len(target.DIMENSIONS) * len(target.CANDIDATES))
        ),
    }


class PopulationOutlierGuardTests(unittest.TestCase):
    def test_aggregate_reports_central_and_max_deviation_separately(self) -> None:
        summary = target.aggregate_int([100, 100, 100, 100, 1000])
        self.assertEqual(summary["median"], 100)
        self.assertEqual(summary["mad"], 0)
        self.assertEqual(summary["relative_mad_ppm"], 0)
        self.assertEqual(summary["max_deviation"], 900)
        self.assertEqual(summary["max_relative_deviation_ppm"], 9_000_000)

    def test_single_extreme_workload_excursion_fails_even_when_mad_is_zero(self) -> None:
        key = (target.DIMENSIONS[0], "adaptive", target.STEADY_WORKLOADS[0])
        result = target.classify_noise(
            aggregate_fixture(workload_values={key: [100, 100, 100, 100, 1000]}),
            smoke=False,
            rounds=5,
        )
        self.assertFalse(result["workload_noise_eligible"])
        self.assertFalse(result["population_evidence_eligible"])
        self.assertTrue(any("isolated excursion" in reason for reason in result["reasons"]))

    def test_single_extreme_rss_excursion_fails_even_when_mad_is_zero(self) -> None:
        key = (target.DIMENSIONS[1], "packed-local")
        result = target.classify_noise(
            aggregate_fixture(rss_values={key: [100, 100, 100, 100, 1000]}),
            smoke=False,
            rounds=5,
        )
        self.assertFalse(result["rss_noise_eligible"])
        self.assertFalse(result["population_evidence_eligible"])

    def test_single_extreme_control_excursion_fails_even_when_mad_is_zero(self) -> None:
        controls = [100] * (5 * len(target.DIMENSIONS) * len(target.CANDIDATES))
        controls[-1] = 1000
        result = target.classify_noise(
            aggregate_fixture(control_values=controls), smoke=False, rounds=5
        )
        self.assertFalse(result["control_noise_eligible"])
        self.assertFalse(result["population_evidence_eligible"])

    def test_looser_excursion_guard_does_not_become_no_outlier_rule(self) -> None:
        workload_key = (target.DIMENSIONS[0], "fast-local", target.STEADY_WORKLOADS[0])
        rss_key = (target.DIMENSIONS[2], "adaptive")
        controls = [100] * (5 * len(target.DIMENSIONS) * len(target.CANDIDATES))
        controls[-1] = 114
        result = target.classify_noise(
            aggregate_fixture(
                control_values=controls,
                workload_values={workload_key: [100, 100, 100, 100, 125]},
                rss_values={rss_key: [100, 100, 100, 100, 125]},
            ),
            smoke=False,
            rounds=5,
        )
        self.assertTrue(result["control_noise_eligible"])
        self.assertTrue(result["workload_noise_eligible"])
        self.assertTrue(result["rss_noise_eligible"])
        self.assertTrue(result["population_evidence_eligible"])

    def test_outlier_thresholds_are_exactly_three_times_mad_thresholds(self) -> None:
        result = target.classify_noise(aggregate_fixture(), smoke=False, rounds=5)
        thresholds = result["thresholds_ppm"]
        self.assertEqual(
            thresholds["control_max_relative_deviation"],
            thresholds["control_relative_mad"] * target.OUTLIER_GUARD_MULTIPLIER,
        )
        self.assertEqual(
            thresholds["workload_max_relative_deviation"],
            thresholds["workload_relative_mad"] * target.OUTLIER_GUARD_MULTIPLIER,
        )
        self.assertEqual(
            thresholds["rss_max_relative_deviation"],
            thresholds["rss_relative_mad"] * target.OUTLIER_GUARD_MULTIPLIER,
        )

    def test_reference_only_rss_excursion_still_does_not_veto_production(self) -> None:
        key = (target.DIMENSIONS[0], "direct-reference")
        result = target.classify_noise(
            aggregate_fixture(rss_values={key: [100, 100, 100, 100, 10_000]}),
            smoke=False,
            rounds=5,
        )
        self.assertTrue(result["rss_noise_eligible"])
        self.assertTrue(result["population_evidence_eligible"])


if __name__ == "__main__":
    unittest.main()
