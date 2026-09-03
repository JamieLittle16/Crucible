from __future__ import annotations

import json
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r2c_world_state_local_admission as admission


class R2cWorldStateLocalAdmissionTests(unittest.TestCase):
    def _fake_pipeline(self, root: Path, *, admitted: bool = True):
        parent_decisions = root / "parent-decisions.json"
        semantic_decisions = root / "semantic-decisions.json"
        parent_decisions.write_text('{"decision":"parent"}\n', encoding="utf-8")
        semantic_decisions.write_text('{"decision":"semantic"}\n', encoding="utf-8")

        def fake_source_bundle(**kwargs: object) -> dict[str, object]:
            output = Path(str(kwargs["output"]))
            output.write_text("SOURCE-RICH source_excerpt MUST NOT ESCAPE\n", encoding="utf-8")
            return {
                "bundle_manifest_sha256": "b" * 64,
            }

        def fake_parent_complete(
            bundle: Path,
            decisions: Path,
            output_dir: Path,
        ) -> dict[str, object]:
            self.assertEqual(decisions, parent_decisions)
            self.assertIn("source_excerpt", bundle.read_text(encoding="utf-8"))
            output_dir.mkdir()
            result = output_dir / admission.parent_complete.REVIEW_RESULT
            result.write_text(
                '{"contains_official_source_text":false,"kind":"parent-review"}\n',
                encoding="utf-8",
            )
            return {
                "parent_review_result_sha256": admission._sha256_file(result),
            }

        def fake_prepare(review_result: Path, output: Path) -> dict[str, object]:
            self.assertTrue(review_result.is_file())
            output.write_text(
                '{"contains_official_source_text":false,"kind":"prepared"}\n',
                encoding="utf-8",
            )
            return {"sha256": admission._sha256_file(output)}

        def fake_apply(*, worksheet: Path, decisions: Path, output: Path) -> dict[str, object]:
            self.assertTrue(worksheet.is_file())
            self.assertEqual(decisions, semantic_decisions)
            output.write_text(
                '{"contains_official_source_text":false,"kind":"completed"}\n',
                encoding="utf-8",
            )
            return {"sha256": admission._sha256_file(output)}

        def fake_materialize(
            review_result: Path,
            worksheet: Path,
            output_dir: Path,
        ) -> dict[str, object]:
            self.assertTrue(review_result.is_file())
            self.assertTrue(worksheet.is_file())
            (output_dir / "records").mkdir(parents=True)
            (output_dir / "semantics").mkdir()
            (output_dir / "records/VAR-NET-R2C-WORLD-0001.json").write_text(
                '{"contains_official_source_text":false}\n', encoding="utf-8"
            )
            (output_dir / "semantics/R2C_WORLD_STATE_SEMANTICS.md").write_text(
                "# Source-free semantics\n", encoding="utf-8"
            )
            (output_dir / "gate.json").write_text('{"id":"gate"}\n', encoding="utf-8")
            manifest = output_dir / "manifest.json"
            manifest.write_text('{"contains_official_source_text":false}\n', encoding="utf-8")
            return {
                "manifest_sha256": admission._sha256_file(manifest),
                "var_records": 1,
                "semantic_rules": 2,
            }

        def fake_gate(*, db_path: Path, staging_dir: Path) -> dict[str, object]:
            self.assertEqual(db_path, root / "atlas.sqlite")
            self.assertTrue((staging_dir / "manifest.json").is_file())
            return {
                "admitted": admitted,
                "gate_id": "GATE-NET-R2C-WORLD-STATE-26_2-001",
            }

        patches = (
            mock.patch.object(admission.source_bundle, "build_bundle", side_effect=fake_source_bundle),
            mock.patch.object(admission.parent_complete, "complete_bundle", side_effect=fake_parent_complete),
            mock.patch.object(admission.prepare, "prepare", side_effect=fake_prepare),
            mock.patch.object(admission.admission_apply, "apply", side_effect=fake_apply),
            mock.patch.object(admission.materialize, "materialize", side_effect=fake_materialize),
            mock.patch.object(admission.bound_gate, "evaluate_bound", side_effect=fake_gate),
        )
        return parent_decisions, semantic_decisions, patches

    def test_output_inside_repository_is_rejected(self) -> None:
        output = admission.REPO_ROOT / "r2c-local-admission-should-not-exist.tar.gz"
        with self.assertRaises(admission.LocalAdmissionError):
            admission._fresh_external_output(output)

    def test_source_free_guard_rejects_excerpt_field(self) -> None:
        with self.assertRaisesRegex(admission.LocalAdmissionError, "excerpt field leaked"):
            admission._require_source_free_utf8(
                b'{"source_excerpt":"forbidden"}\n',
                "synthetic report",
            )

    def test_complete_run_publishes_only_source_free_upload_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "admission.tar.gz"
            parent_decisions, semantic_decisions, patches = self._fake_pipeline(root)
            with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
                result = admission.run(
                    output=output,
                    db=root / "atlas.sqlite",
                    source=root / "mc-src.zip",
                    lock=root / "vanilla.lock.toml",
                    plan=root / "plan.json",
                    parent_decisions=parent_decisions,
                    semantic_decisions=semantic_decisions,
                )

            self.assertTrue(result["source_gate_admitted"])
            self.assertEqual(result["var_records"], 1)
            self.assertEqual(result["semantic_rules"], 2)
            self.assertFalse(result["contains_official_source_text"])
            self.assertFalse(result["repository_promotion_performed"])

            with tarfile.open(output, mode="r:gz") as archive:
                names = {member.name for member in archive.getmembers() if member.isfile()}
                texts = []
                for member in archive.getmembers():
                    if not member.isfile():
                        continue
                    stream = archive.extractfile(member)
                    self.assertIsNotNone(stream)
                    if stream is not None:
                        texts.append(stream.read().decode("utf-8"))
                manifest_stream = archive.extractfile(admission.RUN_MANIFEST)
                self.assertIsNotNone(manifest_stream)
                manifest = json.loads(manifest_stream.read()) if manifest_stream is not None else {}

            self.assertIn(admission.RUN_MANIFEST, names)
            self.assertIn(admission.GATE_REPORT, names)
            self.assertIn("staging/manifest.json", names)
            self.assertIn("staging/gate.json", names)
            self.assertIn("staging/records/VAR-NET-R2C-WORLD-0001.json", names)
            self.assertNotIn("source-review.tar.gz", names)
            self.assertTrue(all("source_excerpt" not in text for text in texts))
            self.assertFalse(manifest["contains_official_source_text"])
            self.assertFalse(manifest["repository_promotion_performed"])
            self.assertTrue(manifest["source_gate_admitted"])

    def test_non_admitted_gate_still_publishes_source_free_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "diagnostic.tar.gz"
            parent_decisions, semantic_decisions, patches = self._fake_pipeline(root, admitted=False)
            with patches[0], patches[1], patches[2], patches[3], patches[4], patches[5]:
                result = admission.run(
                    output=output,
                    db=root / "atlas.sqlite",
                    source=root / "mc-src.zip",
                    lock=root / "vanilla.lock.toml",
                    plan=root / "plan.json",
                    parent_decisions=parent_decisions,
                    semantic_decisions=semantic_decisions,
                )
            self.assertFalse(result["source_gate_admitted"])
            self.assertTrue(output.is_file())

    def test_pipeline_failure_leaves_no_final_archive(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "failed.tar.gz"
            parent_decisions = root / "parent-decisions.json"
            semantic_decisions = root / "semantic-decisions.json"
            parent_decisions.write_text("{}\n", encoding="utf-8")
            semantic_decisions.write_text("{}\n", encoding="utf-8")

            with (
                mock.patch.object(
                    admission.source_bundle,
                    "build_bundle",
                    side_effect=admission.source_bundle.BundleError("synthetic failure"),
                ),
                self.assertRaisesRegex(admission.source_bundle.BundleError, "synthetic failure"),
            ):
                admission.run(
                    output=output,
                    db=root / "atlas.sqlite",
                    source=root / "mc-src.zip",
                    lock=root / "vanilla.lock.toml",
                    plan=root / "plan.json",
                    parent_decisions=parent_decisions,
                    semantic_decisions=semantic_decisions,
                )
            self.assertFalse(output.exists())

    def test_packaging_failure_leaves_no_final_archive(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "failed-packaging.tar.gz"
            parent_decisions, semantic_decisions, patches = self._fake_pipeline(root)

            def failing_add(self: tarfile.TarFile, *args: object, **kwargs: object) -> None:
                del self, args, kwargs
                raise OSError("synthetic packaging failure")

            with (
                patches[0],
                patches[1],
                patches[2],
                patches[3],
                patches[4],
                patches[5],
                mock.patch.object(tarfile.TarFile, "add", new=failing_add),
                self.assertRaisesRegex(OSError, "synthetic packaging failure"),
            ):
                admission.run(
                    output=output,
                    db=root / "atlas.sqlite",
                    source=root / "mc-src.zip",
                    lock=root / "vanilla.lock.toml",
                    plan=root / "plan.json",
                    parent_decisions=parent_decisions,
                    semantic_decisions=semantic_decisions,
                )
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
