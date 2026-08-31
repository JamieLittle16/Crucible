from __future__ import annotations

import copy
import importlib.util
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STATE_MODULE_PATH = ROOT / "tools" / "state_data.py"
STATE_SPEC = importlib.util.spec_from_file_location("helve_state_data", STATE_MODULE_PATH)
assert STATE_SPEC is not None and STATE_SPEC.loader is not None
state_data = importlib.util.module_from_spec(STATE_SPEC)
sys.modules[STATE_SPEC.name] = state_data
sys.modules["state_data"] = state_data
STATE_SPEC.loader.exec_module(state_data)

LOOKUP_MODULE_PATH = ROOT / "tools" / "stored_state_lookup.py"
LOOKUP_SPEC = importlib.util.spec_from_file_location(
    "helve_stored_state_lookup", LOOKUP_MODULE_PATH
)
assert LOOKUP_SPEC is not None and LOOKUP_SPEC.loader is not None
stored_state_lookup = importlib.util.module_from_spec(LOOKUP_SPEC)
sys.modules[LOOKUP_SPEC.name] = stored_state_lookup
LOOKUP_SPEC.loader.exec_module(stored_state_lookup)


def fixture() -> dict[str, object]:
    return {
        "schema": 1,
        "target": {
            "minecraft_version": "26.2",
            "protocol_version": 776,
            "data_version": 4903,
        },
        "air_key": "minecraft:air",
        "states": [
            {
                "key": "minecraft:air",
                "vanilla_id": 0,
                "non_air": False,
                "counted_fluid": False,
                "random_block": False,
                "random_fluid": False,
            },
            {
                "key": "minecraft:oak_log[axis=y]",
                "vanilla_id": 1,
                "non_air": True,
                "counted_fluid": False,
                "random_block": False,
                "random_fluid": False,
            },
            {
                "key": "minecraft:water[level=3]",
                "vanilla_id": 2,
                "non_air": True,
                "counted_fluid": True,
                "random_block": False,
                "random_fluid": False,
            },
        ],
    }


def manifest(data: dict[str, object]) -> dict[str, object]:
    return {
        "target": data["target"],
        "input_digest": state_data.digest(data),
        "generation_digest": "a" * 64,
        "assignment_policy": "vanilla-identity",
        "state_count": 3,
    }


class StoredStateLookupTests(unittest.TestCase):
    def test_hash_witnesses_are_stable(self) -> None:
        self.assertEqual(
            stored_state_lookup.canonical_state_fingerprint("minecraft:air"),
            0xC480B16A40058EC2,
        )
        self.assertEqual(
            stored_state_lookup.canonical_state_fingerprint(
                "minecraft:oak_log[axis=y]"
            ),
            0xA628DDE6F2234D1F,
        )
        self.assertEqual(stored_state_lookup.splitmix64(0), 0xE220A8397B1DCDAF)

    def test_generation_is_deterministic_compact_and_exact(self) -> None:
        data = fixture()
        first = stored_state_lookup.build_artifacts(data, manifest(data))
        second = stored_state_lookup.build_artifacts(data, manifest(data))
        self.assertEqual(first, second)
        rust, binary, result = first
        self.assertIn("include_bytes!", rust)
        self.assertEqual(result["state_count"], 3)
        self.assertEqual(result["layout"], stored_state_lookup.LAYOUT)
        self.assertEqual(result["state_data_input_sha256"], state_data.digest(data))
        self.assertEqual(result["binary_bytes"], len(binary))
        self.assertEqual(
            result["binary_sha256"], stored_state_lookup.sha256_bytes(binary)
        )
        self.assertLess(len(binary), 1024)

    def test_perfect_hash_has_one_exact_candidate_per_fixture_state(self) -> None:
        data = fixture()
        _, _, result = stored_state_lookup.build_artifacts(data, manifest(data))
        self.assertEqual(result["slot_count"], 4)
        self.assertEqual(result["bucket_count"], 1)
        self.assertLessEqual(
            result["max_displacement"], stored_state_lookup.MAX_U16
        )

    def test_fingerprint_collision_fails_closed(self) -> None:
        with self.assertRaises(ValueError):
            stored_state_lookup.build_perfect_hash([7, 7])

    def test_noncanonical_property_order_fails_closed(self) -> None:
        data = fixture()
        states = data["states"]
        assert isinstance(states, list)
        states[1]["key"] = "minecraft:test[z=1,a=2]"
        with self.assertRaises(ValueError):
            stored_state_lookup.build_artifacts(data, manifest(data))

    def test_non_identity_assignment_is_rejected(self) -> None:
        data = fixture()
        selected = manifest(data)
        selected["assignment_policy"] = "canonical-key"
        with self.assertRaises(ValueError):
            stored_state_lookup.build_artifacts(data, selected)

    def test_manifest_input_mismatch_fails_closed(self) -> None:
        data = fixture()
        wrong = manifest(data)
        wrong["input_digest"] = "0" * 64
        with self.assertRaises(ValueError):
            stored_state_lookup.build_artifacts(data, wrong)

    def test_nine_bit_packing_covers_every_bit_alignment(self) -> None:
        values = [index % 435 for index in range(64)]
        packed = stored_state_lookup.pack_nine_bit(values)
        self.assertEqual(
            [
                stored_state_lookup.read_nine_bit(packed, index)
                for index in range(len(values))
            ],
            values,
        )

    def test_target_cardinality_overflow_is_rejected(self) -> None:
        data = fixture()
        states = data["states"]
        assert isinstance(states, list)
        prototype = copy.deepcopy(states[1])
        states.clear()
        for index in range(65_536):
            state = copy.deepcopy(prototype)
            state["key"] = f"minecraft:s{index}"
            state["vanilla_id"] = index
            states.append(state)
        data["air_key"] = "minecraft:s0"
        selected = manifest(data)
        selected["state_count"] = len(states)
        with self.assertRaises(ValueError):
            stored_state_lookup.build_artifacts(data, selected)


if __name__ == "__main__":
    unittest.main()
