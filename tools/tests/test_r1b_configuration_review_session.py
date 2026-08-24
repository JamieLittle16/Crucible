from __future__ import annotations

import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

from tools import r1b_configuration_review_session as session


REPO_ROOT = Path(__file__).resolve().parents[2]
SOURCE_SENTINEL = "OFFICIAL-SOURCE-SENTINEL"


def fake_source_probe_run(_db, _source, _frontier, _lock, bundle_output) -> int:
    assert bundle_output is not None
    Path(bundle_output).write_text(
        json.dumps({"synthetic": True, "source": SOURCE_SENTINEL}) + "\n",
        encoding="utf-8",
    )
    print(SOURCE_SENTINEL)
    return 0


def fake_materialize_review_pack(*, output_dir, **_kwargs):
    output_dir.mkdir()
    (output_dir / "manifest.json").write_text(
        json.dumps({"contains_official_source_text": False}) + "\n",
        encoding="utf-8",
    )
    return {"contains_official_source_text": False}


def fake_build_dossier(**_kwargs):
    return {
        "contains_official_source_text": True,
        "source_excerpt": SOURCE_SENTINEL,
    }


def fake_write_dossier(path, value) -> None:
    path.write_text(json.dumps(value) + "\n", encoding="utf-8")


def fake_prepare_worksheet(*, output, **_kwargs):
    value = {
        "contains_official_source_text": False,
        "candidates": [
            {"var_id": var_id, "decision": {"source_inspected": None, "accepted": None}}
            for var_id, _query in session.source_probe.CANDIDATES
        ],
    }
    output.write_text(json.dumps(value) + "\n", encoding="utf-8")
    return value


class PipelinePatches:
    def __enter__(self):
        self.stack = mock.patch.object(session.source_probe, "run", side_effect=fake_source_probe_run)
        self.source = self.stack.__enter__()
        self.pack_patch = mock.patch.object(
            session.bundle_review,
            "materialize_review_pack",
            side_effect=fake_materialize_review_pack,
        )
        self.pack = self.pack_patch.__enter__()
        self.dossier_build_patch = mock.patch.object(
            session.dossier,
            "build_dossier",
            side_effect=fake_build_dossier,
        )
        self.dossier_build = self.dossier_build_patch.__enter__()
        self.dossier_write_patch = mock.patch.object(
            session.dossier,
            "write_dossier",
            side_effect=fake_write_dossier,
        )
        self.dossier_write = self.dossier_write_patch.__enter__()
        self.prepare_patch = mock.patch.object(
            session.review,
            "prepare_worksheet",
            side_effect=fake_prepare_worksheet,
        )
        self.prepare = self.prepare_patch.__enter__()
        return self

    def __exit__(self, exc_type, exc, tb):
        self.prepare_patch.__exit__(exc_type, exc, tb)
        self.dossier_write_patch.__exit__(exc_type, exc, tb)
        self.dossier_build_patch.__exit__(exc_type, exc, tb)
        self.pack_patch.__exit__(exc_type, exc, tb)
        self.stack.__exit__(exc_type, exc, tb)
        return False


class R1BConfigurationReviewSessionTests(unittest.TestCase):
    def test_build_session_sequences_existing_tools_and_separates_source_rich_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, PipelinePatches():
            output = Path(temporary) / "review-session"
            manifest = session.build_session(output_dir=output, repo_root=REPO_ROOT)

            self.assertEqual(manifest["kind"], session.SESSION_KIND)
            self.assertEqual(manifest["commit_policy"], "EPHEMERAL_DO_NOT_COMMIT")
            self.assertTrue(manifest["contains_official_source_text"])
            self.assertEqual(manifest["candidate_count"], len(session.source_probe.CANDIDATES))

            self.assertIn(SOURCE_SENTINEL, (output / "source-probe.txt").read_text())
            self.assertIn(SOURCE_SENTINEL, (output / "admission-bundle.json").read_text())
            self.assertIn(SOURCE_SENTINEL, (output / "review-dossier.json").read_text())
            self.assertNotIn(SOURCE_SENTINEL, (output / "review-worksheet.json").read_text())
            self.assertNotIn(
                SOURCE_SENTINEL,
                (output / "review-pack" / "manifest.json").read_text(),
            )
            self.assertTrue((output / "session-manifest.json").is_file())

    def test_session_output_must_be_outside_repository(self) -> None:
        output = REPO_ROOT / "target" / "r1b-review-session-test-never-create"
        with self.assertRaisesRegex(session.ReviewSessionError, "outside the Git repository"):
            session.build_session(output_dir=output, repo_root=REPO_ROOT)
        self.assertFalse(output.exists())

    def test_existing_session_directory_is_rejected_without_modification(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "existing"
            output.mkdir()
            marker = output / "keep.txt"
            marker.write_text("keep", encoding="utf-8")
            with self.assertRaisesRegex(session.ReviewSessionError, "must not already exist"):
                session.build_session(output_dir=output, repo_root=REPO_ROOT)
            self.assertEqual(marker.read_text(encoding="utf-8"), "keep")

    def test_component_failure_removes_only_the_fresh_partial_session(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "partial"
            with mock.patch.object(
                session.source_probe,
                "run",
                side_effect=RuntimeError("synthetic source failure"),
            ):
                with self.assertRaisesRegex(RuntimeError, "synthetic source failure"):
                    session.build_session(output_dir=output, repo_root=REPO_ROOT)
            self.assertFalse(output.exists())

    def test_cli_stdout_never_prints_source_text(self) -> None:
        with tempfile.TemporaryDirectory() as temporary, PipelinePatches():
            output = Path(temporary) / "cli-session"
            stdout = io.StringIO()
            with redirect_stdout(stdout):
                status = session.main(["--output-dir", str(output)])
            rendered = stdout.getvalue()
            self.assertEqual(status, 0)
            self.assertNotIn(SOURCE_SENTINEL, rendered)
            self.assertIn("next_finalize_command=", rendered)
            self.assertIn("next_source_gate_command=", rendered)
            self.assertIn("EPHEMERAL_DO_NOT_COMMIT", rendered)


if __name__ == "__main__":
    unittest.main()
