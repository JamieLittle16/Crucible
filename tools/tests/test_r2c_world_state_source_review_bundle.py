from __future__ import annotations

import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r2c_world_state_source_review_bundle as bundle


class R2cWorldStateSourceReviewBundleTests(unittest.TestCase):
    def test_source_rich_output_inside_repository_is_rejected(self) -> None:
        output = bundle.REPO_ROOT / "r2c-source-review-should-not-exist.tar.gz"
        with self.assertRaises(bundle.BundleError):
            bundle._external_output(output)

    def test_bundle_archives_discovery_and_focused_review_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "bundle.tar.gz"

            def fake_discovery_prepare(
                output_dir: Path,
                plan: Path,
                db: Path,
                source: Path,
                lock: Path,
            ) -> dict[str, object]:
                del plan, db, source, lock
                output_dir.mkdir()
                (output_dir / "discovery.json").write_text("{}\n", encoding="utf-8")
                return {
                    "discovery_sha256": "d" * 64,
                    "unique_candidate_methods": 7,
                }

            def fake_review_build(
                discovery_path: Path,
                source: Path,
                lock: Path,
                output_dir: Path,
            ) -> dict[str, object]:
                del source, lock
                self.assertEqual(discovery_path.name, "discovery.json")
                output_dir.mkdir()
                (output_dir / "review-pack.json").write_text("{}\n", encoding="utf-8")
                (output_dir / "worksheet.json").write_text("{}\n", encoding="utf-8")
                (output_dir / "manifest.json").write_text("{}\n", encoding="utf-8")
                return {
                    "review_pack_sha256": "r" * 64,
                    "worksheet_sha256": "w" * 64,
                    "unique_source_records": 5,
                    "source_excerpt_bytes": 1234,
                }

            with (
                mock.patch.object(bundle.discovery, "prepare", side_effect=fake_discovery_prepare),
                mock.patch.object(bundle.packer, "build", side_effect=fake_review_build),
            ):
                result = bundle.build_bundle(
                    output=output,
                    db=root / "atlas.sqlite",
                    source=root / "mc-src.zip",
                    lock=root / "vanilla.lock.toml",
                    plan=root / "plan.json",
                )

            self.assertTrue(output.is_file())
            self.assertEqual(result["output"], str(output.resolve()))
            self.assertEqual(result["discovery_sha256"], "d" * 64)
            self.assertEqual(result["review_pack_sha256"], "r" * 64)
            self.assertEqual(result["worksheet_sha256"], "w" * 64)
            self.assertEqual(result["unique_candidate_methods"], 7)
            self.assertEqual(result["unique_source_records"], 5)
            self.assertEqual(result["source_excerpt_bytes"], 1234)
            self.assertFalse(result["production_admitted"])
            self.assertTrue(result["contains_official_source_text"])

            with tarfile.open(output, mode="r:gz") as archive:
                names = set(archive.getnames())
            self.assertIn("discovery/discovery.json", names)
            self.assertIn("world-state-review/review-pack.json", names)
            self.assertIn("world-state-review/worksheet.json", names)
            self.assertIn("world-state-review/manifest.json", names)


if __name__ == "__main__":
    unittest.main()
