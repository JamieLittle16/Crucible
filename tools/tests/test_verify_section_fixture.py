from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "tools" / "verify_section_fixture.py"
SPEC = importlib.util.spec_from_file_location("crucible_verify_section_fixture", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
verification = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verification
SPEC.loader.exec_module(verification)

FIXTURE = ROOT / "vanilla/fixtures/world/section/26.2-source-reviewed-count-gates.fixture"


class SectionFixtureVerificationTests(unittest.TestCase):
    def test_committed_source_fixture_matches_repository_evidence(self) -> None:
        verification.verify(ROOT, FIXTURE)

    def test_tampered_source_digest_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "tampered.fixture"
            text = FIXTURE.read_text(encoding="utf-8").replace(
                "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750",
                "0e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750",
                1,
            )
            path.write_text(text, encoding="utf-8")
            with self.assertRaises(verification.FixtureError):
                verification.verify(ROOT, path)


if __name__ == "__main__":
    unittest.main()
