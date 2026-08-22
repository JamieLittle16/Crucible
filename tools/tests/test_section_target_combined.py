from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import section_target_combined as combined
from tools import section_target_hardware as population

HEAD = "a" * 40
CPU = 7


def population_record(binary_sha: str, *, smoke: bool = True, rounds: int = 1) -> dict[str, object]:
    record: dict[str, object] = {
        "schema": population.SCHEMA,
        "kind": population.KIND,
        "mode": "smoke" if smoke else "qualification",
        "qualification_complete": True,
        "population_evidence_eligible": False if smoke else True,
        "decision_evidence_eligible": False,
        "decision_blockers": ["synthetic evidence absent"],
        "decision_scope": combined.DECISION_SCOPE,
        "cross_dimension_score_allowed": False,
        "rounds": rounds,
        "cpu": CPU,
        "identities": {
            "repository_commit_sha": HEAD,
            "benchmark_executable_sha256": binary_sha,
        },
        "build": {
            "offline": True,
            "profile": population.BUILD_PROFILE,
            "codegen_policy": population.CODEGEN_POLICY,
        },
        "aggregates": {},
        "noise_qualification": {},
    }
    record["evidence_sha256"] = combined.canonical_digest(record)
    return record


class RuntimeEnvironmentTests(unittest.TestCase):
    def test_clean_environment_is_sanitized_for_binary_execution(self) -> None:
        source = {"PATH": "/bin", "HOME": "/tmp/home", "UNRELATED": "ok"}
        result = combined.runtime_environment(source)
        self.assertEqual(result["RUSTFLAGS"], "")
        self.assertEqual(result["CARGO_ENCODED_RUSTFLAGS"], "")
        self.assertEqual(result["UNRELATED"], "ok")

    def test_compiler_and_release_overrides_fail_closed(self) -> None:
        for key in (
            "RUSTFLAGS",
            "RUSTC_WRAPPER",
            "CARGO_PROFILE_RELEASE_LTO",
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
        ):
            with self.subTest(key=key):
                with self.assertRaises(combined.CombinedEvidenceError):
                    combined.runtime_environment({key: "unexpected"})


class PopulationBindingTests(unittest.TestCase):
    def test_population_record_is_bound_to_exact_retained_binary(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / "benchmark-executable"
            binary.write_bytes(b"exact-binary")
            digest = combined.sha256_file(binary)
            record = population_record(digest)
            observed = combined.validate_population_record(
                record,
                head_sha=HEAD,
                cpu=CPU,
                rounds=1,
                smoke=True,
                binary_path=binary,
            )
            self.assertEqual(observed, record["evidence_sha256"])

            binary.write_bytes(b"changed")
            with self.assertRaises(combined.CombinedEvidenceError):
                combined.validate_population_record(
                    record,
                    head_sha=HEAD,
                    cpu=CPU,
                    rounds=1,
                    smoke=True,
                    binary_path=binary,
                )

    def test_population_record_digest_and_protocol_tampering_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / "benchmark-executable"
            binary.write_bytes(b"exact-binary")
            record = population_record(combined.sha256_file(binary))
            for field, value in (
                ("cpu", CPU + 1),
                ("decision_scope", "invented-cross-dimension-score"),
                ("cross_dimension_score_allowed", True),
                ("decision_evidence_eligible", True),
            ):
                with self.subTest(field=field):
                    changed = dict(record)
                    changed[field] = value
                    changed["evidence_sha256"] = combined.canonical_digest(
                        {key: item for key, item in changed.items() if key != "evidence_sha256"}
                    )
                    with self.assertRaises(combined.CombinedEvidenceError):
                        combined.validate_population_record(
                            changed,
                            head_sha=HEAD,
                            cpu=CPU,
                            rounds=1,
                            smoke=True,
                            binary_path=binary,
                        )

            changed = dict(record)
            changed["mode"] = "qualification"
            with self.assertRaises(combined.CombinedEvidenceError):
                combined.validate_population_record(
                    changed,
                    head_sha=HEAD,
                    cpu=CPU,
                    rounds=1,
                    smoke=True,
                    binary_path=binary,
                )

    def test_population_artifact_reopens_and_rehashes_every_retained_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "benchmark-executable"
            orchestration = root / "orchestration.json"
            binary.write_bytes(b"binary")
            orchestration.write_text("{}\n", encoding="utf-8")
            evidence_sha = "e" * 64
            files = [
                {
                    "path": path.name,
                    "size": path.stat().st_size,
                    "sha256": combined.sha256_file(path),
                }
                for path in (binary, orchestration)
            ]
            artifact: dict[str, object] = {
                "schema": population.ARTIFACT_SCHEMA,
                "kind": population.ARTIFACT_KIND,
                "orchestration_sha256": evidence_sha,
                "files": files,
            }
            artifact["manifest_sha256"] = combined.canonical_digest(artifact)
            observed = combined.validate_population_artifact(
                artifact,
                population_evidence_sha256=evidence_sha,
                population_dir=root,
            )
            self.assertEqual(observed, artifact["manifest_sha256"])

            binary.write_bytes(b"tampered")
            with self.assertRaises(combined.CombinedEvidenceError):
                combined.validate_population_artifact(
                    artifact,
                    population_evidence_sha256=evidence_sha,
                    population_dir=root,
                )


class DecisionFirewallTests(unittest.TestCase):
    def test_pareto_record_is_always_required(self) -> None:
        self.assertEqual(
            combined.decision_blockers(True, True),
            ["dimension-separated Pareto selection record not assembled"],
        )

    def test_measurement_failures_are_preserved_as_separate_blockers(self) -> None:
        blockers = combined.decision_blockers(False, False)
        self.assertIn("population evidence did not pass protocol/noise eligibility", blockers)
        self.assertIn("synthetic mechanism evidence did not pass protocol/noise eligibility", blockers)
        self.assertIn("dimension-separated Pareto selection record not assembled", blockers)

    def test_combined_artifact_manifest_is_content_addressed_and_excludes_itself(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "combined-orchestration.json").write_text("{}\n", encoding="utf-8")
            nested = root / "synthetic" / "round-00"
            nested.mkdir(parents=True)
            (nested / "direct.json").write_text(json.dumps({"ok": True}) + "\n", encoding="utf-8")
            manifest = combined.artifact_manifest(root, "f" * 64)
            self.assertEqual(manifest["kind"], combined.ARTIFACT_KIND)
            self.assertEqual(manifest["combined_evidence_sha256"], "f" * 64)
            paths = {entry["path"] for entry in manifest["files"]}
            self.assertEqual(
                paths,
                {"combined-orchestration.json", "synthetic/round-00/direct.json"},
            )
            expected = combined.canonical_digest(
                {key: value for key, value in manifest.items() if key != "manifest_sha256"}
            )
            self.assertEqual(manifest["manifest_sha256"], expected)


if __name__ == "__main__":
    unittest.main()
