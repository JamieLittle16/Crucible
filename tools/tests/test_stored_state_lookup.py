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
    def test_fingerprint_is_stable_fnv1a64(self) -> None:
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

    def test_generation_is_deterministic_sorted_and_compact(self) -> None:
        data = fixture()
        first_code, first_manifest = stored_state_lookup.render_rust(
            data, manifest(data)
        )
        second_code, second_manifest = stored_state_lookup.render_rust(
            data, manifest(data)
        )
        self.assertEqual(first_code, second_code)
        self.assertEqual(first_manifest, second_manifest)
        fingerprints = [
            stored_state_lookup.canonical_state_fingerprint(str(state["key"]))
            for state in data["states"]
        ]
        for fingerprint in sorted(fingerprints):
            self.assertIn(f"0x{fingerprint:016x}", first_code)
        self.assertEqual(first_manifest["state_count"], 3)
        self.assertEqual(first_manifest["fingerprint_bytes"], 24)
        self.assertEqual(first_manifest["metadata_bytes"], 24)
        self.assertEqual(
            first_manifest["layout"],
            "soa-u64-fingerprint-u32-offset-u16-length-u16-state-v1",
        )
        self.assertEqual(
            first_manifest["state_data_input_sha256"], state_data.digest(data)
        )

    def test_manifest_input_mismatch_fails_closed(self) -> None:
        data = fixture()
        wrong = manifest(data)
        wrong["input_digest"] = "0" * 64
        with self.assertRaises(ValueError):
            stored_state_lookup.render_rust(data, wrong)

    def test_assignment_policy_is_bound_to_state_manifest(self) -> None:
        data = fixture()
        changed = copy.deepcopy(data)
        states = changed["states"]
        assert isinstance(states, list)
        states[1]["key"] = "minecraft:z"
        states[2]["key"] = "minecraft:a"
        selected_manifest = manifest(changed)
        selected_manifest["assignment_policy"] = "canonical-key"
        code, _ = stored_state_lookup.render_rust(changed, selected_manifest)

        assigned = state_data.assign(states, "canonical-key")
        rows = sorted(
            (
                stored_state_lookup.canonical_state_fingerprint(str(state["key"])),
                str(state["key"]),
                state_id,
            )
            for state_id, state in enumerate(assigned)
        )
        target_index = next(
            index for index, (_, key, _) in enumerate(rows) if key == "minecraft:a"
        )
        metadata_lines = [
            line.strip()
            for line in code.splitlines()
            if "StoredStateLookupRow::new(" in line
        ]
        self.assertTrue(metadata_lines[target_index].endswith(", 0),"))

    def test_key_length_over_u16_is_rejected(self) -> None:
        data = fixture()
        states = data["states"]
        assert isinstance(states, list)
        states[1]["key"] = "minecraft:" + ("x" * 65_536)
        selected_manifest = manifest(data)
        with self.assertRaises(ValueError):
            stored_state_lookup.render_rust(data, selected_manifest)

    def test_more_than_u16_state_universe_is_rejected(self) -> None:
        data = fixture()
        states = data["states"]
        assert isinstance(states, list)
        prototype = dict(states[1])
        states.clear()
        for index in range(65_537):
            state = dict(prototype)
            state["key"] = f"minecraft:s{index}"
            state["vanilla_id"] = index
            states.append(state)
        data["air_key"] = "minecraft:s0"
        selected_manifest = manifest(data)
        selected_manifest["state_count"] = 65_537
        with self.assertRaises(ValueError):
            stored_state_lookup.render_rust(data, selected_manifest)


if __name__ == "__main__":
    unittest.main()
