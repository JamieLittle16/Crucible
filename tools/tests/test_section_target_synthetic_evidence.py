from __future__ import annotations

import copy
import unittest

from tools import section_target_synthetic_evidence as synthetic

TARGET = {
    "minecraft_version": "26.2",
    "protocol_version": 776,
    "data_version": 4903,
    "state_count": 32366,
    "state_data_generation_sha256": "a" * 64,
    "state_data_input_sha256": "b" * 64,
}
HEAD = "c" * 40
CPU = 7


def percentile(values: list[int], p: int) -> int:
    ordered = sorted(values)
    index = ((len(ordered) - 1) * p + 99) // 100
    return ordered[index]


def summary(samples: list[int], operations: int) -> dict[str, object]:
    p50 = percentile(samples, 50)
    return {
        "operations_per_sample": operations,
        "p50_ns": p50,
        "p95_ns": percentile(samples, 95),
        "p99_ns": percentile(samples, 99),
        "max_ns": max(samples),
        "p50_ps_per_op": p50 * 1000 // operations,
        "samples_ns": samples,
    }


def child_record(candidate: str = "adaptive", mode: str = "smoke") -> dict[str, object]:
    settings = synthetic.expected_settings(mode)
    measured_samples = settings["measured_samples"]
    replacement_samples = [1000 + index for index in range(measured_samples)]
    promotion_samples = [100 + index for index in range(settings["promotion_samples"])]
    control_samples = [2000 + index for index in range(measured_samples)]
    timings: list[dict[str, object]] = []
    for pattern, cardinality in synthetic.expected_cases(mode):
        for workload in synthetic.REPLACEMENTS:
            timings.append(
                {
                    "workload": workload,
                    "pattern": pattern,
                    "pool_cardinality": cardinality,
                    "actual_cardinality": max(1, min(cardinality, 17)),
                    "representation": "uniform->local8-stable",
                    "unit": "replace",
                    "timing": summary(replacement_samples, settings["mutations"]),
                }
            )
    for target in synthetic.PROMOTION_TARGETS:
        timings.append(
            {
                "workload": f"promotion-to-{target}",
                "pattern": "promotion-boundary",
                "pool_cardinality": target,
                "actual_cardinality": target,
                "representation": "uniform->local8-stable",
                "unit": "single-replace",
                "timing": summary(promotion_samples, 1),
            }
        )
    return {
        "schema": synthetic.SCHEMA,
        "harness_version": synthetic.HARNESS_VERSION,
        "mode": mode,
        "candidate": candidate,
        "production_candidate": candidate != "direct-reference",
        "scope": synthetic.SCOPE,
        "build_profile": synthetic.BUILD_PROFILE,
        "codegen_policy": synthetic.CODEGEN_POLICY,
        **TARGET,
        "commit_sha": HEAD,
        "target_triple": "x86_64-unknown-linux-gnu",
        "cpu_model": "fixture",
        "kernel": "fixture",
        "cpus_allowed_list": str(CPU),
        "mems_allowed_list": "0",
        "load_average": "0 0 0",
        "rustflags": "",
        "cargo_encoded_rustflags": "",
        "rustc_verbose": "fixture",
        "cpu0_governor": "performance",
        "cpu0_current_khz": "1",
        "cpu0_min_khz": "1",
        "cpu0_max_khz": "1",
        "intel_pstate_no_turbo": "1",
        "settings": settings,
        "promotion_targets": list(synthetic.PROMOTION_TARGETS),
        "control": {
            "workload": synthetic.CONTROL_WORKLOAD,
            "unit": "iteration",
            "timing": summary(control_samples, settings["control_operations"]),
        },
        "timings": timings,
    }


def expectation(candidate: str = "adaptive", mode: str = "smoke") -> synthetic.ChildExpectation:
    return synthetic.ChildExpectation(
        candidate=candidate,
        mode=mode,
        head_sha=HEAD,
        cpu=CPU,
        target=TARGET,
    )


def repeated_child(candidate: str, round_index: int, scale: int = 1000) -> dict[str, object]:
    return {
        "round": round_index,
        "candidate": candidate,
        "control_p50_ps_per_op": 1000 + round_index,
        "replacement_p50_ps_per_op": {
            "palette-churn|cardinality-spread|17|17|local8-stable->local8-stable": scale + round_index,
            "same-state-replace|homogeneous|1|1|uniform->uniform": scale * 2 + round_index,
        },
        "promotion_p99_ns": {
            "promotion-to-17|local4-stable->local8-stable": scale * 3 + round_index,
            "promotion-to-257|local8-stable->direct-n": scale * 4 + round_index,
        },
    }


class ScheduleTests(unittest.TestCase):
    def test_five_round_schedule_balances_every_candidate_position(self) -> None:
        schedule = synthetic.schedule(5)
        self.assertEqual(len(schedule), 25)
        for candidate in synthetic.CANDIDATES:
            positions = sorted(
                item.candidate_position for item in schedule if item.candidate == candidate
            )
            self.assertEqual(positions, [0, 1, 2, 3, 4])

    def test_invalid_round_count_fails_closed(self) -> None:
        with self.assertRaises(synthetic.SyntheticEvidenceError):
            synthetic.schedule(0)


class ChildValidationTests(unittest.TestCase):
    def test_smoke_and_qualification_records_validate(self) -> None:
        synthetic.validate_child(child_record(mode="smoke"), expectation(mode="smoke"))
        synthetic.validate_child(
            child_record(mode="qualification"), expectation(mode="qualification")
        )

    def test_identity_tampering_is_rejected(self) -> None:
        mutations = [
            ("schema", 2),
            ("harness_version", "other"),
            ("scope", "other"),
            ("candidate", "packed-local"),
            ("production_candidate", False),
            ("build_profile", "debug"),
            ("codegen_policy", "other"),
            ("commit_sha", "d" * 40),
            ("cpus_allowed_list", "0-7"),
            ("mems_allowed_list", ""),
            ("minecraft_version", "26.3"),
        ]
        for field, value in mutations:
            with self.subTest(field=field):
                record = child_record()
                record[field] = value
                with self.assertRaises(synthetic.SyntheticEvidenceError):
                    synthetic.validate_child(record, expectation())

    def test_settings_and_promotion_target_drift_are_rejected(self) -> None:
        record = child_record()
        record["settings"] = dict(record["settings"])
        record["settings"]["mutations"] += 1
        with self.assertRaises(synthetic.SyntheticEvidenceError):
            synthetic.validate_child(record, expectation())

        record = child_record()
        record["promotion_targets"] = list(record["promotion_targets"][:-1])
        with self.assertRaises(synthetic.SyntheticEvidenceError):
            synthetic.validate_child(record, expectation())

    def test_raw_summary_is_recomputed_not_trusted(self) -> None:
        record = child_record()
        record["control"] = copy.deepcopy(record["control"])
        record["control"]["timing"]["p99_ns"] += 1
        with self.assertRaises(synthetic.SyntheticEvidenceError):
            synthetic.validate_child(record, expectation())

    def test_missing_duplicate_and_unknown_workloads_are_rejected(self) -> None:
        for transform in ("missing", "duplicate", "unknown"):
            with self.subTest(transform=transform):
                record = child_record()
                timings = list(record["timings"])
                if transform == "missing":
                    timings.pop(0)
                elif transform == "duplicate":
                    timings[-1] = copy.deepcopy(timings[0])
                else:
                    timings[0] = dict(timings[0])
                    timings[0]["workload"] = "invented-workload"
                record["timings"] = timings
                with self.assertRaises(synthetic.SyntheticEvidenceError):
                    synthetic.validate_child(record, expectation())

    def test_cardinality_and_units_are_fail_closed(self) -> None:
        record = child_record()
        record["timings"] = copy.deepcopy(record["timings"])
        record["timings"][0]["actual_cardinality"] = 999
        with self.assertRaises(synthetic.SyntheticEvidenceError):
            synthetic.validate_child(record, expectation())

        record = child_record()
        record["timings"] = copy.deepcopy(record["timings"])
        promotion = next(
            timing for timing in record["timings"] if timing["workload"] == "promotion-to-17"
        )
        promotion["unit"] = "replace"
        with self.assertRaises(synthetic.SyntheticEvidenceError):
            synthetic.validate_child(record, expectation())

    def test_child_summary_retains_representation_and_observed_cardinality(self) -> None:
        record = child_record(mode="qualification")
        synthetic.validate_child(record, expectation(mode="qualification"))
        normalized = synthetic.child_summary(record)
        keys = set(normalized["replacement_p50_ps_per_op"])
        self.assertTrue(any("|17|17|uniform->local8-stable" in key for key in keys))
        promotion_keys = set(normalized["promotion_p99_ns"])
        self.assertIn(
            "promotion-to-17|uniform->local8-stable",
            promotion_keys,
        )


class NoiseQualificationTests(unittest.TestCase):
    def stable_children(self, rounds: int = 5) -> list[dict[str, object]]:
        return [
            repeated_child(candidate, round_index, 1000 + candidate_index * 100)
            for round_index in range(rounds)
            for candidate_index, candidate in enumerate(synthetic.CANDIDATES)
        ]

    def test_stable_five_round_evidence_is_eligible(self) -> None:
        aggregates = synthetic.aggregate_children(self.stable_children())
        result = synthetic.classify_noise(aggregates, smoke=False, rounds=5)
        self.assertTrue(result["protocol_eligible"])
        self.assertTrue(result["control_noise_eligible"])
        self.assertTrue(result["replacement_noise_eligible"])
        self.assertTrue(result["promotion_tail_noise_eligible"])
        self.assertTrue(result["synthetic_evidence_eligible"])

    def test_smoke_is_never_eligible(self) -> None:
        aggregates = synthetic.aggregate_children(self.stable_children(rounds=1))
        result = synthetic.classify_noise(aggregates, smoke=True, rounds=1)
        self.assertFalse(result["protocol_eligible"])
        self.assertFalse(result["synthetic_evidence_eligible"])

    def test_replacement_drift_downgrades_evidence(self) -> None:
        children = self.stable_children()
        target = next(
            child for child in children if child["candidate"] == "adaptive" and child["round"] == 4
        )
        key = next(iter(target["replacement_p50_ps_per_op"]))
        target["replacement_p50_ps_per_op"][key] *= 10
        result = synthetic.classify_noise(
            synthetic.aggregate_children(children), smoke=False, rounds=5
        )
        self.assertFalse(result["replacement_noise_eligible"])
        self.assertFalse(result["synthetic_evidence_eligible"])

    def test_promotion_p99_drift_downgrades_evidence(self) -> None:
        children = self.stable_children()
        target = next(
            child
            for child in children
            if child["candidate"] == "packed-local" and child["round"] == 4
        )
        key = next(iter(target["promotion_p99_ns"]))
        target["promotion_p99_ns"][key] *= 10
        result = synthetic.classify_noise(
            synthetic.aggregate_children(children), smoke=False, rounds=5
        )
        self.assertFalse(result["promotion_tail_noise_eligible"])
        self.assertFalse(result["synthetic_evidence_eligible"])

    def test_control_drift_downgrades_evidence(self) -> None:
        children = self.stable_children()
        children[-1]["control_p50_ps_per_op"] *= 10
        result = synthetic.classify_noise(
            synthetic.aggregate_children(children), smoke=False, rounds=5
        )
        self.assertFalse(result["control_noise_eligible"])
        self.assertFalse(result["synthetic_evidence_eligible"])

    def test_reference_only_mechanism_instability_does_not_block_production(self) -> None:
        children = self.stable_children()
        reference = [child for child in children if child["candidate"] == "direct-reference"]
        key = next(iter(reference[-1]["promotion_p99_ns"]))
        reference[-1]["promotion_p99_ns"][key] *= 100
        aggregates = synthetic.aggregate_children(children)
        result = synthetic.classify_noise(aggregates, smoke=False, rounds=5)
        self.assertTrue(result["promotion_tail_noise_eligible"])
        self.assertTrue(result["synthetic_evidence_eligible"])

    def test_record_set_drift_across_rounds_is_rejected(self) -> None:
        children = self.stable_children()
        target = next(child for child in children if child["candidate"] == "direct" and child["round"] == 4)
        target["replacement_p50_ps_per_op"].pop(next(iter(target["replacement_p50_ps_per_op"])))
        with self.assertRaises(synthetic.SyntheticEvidenceError):
            synthetic.aggregate_children(children)


if __name__ == "__main__":
    unittest.main()
