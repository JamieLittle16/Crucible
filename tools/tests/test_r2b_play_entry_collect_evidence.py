from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import r2b_play_entry_collect_evidence as collect


class R2BPlayEntryCollectEvidenceTests(unittest.TestCase):
    def test_repo_local_output_is_rejected_before_collection(self) -> None:
        path = collect.REPO_ROOT / ".helve-test-r2b-evidence"
        with self.assertRaisesRegex(collect.CollectError, "outside the repository"):
            collect._external_fresh_dir(path)

    def test_existing_output_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            with self.assertRaisesRegex(collect.CollectError, "must not already exist"):
                collect._external_fresh_dir(Path(td))

    def test_combined_manifest_preserves_evidence_trust_boundaries(self) -> None:
        seams_manifest = {
            "id": "REVIEW-NET-R2B-PLAY-FINAL-SEAMS-26_2-001",
            "candidate_count": 7,
            "group_counts": {"GENERIC_REGISTRY_WIRE": 4, "GLOBAL_POS_WIRE": 3},
        }
        oracle_value = {
            "target": {"capture_sha256": "a" * 64},
            "artifacts": [{"name": "commands"}, {"name": "update-recipes"}],
        }
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            output = root / "bundle"
            replay = root / "replay.json"
            replay.write_text("{}\n", encoding="utf-8")
            prior = root / "prior.json"
            prior.write_text("{}\n", encoding="utf-8")
            with (
                patch.object(collect.final_seams, "prepare", return_value=seams_manifest) as prepare,
                patch.object(collect.oracle_extract, "_read", return_value={}) as read,
                patch.object(collect.oracle_extract, "extract", return_value=oracle_value) as extract,
                patch.object(collect.oracle_extract, "write") as write,
            ):
                manifest = collect.collect(
                    output_dir=output,
                    prior_117_dossier=prior,
                    r1x_replay=replay,
                    db=root / "atlas.sqlite",
                    source=root / "mc-src.zip",
                    lock=root / "vanilla.lock.toml",
                )

            prepare.assert_called_once()
            read.assert_called_once_with(replay)
            extract.assert_called_once_with({})
            write.assert_called_once_with(output / "composition-oracle.json", oracle_value)
            self.assertEqual(manifest["commit_policy"], "EPHEMERAL_DO_NOT_COMMIT")
            self.assertIs(manifest["contains_official_source_text"], True)
            self.assertIs(manifest["production_admitted"], False)
            self.assertEqual(
                manifest["source_review"]["trust"],
                "SOURCE_RICH_REQUIRES_HUMAN_REVIEW",
            )
            self.assertEqual(
                manifest["composition_oracle"]["trust"],
                "BLACK_BOX_CONFIRMATION_ONLY",
            )
            committed = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(committed, manifest)

    def test_partial_output_is_removed_on_failure(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            output = root / "bundle"
            with (
                patch.object(
                    collect.final_seams,
                    "prepare",
                    return_value={
                        "id": "review",
                        "candidate_count": 1,
                        "group_counts": {"GENERIC_REGISTRY_WIRE": 1},
                    },
                ),
                patch.object(collect.oracle_extract, "_read", side_effect=ValueError("bad replay")),
            ):
                with self.assertRaisesRegex(ValueError, "bad replay"):
                    collect.collect(
                        output_dir=output,
                        prior_117_dossier=root / "prior.json",
                        r1x_replay=root / "replay.json",
                        db=root / "atlas.sqlite",
                        source=root / "mc-src.zip",
                        lock=root / "vanilla.lock.toml",
                    )
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
