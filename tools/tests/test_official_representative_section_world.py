from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import official_representative_section_world as world
import section_representative_plan as plan
import vanilla_section_extractor as base


class OfficialRepresentativeSectionWorldTests(unittest.TestCase):
    def test_server_properties_bind_seed_and_enable_all_dimensions(self) -> None:
        properties = world.server_properties(123).splitlines()
        self.assertIn("level-seed=123", properties)
        self.assertIn("allow-nether=true", properties)
        self.assertIn("sync-chunk-writes=true", properties)
        self.assertIn("max-tick-time=-1", properties)
        self.assertNotIn("allow-nether=false", properties)

    def test_forceload_commands_are_exact_and_plan_bound(self) -> None:
        built = plan.build_plan()
        commands = world.commands_for_plan(built)
        self.assertEqual(len(commands), 192)
        self.assertEqual(
            world.command_digest(commands),
            "cb97b7490c28e38293251561749a87dbda2d0f78d78c7cf98471e5eff825a354",
        )
        self.assertEqual(
            commands[0],
            "execute in minecraft:overworld run forceload add 0 0",
        )
        self.assertEqual(
            commands[64],
            "execute in minecraft:the_nether run forceload add 0 0",
        )
        self.assertEqual(
            commands[128],
            "execute in minecraft:the_end run forceload add 0 0",
        )
        self.assertTrue(
            all(command.startswith("execute in minecraft:") for command in commands)
        )

    def test_region_postcondition_covers_every_planned_chunk_region(self) -> None:
        built = plan.build_plan()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = world.expected_region_paths(root, built)
            self.assertGreater(len(paths), 0)
            relative = {path.relative_to(root).as_posix() for path in paths}
            for dimension, dimension_path in base.STANDARD_DIMENSIONS:
                self.assertTrue(
                    any(
                        item.startswith(dimension_path.as_posix() + "/r.")
                        for item in relative
                    ),
                    dimension,
                )

            expected = set()
            dimensions = built["dimensions"]
            for dimension, dimension_path in base.STANDARD_DIMENSIONS:
                for chunk_x, chunk_z in dimensions[dimension]["chunks"]:
                    expected.add(
                        (
                            root
                            / dimension_path
                            / f"r.{chunk_x // 32}.{chunk_z // 32}.mca"
                        ).relative_to(root).as_posix()
                    )
            self.assertEqual(relative, expected)

    def test_command_schedule_has_no_duplicate_dimension_chunk(self) -> None:
        commands = world.commands_for_plan(plan.build_plan())
        self.assertEqual(len(commands), len(set(commands)))


if __name__ == "__main__":
    unittest.main()
