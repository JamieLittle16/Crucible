from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import section_correctness_bundle as bundle

COMMIT = "c" * 40
GENERATION_SHA = "a" * 64
INPUT_SHA = "b" * 64


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def evidence(candidate: str, commit: str = COMMIT) -> dict[str, object]:
    return {
        "schema": 1,
        "qualification": "section",
        "mode": "full",
        "minecraft_version": "26.2",
        "protocol_version": 776,
        "data_version": 4903,
        "commit_sha": commit,
        "state_count": 32_366,
        "state_data_input_sha256": INPUT_SHA,
        "state_data_generation_sha256": GENERATION_SHA,
        "trace_schema": bundle.TRACE_SCHEMA,
        "sem_ids": list(bundle.SEM_IDS),
        "records": [
            {
                "id": f"EQUIV-WORLD-SECTION-FULL-{candidate.upper().replace('-', '_')}",
                "candidate": candidate,
                "trace_count": bundle.TRACE_COUNT,
                "trace_operations": bundle.TRACE_OPERATIONS,
                "synthetic_operations": bundle.SYNTHETIC_OPERATIONS,
                "trace_fingerprint_fnv1a64": bundle.TRACE_FINGERPRINT,
            }
        ],
    }


class Fixture:
    def __init__(self, root: Path) -> None:
        self.repo = root / "repo"
        self.input = root / "input"
        write_json(
            self.repo / "vanilla/state-data/26.2-state-data-manifest.json",
            {
                "target": {
                    "minecraft_version": "26.2",
                    "protocol_version": 776,
                    "data_version": 4903,
                },
                "state_count": 32_366,
                "input_digest": INPUT_SHA,
                "generation_digest": GENERATION_SHA,
            },
        )
        for candidate in bundle.CANDIDATES:
            write_json(self.input / candidate / "full.json", evidence(candidate))

    def seal(self) -> dict[str, object]:
        manifest = bundle.build_bundle(
            repo_root=self.repo,
            input_root=self.input,
            expected_commit=COMMIT,
        )
        write_json(self.input / bundle.MANIFEST_NAME, manifest)
        return manifest


class CorrectnessBundleTests(unittest.TestCase):
    def test_complete_same_commit_bundle_is_accepted_and_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            first = bundle.build_bundle(
                repo_root=fixture.repo,
                input_root=fixture.input,
                expected_commit=COMMIT,
            )
            second = bundle.build_bundle(
                repo_root=fixture.repo,
                input_root=fixture.input,
                expected_commit=COMMIT,
            )
            self.assertEqual(first, second)
            self.assertEqual(first["commit_sha"], COMMIT)
            self.assertEqual(first["candidate_order"], list(bundle.CANDIDATES))
            self.assertEqual(
                first["bundle_sha256"],
                bundle.canonical_digest(
                    {key: value for key, value in first.items() if key != "bundle_sha256"}
                ),
            )

    def test_sealed_bundle_reopens_and_revalidates_every_child(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            expected = fixture.seal()
            reopened = bundle.validate_sealed_bundle(
                repo_root=fixture.repo,
                bundle_root=fixture.input,
                expected_commit=COMMIT,
            )
            self.assertEqual(reopened, expected)

    def test_missing_candidate_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            (fixture.input / "adaptive/full.json").unlink()
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.build_bundle(repo_root=fixture.repo, input_root=fixture.input)

    def test_unexpected_top_level_entry_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            (fixture.input / "direct-reference").mkdir()
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.build_bundle(repo_root=fixture.repo, input_root=fixture.input)

        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            (fixture.input / "notes.txt").write_text("stale evidence\n", encoding="utf-8")
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.build_bundle(repo_root=fixture.repo, input_root=fixture.input)

    def test_candidate_directory_must_contain_only_full_json(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            (fixture.input / "direct/old.json").write_text("{}\n", encoding="utf-8")
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.build_bundle(repo_root=fixture.repo, input_root=fixture.input)

    def test_sealed_bundle_inventory_is_closed(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            fixture.seal()
            (fixture.input / "notes.txt").write_text("not evidence\n", encoding="utf-8")
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.validate_sealed_bundle(
                    repo_root=fixture.repo, bundle_root=fixture.input
                )

    def test_candidate_path_identity_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            write_json(fixture.input / "adaptive/full.json", evidence("direct"))
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.build_bundle(repo_root=fixture.repo, input_root=fixture.input)

    def test_mixed_commit_bundle_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            write_json(
                fixture.input / "packed-local/full.json",
                evidence("packed-local", "d" * 40),
            )
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.build_bundle(repo_root=fixture.repo, input_root=fixture.input)

    def test_expected_commit_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.build_bundle(
                    repo_root=fixture.repo,
                    input_root=fixture.input,
                    expected_commit="d" * 40,
                )

    def test_sealed_bundle_expected_commit_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            fixture.seal()
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.validate_sealed_bundle(
                    repo_root=fixture.repo,
                    bundle_root=fixture.input,
                    expected_commit="d" * 40,
                )

    def test_target_digest_and_version_drift_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            path = fixture.input / "direct/full.json"
            record = evidence("direct")
            record["state_data_generation_sha256"] = "0" * 64
            write_json(path, record)
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.build_bundle(repo_root=fixture.repo, input_root=fixture.input)

    def test_target_numeric_contract_is_typed_fail_closed(self) -> None:
        for key in ("protocol_version", "data_version", "state_count"):
            with self.subTest(key=key), tempfile.TemporaryDirectory() as raw:
                fixture = Fixture(Path(raw))
                path = fixture.repo / "vanilla/state-data/26.2-state-data-manifest.json"
                manifest = json.loads(path.read_text(encoding="utf-8"))
                if key == "state_count":
                    manifest[key] = True
                else:
                    manifest["target"][key] = True
                write_json(path, manifest)
                with self.assertRaises(bundle.CorrectnessBundleError):
                    bundle.build_bundle(repo_root=fixture.repo, input_root=fixture.input)

    def test_trace_counts_fingerprint_and_sem_surface_are_rejected_on_drift(self) -> None:
        mutations = [
            ("trace_count", bundle.TRACE_COUNT + 1),
            ("trace_operations", bundle.TRACE_OPERATIONS - 1),
            ("synthetic_operations", bundle.SYNTHETIC_OPERATIONS + 1),
            ("trace_fingerprint_fnv1a64", "0" * 16),
        ]
        for field, value in mutations:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as raw:
                fixture = Fixture(Path(raw))
                path = fixture.input / "fast-local/full.json"
                record = evidence("fast-local")
                record["records"][0][field] = value
                write_json(path, record)
                with self.assertRaises(bundle.CorrectnessBundleError):
                    bundle.build_bundle(repo_root=fixture.repo, input_root=fixture.input)

        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            path = fixture.input / "fast-local/full.json"
            record = evidence("fast-local")
            record["sem_ids"] = list(bundle.SEM_IDS[:-1])
            write_json(path, record)
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.build_bundle(repo_root=fixture.repo, input_root=fixture.input)

    def test_sealed_manifest_digest_tampering_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            manifest = fixture.seal()
            manifest["trace_schema"] = 999
            write_json(fixture.input / bundle.MANIFEST_NAME, manifest)
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.validate_sealed_bundle(
                    repo_root=fixture.repo, bundle_root=fixture.input
                )

    def test_sealed_child_tampering_is_rejected_even_if_manifest_is_untouched(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            fixture.seal()
            path = fixture.input / "adaptive/full.json"
            record = evidence("adaptive")
            record["records"][0]["trace_count"] = bundle.TRACE_COUNT + 1
            write_json(path, record)
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.validate_sealed_bundle(
                    repo_root=fixture.repo, bundle_root=fixture.input
                )

    def test_sealed_candidate_manifest_metadata_tampering_is_rejected_with_recomputed_digest(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            fixture = Fixture(Path(raw))
            manifest = fixture.seal()
            manifest["candidates"]["direct"]["sha256"] = "0" * 64
            payload = dict(manifest)
            payload.pop("bundle_sha256")
            manifest["bundle_sha256"] = bundle.canonical_digest(payload)
            write_json(fixture.input / bundle.MANIFEST_NAME, manifest)
            with self.assertRaises(bundle.CorrectnessBundleError):
                bundle.validate_sealed_bundle(
                    repo_root=fixture.repo, bundle_root=fixture.input
                )

    def test_git_commit_and_sha256_formats_are_strict(self) -> None:
        self.assertEqual(bundle.git_sha(COMMIT, "commit"), COMMIT)
        self.assertEqual(bundle.sha256(GENERATION_SHA, "digest"), GENERATION_SHA)
        with self.assertRaises(bundle.CorrectnessBundleError):
            bundle.git_sha(GENERATION_SHA, "wrong")
        with self.assertRaises(bundle.CorrectnessBundleError):
            bundle.sha256(COMMIT, "wrong")


if __name__ == "__main__":
    unittest.main()
