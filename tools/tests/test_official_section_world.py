from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TOOLS = ROOT / "tools"
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

MODULE_PATH = TOOLS / "official_section_world.py"
SPEC = importlib.util.spec_from_file_location("crucible_official_section_world", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
official_section_world = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = official_section_world
SPEC.loader.exec_module(official_section_world)


class OfficialSectionWorldTests(unittest.TestCase):
    def test_probe_server_properties_are_deterministic_and_bounded(self) -> None:
        first = official_section_world.server_properties(
            official_section_world.DEFAULT_SEED
        )
        second = official_section_world.server_properties(
            official_section_world.DEFAULT_SEED
        )
        self.assertEqual(first, second)
        self.assertTrue(first.endswith("\n"))
        lines = first.splitlines()
        self.assertEqual(lines, sorted(lines))
        self.assertIn(f"level-seed={official_section_world.DEFAULT_SEED}", lines)
        self.assertIn("level-name=world", lines)
        self.assertIn("view-distance=2", lines)
        self.assertIn("simulation-distance=2", lines)
        self.assertIn("online-mode=false", lines)
        self.assertIn("server-ip=127.0.0.1", lines)
        self.assertIn("allow-nether=false", lines)
        self.assertIn("sync-chunk-writes=true", lines)

    def test_probe_seed_and_target_overworld_path_are_stable(self) -> None:
        seed = official_section_world.DEFAULT_SEED
        self.assertTrue(seed.isascii())
        self.assertTrue(seed.isdecimal())
        self.assertEqual(seed, "6842363988700132471")
        self.assertEqual(
            official_section_world.OVERWORLD_REGION,
            Path("dimensions/minecraft/overworld/region"),
        )


if __name__ == "__main__":
    unittest.main()
