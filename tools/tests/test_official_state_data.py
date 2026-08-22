from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "tools" / "official_state_data.py"
SPEC = importlib.util.spec_from_file_location("crucible_official_state_data", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
probe = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = probe
SPEC.loader.exec_module(probe)


class OfficialStateDataTests(unittest.TestCase):
    def test_mapping_parser_extracts_required_shapes(self) -> None:
        mapping = """net.minecraft.world.level.block.Block -> abc:\n    net.minecraft.core.IdMapper BLOCK_STATE_REGISTRY -> f\n    1:1:int getId(net.minecraft.world.level.block.state.BlockState):1:1 -> a\nnet.minecraft.world.level.block.state.BlockBehaviour$BlockStateBase -> def:\n    1:1:boolean isAir():1:1 -> q\n"""
        classes, fields, methods = probe.parse_mappings(mapping)
        self.assertEqual(classes["net.minecraft.world.level.block.Block"], "abc")
        self.assertEqual(
            fields[("net.minecraft.world.level.block.Block", "BLOCK_STATE_REGISTRY")],
            "f",
        )
        self.assertEqual(
            probe.unique_method(
                methods,
                "net.minecraft.world.level.block.Block",
                "getId",
            ),
            "a",
        )

    def test_canonical_key_sorts_properties(self) -> None:
        self.assertEqual(
            probe.canonical_key("Block{minecraft:test}[z=2,a=1]"),
            "minecraft:test[a=1,z=2]",
        )

    def test_canonical_key_preserves_propertyless_state(self) -> None:
        self.assertEqual(
            probe.canonical_key("Block{minecraft:stone}"),
            "minecraft:stone",
        )


if __name__ == "__main__":
    unittest.main()
