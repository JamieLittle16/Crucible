from __future__ import annotations

import json
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

from tools import r2_play_liveness_source_review as review


class _Connection:
    def close(self) -> None:
        pass


class R2PlayLivenessSourceReviewTests(unittest.TestCase):
    def test_plan_is_exactly_the_bounded_liveness_frontier(self) -> None:
        ids = [candidate.var_id for candidate in review.CANDIDATES]
        selectors = [review.review_support.selector_key(candidate) for candidate in review.CANDIDATES]
        self.assertEqual(len(ids), 8)
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(len(selectors), len(set(selectors)))
        self.assertEqual(
            set(ids),
            {
                "VAR-NET-R2-PLAY-REGISTRATION-001",
                "VAR-NET-R2-KEEPALIVE-CB-CODEC-001",
                "VAR-NET-R2-KEEPALIVE-SB-CODEC-001",
                "VAR-NET-R2-LIVENESS-CONSTRUCT-001",
                "VAR-NET-R2-LIVENESS-CLOSE-001",
                "VAR-NET-R2-LIVENESS-CLOSED-GATE-001",
                "VAR-NET-R2-LIVENESS-SERVICE-001",
                "VAR-NET-R2-LIVENESS-REPLY-001",
            },
        )

    def test_plan_contains_both_play_registration_and_both_wire_codecs(self) -> None:
        by_id = {candidate.var_id: candidate for candidate in review.CANDIDATES}
        self.assertEqual(
            by_id["VAR-NET-R2-PLAY-REGISTRATION-001"].type_name,
            "net.minecraft.network.protocol.game.GameProtocols",
        )
        self.assertEqual(
            by_id["VAR-NET-R2-KEEPALIVE-CB-CODEC-001"].type_name,
            "net.minecraft.network.protocol.common.ClientboundKeepAlivePacket",
        )
        self.assertEqual(
            by_id["VAR-NET-R2-KEEPALIVE-SB-CODEC-001"].type_name,
            "net.minecraft.network.protocol.common.ServerboundKeepAlivePacket",
        )

    def test_source_rich_output_is_rejected_inside_repository(self) -> None:
        with tempfile.TemporaryDirectory(dir=review.REPO_ROOT) as temporary:
            with self.assertRaises(review.PlayLivenessReviewError):
                review._external_fresh_dir(Path(temporary) / "liveness")

    def test_prepare_separates_source_rich_dossier_from_source_free_worksheet(self) -> None:
        candidate = review.Candidate(
            "VAR-X",
            "example.X",
            "work",
            0,
            ("Review exact body.",),
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
            self.assertFalse(decision["source_inspected"])
            self.assertFalse(decision["accepted"])
            self.assertEqual(decision["semantic_rules"], [])


if __name__ == "__main__":
    unittest.main()
