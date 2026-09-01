from __future__ import annotations

import json
import tarfile
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from tools import r2c_world_state_source_review_bundle as bundle


def write_fake_discovery(path: Path, plan: Path, frontier: Path) -> None:
    path.write_text(
        json.dumps(
            {
                "inputs": {
                    "plan_sha256": bundle._sha256_file(plan),
                    "frontier_sha256": bundle._sha256_file(frontier),
                },
                "source": {
                    "archive_sha256": bundle.packer.EXPECTED_SOURCE_SHA256,
                },
            }
        )
        + "\n",
        encoding="utf-8",
    )


class R2cWorldStateSourceReviewBundleTests(unittest.TestCase):
    def test_source_rich_output_inside_repository_is_rejected(self) -> None:
        output = bundle.REPO_ROOT / "r2c-source-review-should-not-exist.tar.gz"
        with self.assertRaises(bundle.BundleError):
            bundle._external_output(output)

    def test_bundle_archives_discovery_and_focused_review_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "bundle.tar.gz"
            plan = root / "plan.json"
            frontier = root / "frontier.json"
            plan.write_text("fixture plan\n", encoding="utf-8")
            frontier.write_text("fixture frontier\n", encoding="utf-8")

            def fake_discovery_prepare(
                output_dir: Path,
                plan_path: Path,
                db: Path,
                source: Path,
                lock: Path,
            ) -> dict[str, object]:
                del db, source, lock
                self.assertEqual(plan_path, plan)
                output_dir.mkdir()
                write_fake_discovery(output_dir / "discovery.json", plan, frontier)
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
                mock.patch.object(bundle.discovery, "_load_plan", return_value=SimpleNamespace(frontier=frontier)),
                mock.patch.object(bundle.packer, "build", side_effect=fake_review_build),
            ):
                result = bundle.build_bundle(
                    output=output,
                    db=root / "atlas.sqlite",
                    source=root / "mc-src.zip",
                    lock=root / "vanilla.lock.toml",
                    plan=plan,
                )

            self.assertTrue(output.is_file())
            self.assertEqual(result["output"], str(output.resolve()))
            self.assertEqual(result["discovery_sha256"], "d" * 64)
            self.assertEqual(result["review_pack_sha256"], "r" * 64)
            self.assertEqual(result["worksheet_sha256"], "w" * 64)
            self.assertEqual(result["unique_candidate_methods"], 7)
            self.assertEqual(result["unique_source_records"], 5)
            self.assertEqual(result["source_excerpt_bytes"], 1234)
            self.assertEqual(result["archive_regular_files"], 5)
            self.assertEqual(result["plan_sha256"], bundle._sha256_file(plan))
            self.assertEqual(result["frontier_sha256"], bundle._sha256_file(frontier))
            self.assertFalse(result["production_admitted"])
            self.assertTrue(result["contains_official_source_text"])

            with tarfile.open(output, mode="r:gz") as archive:
                names = set(archive.getnames())
                manifest_member = archive.extractfile(bundle.BUNDLE_MANIFEST_NAME)
                self.assertIsNotNone(manifest_member)
                manifest = json.loads(manifest_member.read()) if manifest_member is not None else {}
            self.assertIn(bundle.BUNDLE_MANIFEST_NAME, names)
            self.assertIn("discovery/discovery.json", names)
            self.assertIn("world-state-review/review-pack.json", names)
            self.assertIn("world-state-review/worksheet.json", names)
            self.assertIn("world-state-review/manifest.json", names)
            self.assertEqual(manifest["kind"], bundle.BUNDLE_MANIFEST_KIND)
            self.assertEqual(manifest["plan_sha256"], bundle._sha256_file(plan))
            self.assertEqual(manifest["frontier_sha256"], bundle._sha256_file(frontier))
            self.assertEqual(manifest["discovery_sha256"], "d" * 64)
            self.assertFalse(manifest["contains_official_source_text"])
            self.assertFalse(manifest["production_admitted"])

    def test_packaging_failure_never_publishes_partial_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "bundle.tar.gz"
            plan = root / "plan.json"
            frontier = root / "frontier.json"
            plan.write_text("fixture plan\n", encoding="utf-8")
            frontier.write_text("fixture frontier\n", encoding="utf-8")

            def fake_discovery_prepare(
                output_dir: Path,
                plan_path: Path,
                db: Path,
                source: Path,
                lock: Path,
            ) -> dict[str, object]:
                del db, source, lock
                self.assertEqual(plan_path, plan)
                output_dir.mkdir()
                write_fake_discovery(output_dir / "discovery.json", plan, frontier)
                return {
                    "discovery_sha256": "d" * 64,
                    "unique_candidate_methods": 1,
                }

            def fake_review_build(
                discovery_path: Path,
                source: Path,
                lock: Path,
                output_dir: Path,
            ) -> dict[str, object]:
                del discovery_path, source, lock
                output_dir.mkdir()
                (output_dir / "review-pack.json").write_text("{}\n", encoding="utf-8")
                (output_dir / "worksheet.json").write_text("{}\n", encoding="utf-8")
                (output_dir / "manifest.json").write_text("{}\n", encoding="utf-8")
                return {
                    "review_pack_sha256": "r" * 64,
                    "worksheet_sha256": "w" * 64,
                    "unique_source_records": 1,
                    "source_excerpt_bytes": 1,
                }

            original_add = tarfile.TarFile.add
            calls = 0

            def failing_add(self: tarfile.TarFile, *args: object, **kwargs: object) -> None:
                nonlocal calls
                calls += 1
                if calls == 1:
                    raise OSError("synthetic packaging failure")
                original_add(self, *args, **kwargs)

            with (
                mock.patch.object(bundle.discovery, "prepare", side_effect=fake_discovery_prepare),
                mock.patch.object(bundle.discovery, "_load_plan", return_value=SimpleNamespace(frontier=frontier)),
                mock.patch.object(bundle.packer, "build", side_effect=fake_review_build),
                mock.patch.object(tarfile.TarFile, "add", new=failing_add),
                self.assertRaisesRegex(OSError, "synthetic packaging failure"),
            ):
                bundle.build_bundle(
                    output=output,
                    db=root / "atlas.sqlite",
                    source=root / "mc-src.zip",
                    lock=root / "vanilla.lock.toml",
                    plan=plan,
                )

            self.assertFalse(output.exists())

    def test_provenance_rejects_stale_plan_and_frontier(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            discovery_path = root / "discovery.json"
            plan = root / "plan.json"
            frontier = root / "frontier.json"
            plan.write_text("plan-v1\n", encoding="utf-8")
            frontier.write_text("frontier-v1\n", encoding="utf-8")
            write_fake_discovery(discovery_path, plan, frontier)

            with mock.patch.object(
                bundle.discovery,
                "_load_plan",
                return_value=SimpleNamespace(frontier=frontier),
            ):
                first = bundle._verify_discovery_provenance(discovery_path, plan)
                self.assertEqual(first["plan_sha256"], bundle._sha256_file(plan))
                self.assertEqual(first["frontier_sha256"], bundle._sha256_file(frontier))

                plan.write_text("plan-v2\n", encoding="utf-8")
                with self.assertRaisesRegex(bundle.BundleError, "plan changed"):
                    bundle._verify_discovery_provenance(discovery_path, plan)

                plan.write_text("plan-v1\n", encoding="utf-8")
                frontier.write_text("frontier-v2\n", encoding="utf-8")
                with self.assertRaisesRegex(bundle.BundleError, "frontier changed"):
                    bundle._verify_discovery_provenance(discovery_path, plan)

    def test_verify_archive_rejects_empty_tar(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "empty.tar.gz"
            with tarfile.open(output, mode="w:gz"):
                pass
            with self.assertRaisesRegex(bundle.BundleError, "missing members"):
                bundle._verify_archive(output)


if __name__ == "__main__":
    unittest.main()
