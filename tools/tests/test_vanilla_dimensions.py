from __future__ import annotations

import sys
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import vanilla_dimensions


class VanillaDimensionDescriptorTests(unittest.TestCase):
    def test_standard_dimension_identity_and_26_2_save_paths_are_frozen(self) -> None:
        observed = [
            (descriptor.key, descriptor.region_path.as_posix())
            for descriptor in vanilla_dimensions.STANDARD_DIMENSIONS
        ]
        self.assertEqual(
            observed,
            [
                (
                    "minecraft:overworld",
                    "dimensions/minecraft/overworld/region",
                ),
                (
                    "minecraft:the_nether",
                    "dimensions/minecraft/the_nether/region",
                ),
                (
                    "minecraft:the_end",
                    "dimensions/minecraft/the_end/region",
                ),
            ],
        )

    def test_lookup_returns_descriptor_identity_not_a_copy(self) -> None:
        for descriptor in vanilla_dimensions.STANDARD_DIMENSIONS:
            self.assertIs(
                vanilla_dimensions.require_standard_dimension(descriptor.key),
                descriptor,
            )

    def test_unknown_dimension_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "unsupported standard vanilla dimension"):
            vanilla_dimensions.require_standard_dimension("minecraft:moon")

    def test_dimension_keys_are_unique(self) -> None:
        keys = [descriptor.key for descriptor in vanilla_dimensions.STANDARD_DIMENSIONS]
        self.assertEqual(len(keys), len(set(keys)))
        self.assertEqual(set(keys), set(vanilla_dimensions.BY_KEY))


if __name__ == "__main__":
    unittest.main()
