from __future__ import annotations

import json
import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from tools import section_m03d_qualification as driver

COMMIT = "c" * 40
BUNDLE_SHA = "b" * 64
POPULATION_SHA = "a" * 64
ADMISSION_SHA = "d" * 64
SOURCE_ARTIFACT_SHA = "2" * 64
POLICY = "vanilla-section-representative-v1"


class Fixture:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.repo = root / "repo"
        self.representative = root / "representative"
        self.correctness = root / "correctness"
        self.output = root / "output"
        self.repo.mkdir()
        self.representative.mkdir()
        self.correctness.mkdir()
        for candidate in driver.correctness.CANDIDATES:
            directory = self.correctness / candidate
            directory.mkdir()
            (directory / "full.json").write_text("{}\n", encoding="utf-8")
        (self.correctness / driver.correctness.MANIFEST_NAME).write_text(
            "{}\n", encoding="utf-8"
        )


def correctness_record() -> dict[str, object]:
    return {"commit_sha": COMMIT, "bundle_sha256": BUNDLE_SHA}


def population_identity(*, population_sha: str = POPULATION_SHA) -> SimpleNamespace:
    return SimpleNamespace(
        population_sha256=population_sha,
        admission_sha256=ADMISSION_SHA,
        artifact_manifest_sha256=SOURCE_ARTIFACT_SHA,
        policy=POLICY,
    )


def pack_record() -> dict[str, object]:
    record: dict[str, object] = {
        "policy": POLICY,
        "population_sha256": POPULATION_SHA,
        "admission_sha256": ADMISSION_SHA,
        "source_artifact_manifest_sha256": SOURCE_ARTIFACT_SHA,
        "decision_scope": driver.DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
    }
    record["manifest_sha256"] = driver.canonical_digest(record)
    return record


def combined_record(*, eligible: bool = True) -> dict[str, object]:
    record: dict[str, object] = {
        "mode": "qualification",
        "qualification_complete": True,
        "combined_measurement_evidence_eligible": eligible,
        "decision_evidence_eligible": False,
        "decision_scope": driver.DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
        "decision_blockers": (
            []
            if eligible
            else ["population evidence did not pass protocol/noise eligibility"]
        ),
        "identities": {"repository_commit_sha": COMMIT},
    }
    record["evidence_sha256"] = driver.canonical_digest(record)
    return record


def analysis_record(*, selection_ready: bool = True) -> dict[str, object]:
    blockers = ["explicit production-policy selection record not yet committed"]
    if not selection_ready:
        blockers.append(
            "no single production candidate lies on every standard-dimension frontier"
        )
    record: dict[str, object] = {
        "analysis_complete": True,
        "selection_ready": selection_ready,
        "decision_evidence_eligible": False,
        "decision_scope": driver.DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
        "selection_blockers": blockers,
        "interpretation": {"winner_selected": False},
    }
    record["analysis_sha256"] = driver.canonical_digest(record)
    return record


def fake_build_packs(*, output_dir: Path, **_: object) -> dict[str, object]:
    output_dir.mkdir(parents=True)
    record = pack_record()
    (output_dir / "pack-manifest.json").write_text(
        json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return record


def fake_combined(
    *, output_dir: Path, eligible: bool = True, **_: object
) -> dict[str, object]:
    output_dir.mkdir(parents=True)
    record = combined_record(eligible=eligible)
    (output_dir / "combined-orchestration.json").write_text(
        json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    manifest: dict[str, object] = {
        "schema": 1,
        "kind": "section-target-combined-artifact",
        "files": [],
    }
    manifest["manifest_sha256"] = driver.canonical_digest(manifest)
    (output_dir / "artifact-manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return record


def enter_common(stack: ExitStack):
    stack.enter_context(
        mock.patch.object(driver.population, "allowed_cpus", return_value={0, 1, 2, 3})
    )
    stack.enter_context(
        mock.patch.object(driver.population, "repository_identity", return_value=COMMIT)
    )
    correctness_check = stack.enter_context(
        mock.patch.object(
            driver.correctness,
            "validate_sealed_bundle",
            return_value=correctness_record(),
        )
    )
    pack_call = stack.enter_context(
        mock.patch.object(driver.packs, "build_packs", side_effect=fake_build_packs)
    )
    population_check = stack.enter_context(
        mock.patch.object(
            driver.packs, "admit_population", return_value=population_identity()
        )
    )
    return correctness_check, pack_call, population_check


class FinalQualificationDriverTests(unittest.TestCase):
    def test_valid_session_runs_pareto_but_never_selects_policy(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            fixture = Fixture(Path(raw))
            correctness_check, _, population_check = enter_common(stack)
            combined_call = stack.enter_context(
                mock.patch.object(driver.combined, "orchestrate", side_effect=fake_combined)
            )
            pareto_call = stack.enter_context(
                mock.patch.object(driver.pareto, "analyze", return_value=analysis_record())
            )
            session = driver.run_qualification(
                repo_root=fixture.repo,
                representative_root=fixture.representative,
                correctness_bundle=fixture.correctness,
                output_root=fixture.output,
                cpu=3,
            )

            self.assertTrue(session["measurement_evidence_eligible"])
            self.assertTrue(session["pareto_analysis_complete"])
            self.assertTrue(session["decision_review_ready"])
            self.assertFalse(session["decision_evidence_eligible"])
            self.assertFalse(session["production_policy_selected"])
            self.assertIn(
                "explicit human-reviewed production-policy selection remains required",
                session["blockers"],
            )
            self.assertEqual(correctness_check.call_count, 2)
            population_check.assert_called_once_with(fixture.representative)
            self.assertEqual(combined_call.call_args.kwargs["smoke"], False)
            self.assertEqual(combined_call.call_args.kwargs["rounds"], 5)
            expected_paths = [
                fixture.correctness / candidate / "full.json"
                for candidate in driver.correctness.CANDIDATES
            ]
            self.assertEqual(
                pareto_call.call_args.kwargs["correctness_paths"], expected_paths
            )
            self.assertTrue((fixture.output / driver.SESSION_NAME).is_file())
            self.assertTrue((fixture.output / driver.ARTIFACT_NAME).is_file())
            self.assertTrue((fixture.output / driver.PARETO_NAME).is_file())
            self.assertEqual(
                session["identities"]["representative_artifact_manifest_sha256"],
                SOURCE_ARTIFACT_SHA,
            )

    def test_noisy_combined_evidence_is_sealed_but_never_reaches_pareto(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            fixture = Fixture(Path(raw))
            enter_common(stack)

            def noisy_combined(**kwargs: object) -> dict[str, object]:
                return fake_combined(eligible=False, **kwargs)

            stack.enter_context(
                mock.patch.object(driver.combined, "orchestrate", side_effect=noisy_combined)
            )
            pareto_call = stack.enter_context(mock.patch.object(driver.pareto, "analyze"))
            session = driver.run_qualification(
                repo_root=fixture.repo,
                representative_root=fixture.representative,
                correctness_bundle=fixture.correctness,
                output_root=fixture.output,
                cpu=2,
            )

            self.assertFalse(session["measurement_evidence_eligible"])
            self.assertFalse(session["pareto_analysis_complete"])
            self.assertFalse(session["decision_review_ready"])
            pareto_call.assert_not_called()
            self.assertFalse((fixture.output / driver.PARETO_NAME).exists())
            self.assertTrue((fixture.output / driver.SESSION_NAME).is_file())

    def test_pareto_can_complete_without_being_selection_ready(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            fixture = Fixture(Path(raw))
            enter_common(stack)
            stack.enter_context(
                mock.patch.object(driver.combined, "orchestrate", side_effect=fake_combined)
            )
            stack.enter_context(
                mock.patch.object(
                    driver.pareto,
                    "analyze",
                    return_value=analysis_record(selection_ready=False),
                )
            )
            session = driver.run_qualification(
                repo_root=fixture.repo,
                representative_root=fixture.representative,
                correctness_bundle=fixture.correctness,
                output_root=fixture.output,
                cpu=1,
            )
            self.assertTrue(session["measurement_evidence_eligible"])
            self.assertTrue(session["pareto_analysis_complete"])
            self.assertFalse(session["decision_review_ready"])
            self.assertIn(
                "no single production candidate lies on every standard-dimension frontier",
                session["blockers"],
            )

    def test_existing_output_root_is_rejected_before_pack_generation(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            fixture = Fixture(Path(raw))
            fixture.output.mkdir()
            _, pack_call, _ = enter_common(stack)
            with self.assertRaises(driver.FinalQualificationError):
                driver.run_qualification(
                    repo_root=fixture.repo,
                    representative_root=fixture.representative,
                    correctness_bundle=fixture.correctness,
                    output_root=fixture.output,
                    cpu=0,
                )
            pack_call.assert_not_called()

    def test_output_root_cannot_overlap_evidence_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            fixture = Fixture(Path(raw))
            _, pack_call, _ = enter_common(stack)
            with self.assertRaises(driver.FinalQualificationError):
                driver.run_qualification(
                    repo_root=fixture.repo,
                    representative_root=fixture.representative,
                    correctness_bundle=fixture.correctness,
                    output_root=fixture.representative / "run",
                    cpu=0,
                )
            pack_call.assert_not_called()

    def test_input_root_symlink_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            fixture = Fixture(Path(raw))
            representative_link = fixture.root / "representative-link"
            representative_link.symlink_to(fixture.representative, target_is_directory=True)
            _, pack_call, _ = enter_common(stack)
            with self.assertRaises(driver.FinalQualificationError):
                driver.run_qualification(
                    repo_root=fixture.repo,
                    representative_root=representative_link,
                    correctness_bundle=fixture.correctness,
                    output_root=fixture.output,
                    cpu=0,
                )
            pack_call.assert_not_called()

    def test_invalid_round_count_is_rejected_before_any_evidence_work(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            with mock.patch.object(driver.population, "allowed_cpus") as allowed:
                for rounds in (0, 1, 4, 6):
                    with self.subTest(rounds=rounds), self.assertRaises(
                        driver.FinalQualificationError
                    ):
                        driver.run_qualification(
                            repo_root=fixture.repo,
                            representative_root=fixture.representative,
                            correctness_bundle=fixture.correctness,
                            output_root=fixture.output,
                            cpu=0,
                            rounds=rounds,
                        )
                allowed.assert_not_called()

    def test_invalid_cpu_is_rejected_before_repository_or_pack_work(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            with mock.patch.object(
                driver.population, "allowed_cpus", return_value={0, 1}
            ), mock.patch.object(
                driver.population, "repository_identity"
            ) as identity, mock.patch.object(
                driver.packs, "build_packs"
            ) as pack_call:
                with self.assertRaises(driver.FinalQualificationError):
                    driver.run_qualification(
                        repo_root=fixture.repo,
                        representative_root=fixture.representative,
                        correctness_bundle=fixture.correctness,
                        output_root=fixture.output,
                        cpu=3,
                    )
                identity.assert_not_called()
                pack_call.assert_not_called()

    def test_scope_drift_is_rejected_before_pareto(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            fixture = Fixture(Path(raw))
            enter_common(stack)

            def bad_combined(**kwargs: object) -> dict[str, object]:
                record = fake_combined(**kwargs)
                record["cross_dimension_score_allowed"] = True
                payload = dict(record)
                payload.pop("evidence_sha256")
                record["evidence_sha256"] = driver.canonical_digest(payload)
                return record

            stack.enter_context(
                mock.patch.object(driver.combined, "orchestrate", side_effect=bad_combined)
            )
            pareto_call = stack.enter_context(mock.patch.object(driver.pareto, "analyze"))
            with self.assertRaises(driver.FinalQualificationError):
                driver.run_qualification(
                    repo_root=fixture.repo,
                    representative_root=fixture.representative,
                    correctness_bundle=fixture.correctness,
                    output_root=fixture.output,
                    cpu=0,
                )
            pareto_call.assert_not_called()

    def test_repository_change_after_measurement_invalidates_session_seal(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            fixture = Fixture(Path(raw))
            stack.enter_context(
                mock.patch.object(driver.population, "allowed_cpus", return_value={0})
            )
            stack.enter_context(
                mock.patch.object(
                    driver.population,
                    "repository_identity",
                    side_effect=[COMMIT, "d" * 40],
                )
            )
            stack.enter_context(
                mock.patch.object(
                    driver.correctness,
                    "validate_sealed_bundle",
                    return_value=correctness_record(),
                )
            )
            stack.enter_context(
                mock.patch.object(driver.packs, "build_packs", side_effect=fake_build_packs)
            )
            stack.enter_context(
                mock.patch.object(driver.combined, "orchestrate", side_effect=fake_combined)
            )
            stack.enter_context(
                mock.patch.object(driver.pareto, "analyze", return_value=analysis_record())
            )
            with self.assertRaises(driver.FinalQualificationError):
                driver.run_qualification(
                    repo_root=fixture.repo,
                    representative_root=fixture.representative,
                    correctness_bundle=fixture.correctness,
                    output_root=fixture.output,
                    cpu=0,
                )
            self.assertFalse((fixture.output / driver.SESSION_NAME).exists())

    def test_representative_population_change_invalidates_session_seal(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            fixture = Fixture(Path(raw))
            stack.enter_context(
                mock.patch.object(driver.population, "allowed_cpus", return_value={0})
            )
            stack.enter_context(
                mock.patch.object(driver.population, "repository_identity", return_value=COMMIT)
            )
            stack.enter_context(
                mock.patch.object(
                    driver.correctness,
                    "validate_sealed_bundle",
                    return_value=correctness_record(),
                )
            )
            stack.enter_context(
                mock.patch.object(driver.packs, "build_packs", side_effect=fake_build_packs)
            )
            stack.enter_context(
                mock.patch.object(
                    driver.packs,
                    "admit_population",
                    return_value=population_identity(population_sha="3" * 64),
                )
            )
            stack.enter_context(
                mock.patch.object(driver.combined, "orchestrate", side_effect=fake_combined)
            )
            stack.enter_context(
                mock.patch.object(driver.pareto, "analyze", return_value=analysis_record())
            )
            with self.assertRaises(driver.FinalQualificationError):
                driver.run_qualification(
                    repo_root=fixture.repo,
                    representative_root=fixture.representative,
                    correctness_bundle=fixture.correctness,
                    output_root=fixture.output,
                    cpu=0,
                )
            self.assertFalse((fixture.output / driver.SESSION_NAME).exists())

    def test_session_artifact_manifest_hashes_generated_chain(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            fixture = Fixture(Path(raw))
            enter_common(stack)
            stack.enter_context(
                mock.patch.object(driver.combined, "orchestrate", side_effect=fake_combined)
            )
            stack.enter_context(
                mock.patch.object(driver.pareto, "analyze", return_value=analysis_record())
            )
            session = driver.run_qualification(
                repo_root=fixture.repo,
                representative_root=fixture.representative,
                correctness_bundle=fixture.correctness,
                output_root=fixture.output,
                cpu=0,
            )
            manifest = json.loads(
                (fixture.output / driver.ARTIFACT_NAME).read_text(encoding="utf-8")
            )
            self.assertEqual(manifest["session_sha256"], session["session_sha256"])
            paths = {entry["path"] for entry in manifest["files"]}
            self.assertIn(driver.SESSION_NAME, paths)
            self.assertIn(driver.PARETO_NAME, paths)
            self.assertIn("packs/pack-manifest.json", paths)
            self.assertIn("combined/artifact-manifest.json", paths)
            payload = dict(manifest)
            digest = payload.pop("manifest_sha256")
            self.assertEqual(digest, driver.canonical_digest(payload))


if __name__ == "__main__":
    unittest.main()
