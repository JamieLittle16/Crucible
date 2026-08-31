from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import r2c_world_state_admission_promote as promote
from tools.tests.test_r2c_world_state_admission_promote import PromotionFixture


class WorldStateAdmissionAtomicityTests(unittest.TestCase):
    def test_second_write_failure_rolls_back_all_attempted_repository_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            original = promote._write_file_exclusive
            calls = 0

            def fail_on_second(path: Path, raw: bytes) -> None:
                nonlocal calls
                calls += 1
                if calls == 2:
                    raise OSError("injected second-write failure")
                original(path, raw)

            with mock.patch.object(
                promote,
                "_write_file_exclusive",
                side_effect=fail_on_second,
            ):
                with self.assertRaisesRegex(
                    promote.PromoteError,
                    "attempted repository files rolled back",
                ):
                    promote.promote(fixture.staging, fixture.report, fixture.repo)

            expected_absent = [
                fixture.repo / promote.SEMANTICS_PATH,
                fixture.repo / promote.GATE_PATH,
                fixture.repo / promote.REPORT_PATH,
                fixture.repo / promote.RECORD_ROOT / f"{fixture.var_id}.json",
                fixture.repo / promote.MANIFEST_PATH,
            ]
            for path in expected_absent:
                self.assertFalse(path.exists(), f"partial promotion file survived rollback: {path}")


if __name__ == "__main__":
    unittest.main()
