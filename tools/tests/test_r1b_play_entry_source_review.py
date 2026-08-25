from __future__ import annotations

import json
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

from tools import r1b_play_entry_source_review as review


class _Connection:
    def close(self) -> None:
        pass


class R1BPlayEntrySourceReviewTests(unittest.TestCase):
    def test_plan_is_bounded_and_effective_selectors_are_unique(self) -> None:
        ids = [candidate.var_id for candidate in review.CANDIDATES]
        selectors = [review.selector_key(candidate) for candidate in review.CANDIDATES]
        self.assertEqual(len(ids), 27)
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(len(selectors), len(set(selectors)))
        self.assertEqual(
            set(review.ROUTE_DISPOSITIONS),
            {
                "MANDATORY",
                "CONDITIONAL",
                "DEFAULT_EMPTY",
                "INTERNAL_ONLY",
                "DELEGATED_REVIEW_REQUIRED",
            },
        )

    def test_plan_contains_control_helpers_registration_and_direct_packets(self) -> None:
        ids = {candidate.var_id for candidate in review.CANDIDATES}
        required = {
            "DISC-NET-R1B-PLAY-GAME-PROTOCOLS-001",
            "DISC-NET-R1B-PLAY-PLACE-NEW-PLAYER-001",
            "DISC-NET-R1B-PLAY-PERMISSION-ENTRY-001",
            "DISC-NET-R1B-PLAY-RECIPE-BOOK-001",
            "DISC-NET-R1B-PLAY-SCOREBOARD-001",
            "DISC-NET-R1B-PLAY-LEVEL-INFO-001",
            "DISC-NET-R1B-PLAY-TELEPORT-SEND-001",
            "DISC-NET-R1B-PLAY-LOGIN-CODEC-001",
            "DISC-NET-R1B-PLAY-ABILITIES-WRITE-001",
            "DISC-NET-R1B-PLAY-PLAYER-INFO-INITIAL-001",
            "DISC-NET-R1B-PLAY-POSITION-CODEC-001",
        }
        self.assertTrue(required <= ids)
        self.assertNotIn("DISC-NET-R1B-PLAY-POSITION-WRITE-001", ids)

    def test_exact_signature_is_explicit_for_same_arity_abilities_constructors(self) -> None:
        candidate = next(
            candidate
            for candidate in review.CANDIDATES
            if candidate.var_id == "DISC-NET-R1B-PLAY-ABILITIES-FROM-STATE-001"
        )
        self.assertEqual(
            candidate.exact_signature,
            "ClientboundPlayerAbilitiesPacket(final Abilities abilities)",
        )

    def test_source_rich_output_is_rejected_inside_repository(self) -> None:
        with tempfile.TemporaryDirectory(dir=review.REPO_ROOT) as temporary:
            with self.assertRaises(review.PlayEntryReviewError):
                review._external_fresh_dir(Path(temporary) / "play-entry")

    def test_prepare_separates_source_rich_dossier_from_source_free_worksheet(self) -> None:
        candidate = review.Candidate(
            "DISC-X", "example.X", "work", 0, ("Review exact body.",)
        )
        row = {
            "path": "src/X.java",
            "start_line": 2,
            "end_line": 4,
        }
        record = {
            "source": {
                "type": "example.X",
                "signature": "work()",
                "fingerprint_algorithm": "test",
                "normalized_sha256": "a" * 64,
                "body_sha256": "b" * 64,
            },
            "atlas_observed_hazards": ["CLIENT_OBSERVABLE"],
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.zip"
            with zipfile.ZipFile(source, "w") as archive:
                archive.writestr("src/X.java", "class X {\n  void work() {\n    send();\n  }\n}\n")
            output = root / "review"
            with (
                mock.patch.object(review, "CANDIDATES", (candidate,)),
                mock.patch.object(review.vanilla_atlas, "connect_db", return_value=_Connection()),
                mock.patch.object(review.source_probe, "require_pinned_source", return_value="c" * 64),
                mock.patch.object(review, "_resolve_all", return_value=[(candidate, row)]),
                mock.patch.object(review.source_probe, "record_template", return_value=record),
            ):
                manifest = review.prepare(output, root / "atlas.sqlite", source, root / "lock.json")

            dossier_raw = (output / "review-dossier.json").read_text(encoding="utf-8")
            worksheet_raw = (output / "review-worksheet.json").read_text(encoding="utf-8")
            dossier = json.loads(dossier_raw)
            worksheet = json.loads(worksheet_raw)
            self.assertEqual(manifest["candidate_count"], 1)
            self.assertTrue(dossier["contains_official_source_text"])
            self.assertIn("void work()", dossier_raw)
            self.assertFalse(worksheet["contains_official_source_text"])
            self.assertNotIn("void work()", worksheet_raw)
            decision = worksheet["candidates"][0]["decision"]
            self.assertEqual(decision["route_disposition"], "")
            self.assertFalse(decision["source_inspected"])
            self.assertFalse(decision["accepted"])

    def test_prepare_failure_removes_partial_source_rich_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "review"
            with (
                mock.patch.object(review.vanilla_atlas, "connect_db", return_value=_Connection()),
                mock.patch.object(
                    review.source_probe,
                    "require_pinned_source",
                    side_effect=review.source_probe.ProbeError("pin mismatch"),
                ),
            ):
                with self.assertRaises(review.source_probe.ProbeError):
                    review.prepare(
                        output,
                        root / "atlas.sqlite",
                        root / "source.zip",
                        root / "lock.json",
                    )
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
