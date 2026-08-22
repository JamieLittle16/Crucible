from __future__ import annotations

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import representative_set_artifact_manifest as artifact


def canonical_digest(value: object) -> str:
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode()
    ).hexdigest()


class RepresentativeSetArtifactManifestTests(unittest.TestCase):
    def provenance(self) -> dict[str, object]:
        return {
            "repository_commit_sha": "a" * 40,
            "github_run_id": "123",
            "github_run_attempt": "2",
            "python_version": "3.test",
            "rustc_version": "rustc test",
            "java_version": "java test",
        }

    def write_complete(self, root: Path) -> None:
        set_bytes = b'{"kind":"section-corpus-set"}\n'
        (root / "corpus-set.json").write_bytes(set_bytes)
        admission = {
            "schema": 1,
            "kind": "section-representative-set-admission",
            "population_sha256": "1" * 64,
            "set_evidence_sha256": "2" * 64,
            "set_file_sha256": hashlib.sha256(set_bytes).hexdigest(),
            "member_count": 4,
            "decision_eligible": True,
            "benchmark_handoff_eligible": True,
            "decision_scope": "dimension-separated-only",
            "cross_dimension_score_allowed": False,
        }
        admission["admission_sha256"] = canonical_digest(admission)
        (root / "population-admission.json").write_text(
            json.dumps(admission, sort_keys=True) + "\n", encoding="utf-8"
        )

    def test_complete_artifact_is_explicitly_handoff_eligible(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_complete(root)
            result = artifact.build_manifest(root, provenance=self.provenance())
        self.assertTrue(result["qualification_complete"])
        self.assertTrue(result["decision_eligible"])
        self.assertTrue(result["benchmark_handoff_eligible"])
        self.assertEqual(result["schema"], 2)
        self.assertEqual(result["provenance"]["repository_commit_sha"], "a" * 40)
        self.assertEqual(len(result["identities"]["admission_sha256"]), 64)
        self.assertEqual(len(result["manifest_sha256"]), 64)

    def test_partial_diagnostic_artifact_cannot_masquerade_as_qualified(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "seed-0").mkdir()
            (root / "seed-0" / "server.log").write_text("failure\n")
            result = artifact.build_manifest(root, provenance=self.provenance())
        self.assertFalse(result["qualification_complete"])
        self.assertFalse(result["decision_eligible"])
        self.assertFalse(result["benchmark_handoff_eligible"])
        self.assertEqual(result["identities"], {})

    def test_set_file_change_after_admission_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_complete(root)
            (root / "corpus-set.json").write_text("tampered\n")
            with self.assertRaises(artifact.ArtifactManifestError):
                artifact.build_manifest(root, provenance=self.provenance())

    def test_admission_record_digest_is_reverified(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_complete(root)
            path = root / "population-admission.json"
            admission = json.loads(path.read_text())
            admission["member_count"] = 3
            path.write_text(json.dumps(admission) + "\n")
            with self.assertRaises(artifact.ArtifactManifestError):
                artifact.build_manifest(root, provenance=self.provenance())

    def test_manifest_excludes_itself_from_file_inventory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_complete(root)
            (root / "artifact-manifest.json").write_text("old\n")
            result = artifact.build_manifest(root, provenance=self.provenance())
        self.assertNotIn(
            "artifact-manifest.json", {entry["path"] for entry in result["files"]}
        )


if __name__ == "__main__":
    unittest.main()
