from __future__ import annotations

import copy
import importlib.util
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "tools" / "state_data.py"
SPEC = importlib.util.spec_from_file_location("helve_state_data", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
state_data = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = state_data
SPEC.loader.exec_module(state_data)


def fixture(state_count: int = 4) -> dict[str, object]:
    states: list[dict[str, object]] = []
    for index in range(state_count):
        states.append(
            {
                "key": f"minecraft:s{index}",
                "vanilla_id": index,
                "non_air": index != 0,
                "counted_fluid": index == 2,
                "random_block": index == 3,
                "random_fluid": False,
            }
        )
    states[0]["key"] = "minecraft:air"
    return {
        "schema": 1,
        "target": {
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "data_version": 4903,
        },
        "air_key": "minecraft:air",
        "states": states,
    }


class StateDataTests(unittest.TestCase):
    def test_identity_generation_is_deterministic(self) -> None:
        data = fixture()
        first_code, first_manifest = state_data.render_rust(data, "vanilla-identity")
        second_code, second_manifest = state_data.render_rust(data, "vanilla-identity")
        self.assertEqual(first_code, second_code)
        self.assertEqual(first_manifest, second_manifest)
        self.assertEqual(first_manifest["mapping"], "identity")
        self.assertEqual(first_manifest["repr"], "u16")
        self.assertEqual(first_manifest["air_internal_id"], 0)

    def test_generation_digest_changes_with_input_and_assignment_policy(self) -> None:
        data = fixture()
        _, baseline = state_data.render_rust(data, "vanilla-identity")

        changed_input = copy.deepcopy(data)
        states = changed_input["states"]
        assert isinstance(states, list)
        states[3]["random_block"] = False
        _, input_changed = state_data.render_rust(changed_input, "vanilla-identity")
        _, policy_changed = state_data.render_rust(data, "canonical-key")

        self.assertNotEqual(
            baseline["generation_digest"], input_changed["generation_digest"]
        )
        self.assertNotEqual(
            baseline["generation_digest"], policy_changed["generation_digest"]
        )

    def test_canonical_assignment_generates_translation_tables(self) -> None:
        data = fixture()
        states = data["states"]
        assert isinstance(states, list)
        states[1]["key"] = "minecraft:z"
        states[2]["key"] = "minecraft:a"
        code, manifest = state_data.render_rust(data, "canonical-key")
        self.assertEqual(manifest["mapping"], "translated")
        self.assertIn("CRUCIBLE_TO_VANILLA_STATE_ID", code)
        self.assertIn("VANILLA_TO_CRUCIBLE_STATE_ID", code)

    def test_width_widens_beyond_u16_capacity(self) -> None:
        data = fixture(65_537)
        _, manifest = state_data.render_rust(data, "vanilla-identity")
        self.assertEqual(manifest["repr"], "u32")

    def test_duplicate_canonical_state_key_is_rejected(self) -> None:
        data = fixture()
        states = data["states"]
        assert isinstance(states, list)
        states[2]["key"] = states[1]["key"]
        with self.assertRaises(ValueError):
            state_data.validate(data)

    def test_identity_assignment_requires_exact_dense_external_ids(self) -> None:
        data = fixture()
        states = data["states"]
        assert isinstance(states, list)
        states[3]["vanilla_id"] = 9
        with self.assertRaises(ValueError):
            state_data.assign(states, "vanilla-identity")

    def test_all_sixteen_raw_fact_bit_combinations_pack_exactly(self) -> None:
        for bits in range(16):
            state = {
                "non_air": bits & 1 != 0,
                "counted_fluid": bits & 2 != 0,
                "random_block": bits & 4 != 0,
                "random_fluid": bits & 8 != 0,
            }
            self.assertEqual(state_data.packed_flags(state), bits)

        code, _ = state_data.render_rust(fixture(), "vanilla-identity")
        self.assertIn("mutation_flag_decoder_covers_all_sixteen_combinations", code)
        self.assertIn("facts_from_mutation_flags(STATE_MUTATION_FLAGS", code)

    def test_invalid_fluid_relation_is_rejected(self) -> None:
        data = fixture()
        states = data["states"]
        assert isinstance(states, list)
        states[1]["non_air"] = False
        states[1]["counted_fluid"] = True
        with self.assertRaises(ValueError):
            state_data.validate(data)

    def test_random_fluid_requires_counted_fluid(self) -> None:
        data = fixture()
        states = data["states"]
        assert isinstance(states, list)
        states[1]["random_fluid"] = True
        with self.assertRaises(ValueError):
            state_data.validate(data)


if __name__ == "__main__":
    unittest.main()
