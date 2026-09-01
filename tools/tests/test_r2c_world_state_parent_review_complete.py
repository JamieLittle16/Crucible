from __future__ import annotations

import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r2c_world_state_parent_review_complete as complete_review


class ParentReviewCompletionTests(unittest.TestCase):
    @staticmethod
    def _write_bundle(
        path: Path,
        *,
        extra_file: str | None = None,
        replace_regular_with_symlink: str | None = None,
    ) -> None:
        payloads = {
            complete_review.BUNDLE_MANIFEST: b"{}\n",
            complete_review.DISCOVERY: b"{}\n",
            complete_review.REVIEW_PACK: b"{}\n",
            complete_review.WORKSHEET: b"{}\n",
            complete_review.REVIEW_MANIFEST: b"{}\n",
        }
        if extra_file is not None:
            payloads[extra_file] = b"unexpected\n"
        with tarfile.open(path, mode="w:gz") as archive:
            for directory in sorted(complete_review.ALLOWED_DIRECTORIES):
                info = tarfile.TarInfo(directory)
                info.type = tarfile.DIRTYPE
                archive.addfile(info)
            for name, raw in payloads.items():
                if name == replace_regular_with_symlink:
                    info = tarfile.TarInfo(name)
                    info.type = tarfile.SYMTYPE
                    info.linkname = "/tmp/escape"
                    archive.addfile(info)
                    continue
                info = tarfile.TarInfo(name)
                info.size = len(raw)
                archive.addfile(info, io.BytesIO(raw))

    def test_materializes_only_exact_canonical_regular_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundle = root / "bundle.tar.gz"
            extract = root / "extract"
            extract.mkdir()
            self._write_bundle(bundle)
            paths = complete_review._materialize_bundle(bundle, extract)
            self.assertEqual(set(paths), set(complete_review.BUNDLE_REGULAR_FILES))
            self.assertEqual(
                {path.relative_to(extract).as_posix() for path in paths.values()},
                set(complete_review.BUNDLE_REGULAR_FILES),
            )

    def test_rejects_extra_regular_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundle = root / "bundle.tar.gz"
            extract = root / "extract"
            extract.mkdir()
            self._write_bundle(bundle, extra_file="world-state-review/source.java")
            with self.assertRaisesRegex(complete_review.CompleteError, "unexpected parent review bundle file"):
                complete_review._materialize_bundle(bundle, extract)

    def test_rejects_symlink_member(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundle = root / "bundle.tar.gz"
            extract = root / "extract"
            extract.mkdir()
            self._write_bundle(bundle, replace_regular_with_symlink=complete_review.REVIEW_PACK)
            with self.assertRaisesRegex(complete_review.CompleteError, "must be a regular file"):
                complete_review._materialize_bundle(bundle, extract)

    def test_complete_publishes_only_source_free_outputs_atomically(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            review_pack = root / "review-pack.json"
            worksheet = root / "worksheet.json"
            manifest = root / "bundle-manifest.json"
            decisions = root / "decisions.json"
            for path in (review_pack, worksheet, manifest, decisions):
                path.write_text("{}\n", encoding="utf-8")
            output = root / "output"

            def fake_apply(**kwargs):
                Path(kwargs["output"]).write_text(
                    json.dumps({"completed": True}) + "\n", encoding="utf-8"
                )
                return {"sha256": "a" * 64, "rejected_sources": 7}

            def fake_finalize(_pack, _worksheet, result):
                Path(result).write_text(
                    json.dumps({"all_groups_review_complete": True}) + "\n",
                    encoding="utf-8",
                )
                return {"sha256": "b" * 64, "selected_sources": 5, "groups": 3}

            with (
                mock.patch.object(complete_review.apply_review, "apply", side_effect=fake_apply),
                mock.patch.object(complete_review.finalize_review, "finalize", side_effect=fake_finalize),
            ):
                summary = complete_review.complete(
                    review_pack=review_pack,
                    worksheet=worksheet,
                    bundle_manifest=manifest,
                    decisions=decisions,
                    output_dir=output,
                )

            self.assertEqual(sorted(path.name for path in output.iterdir()), [
                complete_review.COMPLETED_WORKSHEET,
                complete_review.REVIEW_RESULT,
            ])
            self.assertEqual(summary["selected_sources"], 5)
            self.assertEqual(summary["rejected_sources"], 7)
            self.assertFalse(summary["contains_official_source_text"])
            self.assertFalse(summary["production_admitted"])

    def test_failure_does_not_publish_partial_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            review_pack = root / "review-pack.json"
            worksheet = root / "worksheet.json"
            manifest = root / "bundle-manifest.json"
            decisions = root / "decisions.json"
            for path in (review_pack, worksheet, manifest, decisions):
                path.write_text("{}\n", encoding="utf-8")
            output = root / "output"

            with mock.patch.object(
                complete_review.apply_review,
                "apply",
                side_effect=complete_review.apply_review.ApplyError("decision mismatch"),
            ):
                with self.assertRaisesRegex(complete_review.CompleteError, "decision mismatch"):
                    complete_review.complete(
                        review_pack=review_pack,
                        worksheet=worksheet,
                        bundle_manifest=manifest,
                        decisions=decisions,
                        output_dir=output,
                    )
            self.assertFalse(output.exists())

    def test_source_rich_field_in_output_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            review_pack = root / "review-pack.json"
            worksheet = root / "worksheet.json"
            manifest = root / "bundle-manifest.json"
            decisions = root / "decisions.json"
            for path in (review_pack, worksheet, manifest, decisions):
                path.write_text("{}\n", encoding="utf-8")
            output = root / "output"

            def fake_apply(**kwargs):
                Path(kwargs["output"]).write_text(
                    json.dumps({"source_excerpt": "forbidden"}) + "\n", encoding="utf-8"
                )
                return {"sha256": "a" * 64, "rejected_sources": 0}

            def fake_finalize(_pack, _worksheet, result):
                Path(result).write_text("{}\n", encoding="utf-8")
                return {"sha256": "b" * 64, "selected_sources": 1, "groups": 3}

            with (
                mock.patch.object(complete_review.apply_review, "apply", side_effect=fake_apply),
                mock.patch.object(complete_review.finalize_review, "finalize", side_effect=fake_finalize),
            ):
                with self.assertRaisesRegex(complete_review.CompleteError, "source-rich field leaked"):
                    complete_review.complete(
                        review_pack=review_pack,
                        worksheet=worksheet,
                        bundle_manifest=manifest,
                        decisions=decisions,
                        output_dir=output,
                    )
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
