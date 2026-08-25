from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools import milestone_release_notes


class MilestoneReleaseNotesTests(unittest.TestCase):
    def test_known_style_tag_resolves_real_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            expected = root / "milestone-r2a-live-play-liveness.md"
            expected.write_text("notes\n", encoding="utf-8")
            self.assertEqual(
                milestone_release_notes.resolve_notes(
                    "milestone-r2a-live-play-liveness", notes_root=root
                ),
                expected,
            )

    def test_unknown_tag_fails_closed_when_notes_are_missing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(milestone_release_notes.MilestoneReleaseError):
                milestone_release_notes.resolve_notes(
                    "milestone-r9-unknown", notes_root=Path(temporary)
                )

    def test_noncanonical_tag_syntax_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for tag in (
                "milestone-R2A-live",
                "milestone-r2a/live",
                "milestone-../r2a",
                "milestone-",
                "release-r2a",
            ):
                with self.subTest(tag=tag):
                    with self.assertRaises(milestone_release_notes.MilestoneReleaseError):
                        milestone_release_notes.resolve_notes(tag, notes_root=root)

    def test_symlink_notes_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / "target.md"
            target.write_text("notes\n", encoding="utf-8")
            link = root / "milestone-r2a-live-play-liveness.md"
            link.symlink_to(target)
            with self.assertRaises(milestone_release_notes.MilestoneReleaseError):
                milestone_release_notes.resolve_notes(
                    "milestone-r2a-live-play-liveness", notes_root=root
                )


if __name__ == "__main__":
    unittest.main()
