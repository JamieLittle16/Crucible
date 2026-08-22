#!/usr/bin/env python3
"""Validate and aggregate candidate-isolated synthetic section evidence."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Iterable

SCHEMA = 1
HARNESS_VERSION = "section-target-synthetic-bench-v1"
SCOPE = "synthetic-mechanism-stress"
BUILD_PROFILE = "release"
CODEGEN_POLICY = "lto=thin,codegen-units=1,panic=abort,strip=debuginfo"
CANDIDATES = (
    "direct-reference",
    "direct",
    "adaptive",
    "fast-local",
    "packed-local",
)
PRODUCTION_CANDIDATES = CANDIDATES[1:]
REPLACEMENTS = (
    "same-state-replace",
    "low-entropy-replace",
    "high-entropy-replace",
    "palette-churn",
)
PROMOTION_TARGETS = (2, 3, 5, 9, 17, 33, 65, 129, 257)
PROMOTIONS = tuple(f"promotion-to-{target}" for target in PROMOTION_TARGETS)
CONTROL_WORKLOAD = "control-integer-loop"

CARDINALITIES = (1, 2, 4, 8, 16, 17, 32, 64, 128, 256, 257, 1024, 4096)
QUALIFICATION_CASES = tuple(("cardinality-spread", value) for value in CARDINALITIES) + (
    ("homogeneous", 1),
    ("layered", 8),
    ("clustered", 16),
    ("checker", 2),
    ("noisy", 257),
    ("noisy", 4096),
    ("fluid-containing", 8),
    ("survival-like", 32),
    ("build-like", 64),
)
SMOKE_CASES = tuple(
    ("cardinality-spread", value) for value in (1, 16, 17, 256, 257, 4096)
) + (
    ("layered", 8),
    ("fluid-containing", 8),
    ("survival-like", 32),
    ("noisy", 257),
)

CONTROL_MAX_RELATIVE_MAD_PPM = 50_000
REPLACEMENT_MAX_RELATIVE_MAD_PPM = 100_000
PROMOTION_P99_MAX_RELATIVE_MAD_PPM = 150_000


class SyntheticEvidenceError(RuntimeError):
    """Raised when candidate-isolated synthetic evidence is not trustworthy."""


@dataclass(frozen=True)
class ScheduledSyntheticChild:
    round_index: int
    candidate_position: int
    candidate: str


@dataclass(frozen=True)
class ChildExpectation:
    candidate: str
    mode: str
    head_sha: str
    cpu: int
    target: dict[str, object]


def rotate(values: tuple[str, ...], offset: int) -> tuple[str, ...]:
    if not values:
        return values
    offset %= len(values)
    return values[offset:] + values[:offset]


def schedule(rounds: int) -> list[ScheduledSyntheticChild]:
    if rounds <= 0:
        raise SyntheticEvidenceError("round count must be positive")
    children: list[ScheduledSyntheticChild] = []
    for round_index in range(rounds):
        for candidate_position, candidate in enumerate(rotate(CANDIDATES, round_index)):
            children.append(
                ScheduledSyntheticChild(
                    round_index=round_index,
                    candidate_position=candidate_position,
                    candidate=candidate,
                )
            )
    return children


def expected_settings(mode: str) -> dict[str, int]:
    if mode == "smoke":
        return {
            "warmup_samples": 1,
            "measured_samples": 3,
            "mutations": 2_048,
            "promotion_samples": 8,
            "control_operations": 20_000,
            "case_count": 10,
        }
    if mode == "qualification":
        return {
            "warmup_samples": 5,
            "measured_samples": 25,
            "mutations": 32_768,
            "promotion_samples": 1_000,
            "control_operations": 1_000_000,
            "case_count": 22,
        }
    raise SyntheticEvidenceError(f"unsupported synthetic mode: {mode!r}")


def expected_cases(mode: str) -> tuple[tuple[str, int], ...]:
    if mode == "smoke":
        return SMOKE_CASES
    if mode == "qualification":
        return QUALIFICATION_CASES
    raise SyntheticEvidenceError(f"unsupported synthetic mode: {mode!r}")


def _integer(value: object, label: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise SyntheticEvidenceError(f"{label} must be an integer")
    return value


def _percentile(sorted_values: list[int], percentile: int) -> int:
    if not sorted_values:
        raise SyntheticEvidenceError("cannot summarize empty timing samples")
    index = ((len(sorted_values) - 1) * percentile + 99) // 100
    return sorted_values[index]


def validate_summary(
    summary: object,
    *,
    expected_samples: int,
    expected_operations: int,
    label: str,
) -> dict[str, Any]:
    if not isinstance(summary, dict):
        raise SyntheticEvidenceError(f"{label} must be an object")
    samples = summary.get("samples_ns")
    if not isinstance(samples, list) or len(samples) != expected_samples:
        raise SyntheticEvidenceError(f"{label} sample count mismatch")
    values = [
        _integer(value, f"{label}.samples_ns")
        for value in samples
    ]
    if any(value < 0 for value in values):
        raise SyntheticEvidenceError(f"{label} contains negative timing samples")
    operations = _integer(summary.get("operations_per_sample"), f"{label}.operations_per_sample")
    if operations != expected_operations:
        raise SyntheticEvidenceError(
            f"{label} operations/sample mismatch: expected {expected_operations}, got {operations}"
        )

    ordered = sorted(values)
    expected = {
        "p50_ns": _percentile(ordered, 50),
        "p95_ns": _percentile(ordered, 95),
        "p99_ns": _percentile(ordered, 99),
        "max_ns": ordered[-1],
    }
    expected["p50_ps_per_op"] = expected["p50_ns"] * 1_000 // expected_operations
    for field, value in expected.items():
        observed = _integer(summary.get(field), f"{label}.{field}")
        if observed != value:
            raise SyntheticEvidenceError(
                f"{label}.{field} does not match raw samples: expected {value}, got {observed}"
            )
    return summary


def validate_child(record: dict[str, Any], expectation: ChildExpectation) -> None:
    if record.get("schema") != SCHEMA or record.get("harness_version") != HARNESS_VERSION:
        raise SyntheticEvidenceError("synthetic child schema/version mismatch")
    if record.get("scope") != SCOPE:
        raise SyntheticEvidenceError("synthetic child scope mismatch")
    if record.get("mode") != expectation.mode or record.get("candidate") != expectation.candidate:
        raise SyntheticEvidenceError("synthetic child mode/candidate mismatch")
    if record.get("production_candidate") is not (expectation.candidate != "direct-reference"):
        raise SyntheticEvidenceError("synthetic child production-candidate flag mismatch")
    if record.get("build_profile") != BUILD_PROFILE or record.get("codegen_policy") != CODEGEN_POLICY:
        raise SyntheticEvidenceError("synthetic child build/codegen mismatch")
    if record.get("commit_sha") != expectation.head_sha:
        raise SyntheticEvidenceError("synthetic child source commit mismatch")
    if record.get("rustflags") not in ("", None) or record.get("cargo_encoded_rustflags") not in ("", None):
        raise SyntheticEvidenceError("synthetic child observed unexpected Rust flags")
    if record.get("cpus_allowed_list") != str(expectation.cpu):
        raise SyntheticEvidenceError("synthetic child escaped required CPU affinity")
    if not isinstance(record.get("mems_allowed_list"), str) or not record["mems_allowed_list"]:
        raise SyntheticEvidenceError("synthetic child memory-node affinity provenance is missing")
    for key, value in expectation.target.items():
        if record.get(key) != value:
            raise SyntheticEvidenceError(f"synthetic child target identity mismatch at {key}")

    settings = record.get("settings")
    if settings != expected_settings(expectation.mode):
        raise SyntheticEvidenceError(f"synthetic child settings mismatch: {settings!r}")
    if record.get("promotion_targets") != list(PROMOTION_TARGETS):
        raise SyntheticEvidenceError("synthetic child promotion target set drifted")

    measured_samples = settings["measured_samples"]
    mutations = settings["mutations"]
    promotion_samples = settings["promotion_samples"]
    control_operations = settings["control_operations"]

    control = record.get("control")
    if not isinstance(control, dict) or control.get("workload") != CONTROL_WORKLOAD:
        raise SyntheticEvidenceError("synthetic child control workload identity mismatch")
    if control.get("unit") != "iteration":
        raise SyntheticEvidenceError("synthetic child control unit mismatch")
    validate_summary(
        control.get("timing"),
        expected_samples=measured_samples,
        expected_operations=control_operations,
        label="synthetic control",
    )

    timings = record.get("timings")
    if not isinstance(timings, list):
        raise SyntheticEvidenceError("synthetic child timings must be an array")
    cases = expected_cases(expectation.mode)
    expected_count = len(cases) * len(REPLACEMENTS) + len(PROMOTIONS)
    if len(timings) != expected_count:
        raise SyntheticEvidenceError(
            f"synthetic timing count mismatch: expected {expected_count}, got {len(timings)}"
        )

    seen_replacements: set[tuple[str, str, int]] = set()
    seen_promotions: set[str] = set()
    expected_replacements = {
        (workload, pattern, cardinality)
        for pattern, cardinality in cases
        for workload in REPLACEMENTS
    }
    for index, raw in enumerate(timings):
        if not isinstance(raw, dict):
            raise SyntheticEvidenceError(f"synthetic timing {index} must be an object")
        workload = raw.get("workload")
        if not isinstance(workload, str):
            raise SyntheticEvidenceError(f"synthetic timing {index} workload is malformed")
        pattern = raw.get("pattern")
        pool = _integer(raw.get("pool_cardinality"), f"timing {workload}.pool_cardinality")
        actual = _integer(raw.get("actual_cardinality"), f"timing {workload}.actual_cardinality")
        representation = raw.get("representation")
        if not isinstance(representation, str) or not representation:
            raise SyntheticEvidenceError(f"timing {workload} lacks representation identity")

        if workload in REPLACEMENTS:
            if not isinstance(pattern, str):
                raise SyntheticEvidenceError(f"replacement timing {workload} pattern is malformed")
            identity = (workload, pattern, pool)
            if identity in seen_replacements:
                raise SyntheticEvidenceError(f"duplicate synthetic replacement record: {identity}")
            seen_replacements.add(identity)
            if raw.get("unit") != "replace":
                raise SyntheticEvidenceError(f"replacement timing {identity} unit mismatch")
            if actual <= 0 or actual > pool:
                raise SyntheticEvidenceError(f"replacement timing {identity} actual cardinality invalid")
            validate_summary(
                raw.get("timing"),
                expected_samples=measured_samples,
                expected_operations=mutations,
                label=f"replacement {identity}",
            )
            continue

        if workload in PROMOTIONS:
            if workload in seen_promotions:
                raise SyntheticEvidenceError(f"duplicate synthetic promotion record: {workload}")
            seen_promotions.add(workload)
            target = PROMOTION_TARGETS[PROMOTIONS.index(workload)]
            if pattern != "promotion-boundary" or pool != target or actual != target:
                raise SyntheticEvidenceError(f"promotion timing {workload} identity mismatch")
            if raw.get("unit") != "single-replace":
                raise SyntheticEvidenceError(f"promotion timing {workload} unit mismatch")
            validate_summary(
                raw.get("timing"),
                expected_samples=promotion_samples,
                expected_operations=1,
                label=f"promotion {workload}",
            )
            continue

        raise SyntheticEvidenceError(f"unexpected synthetic workload: {workload}")

    if seen_replacements != expected_replacements:
        missing = expected_replacements - seen_replacements
        extra = seen_replacements - expected_replacements
        raise SyntheticEvidenceError(
            f"synthetic replacement surface mismatch; missing={sorted(missing)} extra={sorted(extra)}"
        )
    if seen_promotions != set(PROMOTIONS):
        raise SyntheticEvidenceError("synthetic promotion surface mismatch")


def child_summary(record: dict[str, Any]) -> dict[str, object]:
    """Return the exact repeat-comparison values after `validate_child` succeeds."""
    replacements: dict[str, int] = {}
    promotions: dict[str, int] = {}
    for raw in record["timings"]:
        workload = raw["workload"]
        timing = raw["timing"]
        if workload in REPLACEMENTS:
            key = "|".join(
                (
                    workload,
                    str(raw["pattern"]),
                    str(raw["pool_cardinality"]),
                    str(raw["actual_cardinality"]),
                    str(raw["representation"]),
                )
            )
            if key in replacements:
                raise SyntheticEvidenceError(f"duplicate normalized replacement key: {key}")
            replacements[key] = int(timing["p50_ps_per_op"])
        else:
            key = "|".join((workload, str(raw["representation"])))
            if key in promotions:
                raise SyntheticEvidenceError(f"duplicate normalized promotion key: {key}")
            promotions[key] = int(timing["p99_ns"])
    return {
        "control_p50_ps_per_op": int(record["control"]["timing"]["p50_ps_per_op"]),
        "replacement_p50_ps_per_op": replacements,
        "promotion_p99_ns": promotions,
    }


def median_int(values: Iterable[int]) -> int:
    ordered = sorted(values)
    if not ordered:
        raise SyntheticEvidenceError("cannot summarize an empty sample set")
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) // 2


def aggregate_int(values: list[int]) -> dict[str, int]:
    median = median_int(values)
    mad = median_int([abs(value - median) for value in values])
    return {
        "count": len(values),
        "median": median,
        "mad": mad,
        "relative_mad_ppm": mad * 1_000_000 // max(abs(median), 1),
        "min": min(values),
        "max": max(values),
    }


def aggregate_children(children: list[dict[str, object]]) -> dict[str, object]:
    by_candidate: dict[str, object] = {}
    for candidate in CANDIDATES:
        selected = [child for child in children if child.get("candidate") == candidate]
        if not selected:
            raise SyntheticEvidenceError(f"missing synthetic children for {candidate}")
        replacement_keys = {
            key
            for child in selected
            for key in dict(child["replacement_p50_ps_per_op"]).keys()
        }
        promotion_keys = {
            key
            for child in selected
            for key in dict(child["promotion_p99_ns"]).keys()
        }
        for child in selected:
            if set(dict(child["replacement_p50_ps_per_op"])) != replacement_keys:
                raise SyntheticEvidenceError(f"replacement record set drifted across rounds for {candidate}")
            if set(dict(child["promotion_p99_ns"])) != promotion_keys:
                raise SyntheticEvidenceError(f"promotion record set drifted across rounds for {candidate}")
        by_candidate[candidate] = {
            "replacement_p50_ps_per_op": {
                key: aggregate_int(
                    [int(dict(child["replacement_p50_ps_per_op"])[key]) for child in selected]
                )
                for key in sorted(replacement_keys)
            },
            "promotion_p99_ns": {
                key: aggregate_int(
                    [int(dict(child["promotion_p99_ns"])[key]) for child in selected]
                )
                for key in sorted(promotion_keys)
            },
        }
    controls = [int(child["control_p50_ps_per_op"]) for child in children]
    return {
        "candidates": by_candidate,
        "global_control_p50_ps_per_op": aggregate_int(controls),
    }


def classify_noise(
    aggregates: dict[str, object],
    *,
    smoke: bool,
    rounds: int,
) -> dict[str, object]:
    reasons: list[str] = []
    protocol_eligible = not smoke and rounds >= 5 and rounds % 5 == 0
    if not protocol_eligible:
        reasons.append("synthetic qualification requires at least five rounds and a multiple of five")

    control = dict(aggregates["global_control_p50_ps_per_op"])
    control_ok = int(control["relative_mad_ppm"]) <= CONTROL_MAX_RELATIVE_MAD_PPM
    if not control_ok:
        reasons.append("synthetic candidate-independent control exceeded noise threshold")

    replacement_ok = True
    promotion_ok = True
    candidates = dict(aggregates["candidates"])
    for candidate in PRODUCTION_CANDIDATES:
        candidate_data = dict(candidates[candidate])
        for key, raw_summary in dict(candidate_data["replacement_p50_ps_per_op"]).items():
            summary = dict(raw_summary)
            if int(summary["relative_mad_ppm"]) > REPLACEMENT_MAX_RELATIVE_MAD_PPM:
                replacement_ok = False
                reasons.append(f"synthetic replacement noise exceeded threshold: {candidate}/{key}")
        for key, raw_summary in dict(candidate_data["promotion_p99_ns"]).items():
            summary = dict(raw_summary)
            if int(summary["relative_mad_ppm"]) > PROMOTION_P99_MAX_RELATIVE_MAD_PPM:
                promotion_ok = False
                reasons.append(f"synthetic promotion-p99 noise exceeded threshold: {candidate}/{key}")

    eligible = protocol_eligible and control_ok and replacement_ok and promotion_ok
    return {
        "protocol_eligible": protocol_eligible,
        "control_noise_eligible": control_ok,
        "replacement_noise_eligible": replacement_ok,
        "promotion_tail_noise_eligible": promotion_ok,
        "synthetic_evidence_eligible": eligible,
        "thresholds_ppm": {
            "control_relative_mad": CONTROL_MAX_RELATIVE_MAD_PPM,
            "replacement_p50_relative_mad": REPLACEMENT_MAX_RELATIVE_MAD_PPM,
            "promotion_p99_relative_mad": PROMOTION_P99_MAX_RELATIVE_MAD_PPM,
        },
        "reasons": sorted(set(reasons)),
    }
