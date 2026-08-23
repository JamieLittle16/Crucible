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


def fake_build_packs(*, output_dir: Path, **_: object) -> dict[str, object]:
    output_dir.mkdir(parents=True)
    record = pack_record()
    (output_dir / "pack-manifest.json").write_text(
        json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return record


def noisy_combined_with_pack_tamper(
    *, pack_root: Path, output_dir: Path, **_: object
) -> dict[str, object]:
    output_dir.mkdir(parents=True)

    manifest_path = pack_root / "pack-manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["tamper_marker"] = True
    manifest.pop("manifest_sha256")
    manifest["manifest_sha256"] = driver.canonical_digest(manifest)
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    combined: dict[str, object] = {
        "mode": "qualification",
        "qualification_complete": True,
        "combined_measurement_evidence_eligible": False,
        "decision_evidence_eligible": False,
        "decision_scope": driver.DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
        "decision_blockers": ["synthetic noise gate did not pass"],
        "identities": {"repository_commit_sha": COMMIT},
    }
    combined["evidence_sha256"] = driver.canonical_digest(combined)
    return combined


class FinalQualificationPackRevalidationTests(unittest.TestCase):
    def test_pack_manifest_mutation_after_measurement_prevents_session_seal(self) -> None:
        with tempfile.TemporaryDirectory() as raw, ExitStack() as stack:
            root = Path(raw)
            repo = root / "repo"
            representative = root / "representative"
            correctness = root / "correctness"
            output = root / "output"
            repo.mkdir()
            representative.mkdir()
            correctness.mkdir()

            stack.enter_context(
                mock.patch.object(driver.population, "allowed_cpus", return_value={0})
            )
            stack.enter_context(
                mock.patch.object(
                    driver.population, "repository_identity", return_value=COMMIT
                )
            )
            stack.enter_context(
                mock.patch.object(
                    driver.correctness,
                    "validate_sealed_bundle",
                    return_value={
                        "commit_sha": COMMIT,
                        "bundle_sha256": BUNDLE_SHA,
                    },
                )
            )
            stack.enter_context(
                mock.patch.object(
                    driver.packs, "build_packs", side_effect=fake_build_packs
                )
            )
            stack.enter_context(
                mock.patch.object(
                    driver.packs,
                    "admit_population",
                    return_value=SimpleNamespace(
                        population_sha256=POPULATION_SHA,
                        admission_sha256=ADMISSION_SHA,
                        artifact_manifest_sha256=SOURCE_ARTIFACT_SHA,
                        policy=POLICY,
                    ),
                )
            )
            stack.enter_context(
                mock.patch.object(
                    driver.combined,
                    "orchestrate",
                    side_effect=noisy_combined_with_pack_tamper,
                )
            )
            pareto_call = stack.enter_context(mock.patch.object(driver.pareto, "analyze"))

            with self.assertRaisesRegex(
                driver.FinalQualificationError,
                "benchmark pack manifest changed during qualification session",
            ):
                driver.run_qualification(
                    repo_root=repo,
                    representative_root=representative,
                    correctness_bundle=correctness,
                    output_root=output,
                    cpu=0,
                )

            pareto_call.assert_not_called()
            self.assertFalse((output / driver.SESSION_NAME).exists())
            self.assertFalse((output / driver.ARTIFACT_NAME).exists())


if __name__ == "__main__":
    unittest.main()
