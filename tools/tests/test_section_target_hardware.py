from __future__ import annotations

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

import section_target_hardware as target

REPO = Path(".")


def _repository_target() -> dict[str, object]:
    manifest = json.loads(
        Path("vanilla/state-data/26.2-state-data-manifest.json").read_text(
            encoding="utf-8"
        )
    )
    return {
        "minecraft_version": manifest["target"]["minecraft_version"],
        "protocol_version": manifest["target"]["protocol_version"],
        "data_version": manifest["target"]["data_version"],
        "state_count": manifest["state_count"],
        "state_data_generation_sha256": manifest["generation_digest"],
        "state_data_input_sha256": manifest["input_digest"],
    }


def _fake_pack_root(root: Path) -> dict[str, object]:
    packs: dict[str, object] = {}
    for index, dimension in enumerate(target.DIMENSIONS):
        name = f"dimension-{index}.section-pack"
        payload = f"pack:{dimension}".encode("ascii")
        path = root / name
        path.write_bytes(payload)
        packs[dimension] = {
            "path": name,
            "section_count": 4,
            "total_cells": 4 * 4096,
            "size": len(payload),
            "sha256": hashlib.sha256(payload).hexdigest(),
        }
    members = [
        {
            "seed_index": seed,
            "corpus_sha256": hashlib.sha256(
                f"corpus-{seed}".encode("ascii")
            ).hexdigest(),
            "per_dimension_sections": {
                dimension: 1 for dimension in target.DIMENSIONS
            },
        }
        for seed in range(4)
    ]
    manifest: dict[str, object] = {
        "schema": target.PACK_SCHEMA,
        "kind": target.PACK_KIND,
        "policy": target.REPRESENTATIVE_POLICY,
        "population_sha256": "a" * 64,
        "admission_sha256": "b" * 64,
        "source_artifact_manifest_sha256": "c" * 64,
        "target": _repository_target(),
        "members": members,
        "decision_scope": target.DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
        "packs": packs,
    }
    manifest["manifest_sha256"] = target.canonical_digest(manifest)
    (root / "pack-manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return manifest


def _rewrite_manifest(root: Path, mutate) -> None:
    path = root / "pack-manifest.json"
    manifest = json.loads(path.read_text(encoding="utf-8"))
    manifest.pop("manifest_sha256", None)
    mutate(manifest)
    manifest["manifest_sha256"] = target.canonical_digest(manifest)
    path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def _summary(samples: int, base: int = 100) -> dict[str, object]:
    return {
        "operations_per_sample": 10,
        "p50_ns": base,
        "p95_ns": base + 1,
        "p99_ns": base + 2,
        "max_ns": base + 3,
        "p50_ps_per_op": base * 100,
        "samples_ns": [base + index for index in range(samples)],
    }


def _valid_child(
    *,
    scheduled: target.ScheduledChild,
    pack_set: target.PackSet,
    head_sha: str = "d" * 40,
    cpu: int = 7,
    smoke: bool = False,
) -> dict[str, object]:
    entry = pack_set.entries[scheduled.dimension]
    samples = 3 if smoke else 21
    return {
        "schema": target.CHILD_SCHEMA,
        "harness_version": target.CHILD_VERSION,
        "mode": "smoke" if smoke else "qualification",
        "candidate": scheduled.candidate,
        "production_candidate": scheduled.candidate != "direct-reference",
        "build_profile": target.BUILD_PROFILE,
        "codegen_policy": target.CODEGEN_POLICY,
        "minecraft_version": pack_set.target["minecraft_version"],
        "protocol_version": pack_set.target["protocol_version"],
        "data_version": pack_set.target["data_version"],
        "state_count": pack_set.target["state_count"],
        "state_data_generation_sha256": pack_set.target[
            "state_data_generation_sha256"
        ],
        "state_data_input_sha256": pack_set.target["state_data_input_sha256"],
        "population_sha256": pack_set.population_sha256,
        "admission_sha256": pack_set.admission_sha256,
        "dimension": scheduled.dimension,
        "section_count": entry.section_count,
        "commit_sha": head_sha,
        "rustflags": "",
        "cargo_encoded_rustflags": "",
        "cpus_allowed_list": str(cpu),
        "mems_allowed_list": "0",
        "memory": {
            "rss_protocol": target.RSS_PROTOCOL,
            "rss_baseline_kib": 1_000,
            "rss_loaded_kib": 1_250,
            "rss_loaded_delta_kib": 250,
        },
        "representations": {"uniform": entry.section_count},
        "construction": _summary(entry.section_count),
        "timings": [
            {
                "workload": workload,
                "unit": "query",
                "timing": _summary(samples, 100 + index),
            }
            for index, workload in enumerate(target.WORKLOADS)
        ],
    }


def _stable_children(rounds: int) -> list[dict[str, object]]:
    children: list[dict[str, object]] = []
    for item in target.schedule(rounds):
        drift = item.round_index
        children.append(
            {
                "round": item.round_index,
                "dimension": item.dimension,
                "candidate": item.candidate,
                "rss_loaded_delta_kib": 10_000 + drift,
                "construction_p99_ns": 2_000 + drift,
                "timing_p50_ps_per_op": {
                    workload: 1_000 + drift for workload in target.WORKLOADS
                },
            }
        )
    return children


class PackFirewallTests(unittest.TestCase):
    def test_valid_content_addressed_pack_set_is_admitted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _fake_pack_root(root)
            admitted = target.verify_pack_set(root, REPO)
            self.assertEqual(admitted.policy, target.REPRESENTATIVE_POLICY)
            self.assertEqual(set(admitted.entries), set(target.DIMENSIONS))
            self.assertEqual(admitted.population_sha256, "a" * 64)

    def test_unknown_representative_policy_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _fake_pack_root(root)
            _rewrite_manifest(
                root,
                lambda manifest: manifest.__setitem__("policy", "future-policy-v2"),
            )
            with self.assertRaises(target.QualificationError):
                target.verify_pack_set(root, REPO)

    def test_corrupted_pack_is_rejected_even_when_manifest_is_unchanged(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = _fake_pack_root(root)
            entry = manifest["packs"][target.DIMENSIONS[0]]
            (root / entry["path"]).write_bytes(b"corrupted")
            with self.assertRaises(target.QualificationError):
                target.verify_pack_set(root, REPO)

    def test_manifest_digest_drift_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _fake_pack_root(root)
            path = root / "pack-manifest.json"
            manifest = json.loads(path.read_text(encoding="utf-8"))
            manifest["population_sha256"] = "e" * 64
            path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaises(target.QualificationError):
                target.verify_pack_set(root, REPO)

    def test_cross_dimension_scoring_and_dimension_drift_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _fake_pack_root(root)
            _rewrite_manifest(
                root,
                lambda manifest: manifest.__setitem__(
                    "cross_dimension_score_allowed", True
                ),
            )
            with self.assertRaises(target.QualificationError):
                target.verify_pack_set(root, REPO)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _fake_pack_root(root)

            def mutate(manifest: dict[str, object]) -> None:
                manifest["packs"].pop(target.DIMENSIONS[-1])

            _rewrite_manifest(root, mutate)
            with self.assertRaises(target.QualificationError):
                target.verify_pack_set(root, REPO)

    def test_member_seed_count_and_path_firewalls_are_rechecked(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _fake_pack_root(root)

            def duplicate_seed(manifest: dict[str, object]) -> None:
                manifest["members"][3]["seed_index"] = 2

            _rewrite_manifest(root, duplicate_seed)
            with self.assertRaises(target.QualificationError):
                target.verify_pack_set(root, REPO)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _fake_pack_root(root)

            def wrong_count(manifest: dict[str, object]) -> None:
                manifest["members"][0]["per_dimension_sections"][
                    target.DIMENSIONS[0]
                ] = 2

            _rewrite_manifest(root, wrong_count)
            with self.assertRaises(target.QualificationError):
                target.verify_pack_set(root, REPO)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _fake_pack_root(root)

            def unsafe_path(manifest: dict[str, object]) -> None:
                manifest["packs"][target.DIMENSIONS[0]]["path"] = "../escape.pack"

            _rewrite_manifest(root, unsafe_path)
            with self.assertRaises(target.QualificationError):
                target.verify_pack_set(root, REPO)


class BuildAndScheduleTests(unittest.TestCase):
    def test_hidden_compiler_and_release_profile_overrides_are_rejected(self) -> None:
        for key in (
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "RUSTC_WRAPPER",
            "RUSTC_BOOTSTRAP",
            "CARGO_PROFILE_RELEASE_LTO",
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
        ):
            with self.subTest(key=key):
                self.assertIn(key, target.forbidden_environment({key: "bad"}))
        self.assertEqual(target.forbidden_environment({"RUSTFLAGS": ""}), [])

    def test_parent_cargo_config_rejects_binary_affecting_settings(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.toml"
            for parsed in (
                {"build": {"rustflags": ["-Ctarget-cpu=native"]}},
                {"target": {"x86_64-unknown-linux-gnu": {"linker": "custom-ld"}}},
                {"env": {"RUSTFLAGS": "-Ctarget-cpu=native"}},
            ):
                with self.subTest(parsed=parsed):
                    with self.assertRaises(target.QualificationError):
                        target._reject_semantic_parent_cargo_config(path, parsed)
            target._reject_semantic_parent_cargo_config(path, {"alias": {"xtask": "run"}})

    def test_five_round_schedule_balances_candidate_positions_per_dimension(self) -> None:
        scheduled = target.schedule(5)
        self.assertEqual(len(scheduled), 5 * len(target.DIMENSIONS) * len(target.CANDIDATES))
        for dimension in target.DIMENSIONS:
            positions = {candidate: [] for candidate in target.CANDIDATES}
            for item in scheduled:
                if item.dimension == dimension:
                    positions[item.candidate].append(item.candidate_position)
            for candidate in target.CANDIDATES:
                self.assertEqual(sorted(positions[candidate]), list(range(5)))

    def test_dimension_order_rotates_deterministically(self) -> None:
        scheduled = target.schedule(3)
        first_per_round = []
        for round_index in range(3):
            first = next(
                item
                for item in scheduled
                if item.round_index == round_index and item.dimension_position == 0
            )
            first_per_round.append(first.dimension)
        self.assertEqual(tuple(first_per_round), target.DIMENSIONS)

    def test_median_and_mad_summary_is_integer_and_deterministic(self) -> None:
        self.assertEqual(target.median_int([4, 1, 3, 2]), 2)
        summary = target.aggregate_int([100, 101, 102, 103, 104])
        self.assertEqual(summary["median"], 102)
        self.assertEqual(summary["mad"], 1)
        self.assertEqual(summary["min"], 100)
        self.assertEqual(summary["max"], 104)


class ChildEvidenceTests(unittest.TestCase):
    def _pack_set(self, root: Path) -> target.PackSet:
        _fake_pack_root(root)
        return target.verify_pack_set(root, REPO)

    def test_exact_child_identity_and_rss_contract_is_admitted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            pack_set = self._pack_set(Path(directory))
            item = target.schedule(1)[0]
            record = _valid_child(scheduled=item, pack_set=pack_set)
            target.validate_child_record(
                record,
                scheduled=item,
                pack_set=pack_set,
                head_sha="d" * 40,
                cpu=7,
                smoke=False,
            )

    def test_child_identity_tampering_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            pack_set = self._pack_set(Path(directory))
            item = target.schedule(1)[0]
            for field, value in (
                ("commit_sha", "e" * 40),
                ("cpus_allowed_list", "7-8"),
                ("codegen_policy", "lto=off"),
                ("population_sha256", "f" * 64),
            ):
                with self.subTest(field=field):
                    record = _valid_child(scheduled=item, pack_set=pack_set)
                    record[field] = value
                    with self.assertRaises(target.QualificationError):
                        target.validate_child_record(
                            record,
                            scheduled=item,
                            pack_set=pack_set,
                            head_sha="d" * 40,
                            cpu=7,
                            smoke=False,
                        )

    def test_child_signed_rss_workload_and_construction_contracts_are_rechecked(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            pack_set = self._pack_set(Path(directory))
            item = target.schedule(1)[0]

            record = _valid_child(scheduled=item, pack_set=pack_set)
            record["memory"]["rss_loaded_delta_kib"] = 249
            with self.assertRaises(target.QualificationError):
                target.validate_child_record(
                    record,
                    scheduled=item,
                    pack_set=pack_set,
                    head_sha="d" * 40,
                    cpu=7,
                    smoke=False,
                )

            record = _valid_child(scheduled=item, pack_set=pack_set)
            record["timings"].pop()
            with self.assertRaises(target.QualificationError):
                target.validate_child_record(
                    record,
                    scheduled=item,
                    pack_set=pack_set,
                    head_sha="d" * 40,
                    cpu=7,
                    smoke=False,
                )

            record = _valid_child(scheduled=item, pack_set=pack_set)
            record["construction"]["samples_ns"].pop()
            with self.assertRaises(target.QualificationError):
                target.validate_child_record(
                    record,
                    scheduled=item,
                    pack_set=pack_set,
                    head_sha="d" * 40,
                    cpu=7,
                    smoke=False,
                )


class NoiseQualificationTests(unittest.TestCase):
    def test_stable_five_round_population_evidence_can_pass_population_gate(self) -> None:
        aggregates = target.aggregate_children(_stable_children(5))
        noise = target.classify_noise(aggregates, smoke=False, rounds=5)
        self.assertTrue(noise["protocol_eligible"])
        self.assertTrue(noise["control_noise_eligible"])
        self.assertTrue(noise["workload_noise_eligible"])
        self.assertTrue(noise["rss_noise_eligible"])
        self.assertTrue(noise["population_evidence_eligible"])

    def test_completed_smoke_is_never_population_decision_input(self) -> None:
        aggregates = target.aggregate_children(_stable_children(1))
        noise = target.classify_noise(aggregates, smoke=True, rounds=1)
        self.assertFalse(noise["protocol_eligible"])
        self.assertFalse(noise["population_evidence_eligible"])

    def test_high_run_to_run_drift_downgrades_successful_run_to_diagnostic(self) -> None:
        children = _stable_children(5)
        for child in children:
            drift = int(child["round"])
            child["timing_p50_ps_per_op"] = {
                workload: 1_000 + drift * 500 for workload in target.WORKLOADS
            }
        noise = target.classify_noise(
            target.aggregate_children(children), smoke=False, rounds=5
        )
        self.assertFalse(noise["control_noise_eligible"])
        self.assertFalse(noise["workload_noise_eligible"])
        self.assertFalse(noise["population_evidence_eligible"])

    def test_negative_or_unstable_production_rss_cannot_qualify(self) -> None:
        children = _stable_children(5)
        for child in children:
            if (
                child["candidate"] == "packed-local"
                and child["dimension"] == target.DIMENSIONS[0]
            ):
                child["rss_loaded_delta_kib"] = -1
        noise = target.classify_noise(
            target.aggregate_children(children), smoke=False, rounds=5
        )
        self.assertFalse(noise["rss_noise_eligible"])
        self.assertFalse(noise["population_evidence_eligible"])

    def test_reference_only_rss_instability_does_not_block_production_population(self) -> None:
        children = _stable_children(5)
        for child in children:
            if child["candidate"] == "direct-reference":
                child["rss_loaded_delta_kib"] = -100
        noise = target.classify_noise(
            target.aggregate_children(children), smoke=False, rounds=5
        )
        self.assertTrue(noise["rss_noise_eligible"])
        self.assertTrue(noise["population_evidence_eligible"])


if __name__ == "__main__":
    unittest.main()
