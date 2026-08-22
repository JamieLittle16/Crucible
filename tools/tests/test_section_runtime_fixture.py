from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


fixture_tool = load_module(
    "crucible_section_runtime_fixture", ROOT / "tools" / "section_runtime_fixture.py"
)


class SectionRuntimeFixtureTests(unittest.TestCase):
    def runtime(self) -> dict[str, object]:
        return {
            "schema": 1,
            "target": {
                "minecraft_version": "26.2",
                "protocol_version": 776,
                "data_version": 4903,
            },
            "air_key": "minecraft:air",
            "provenance": {
                "server_sha256": fixture_tool.RUNTIME_SERVER_SHA256,
                "server_mappings_sha256": None,
                "name_mapping": "identity-unobfuscated",
                "startup_sequence": [
                    "SharedConstants.tryDetectVersion",
                    "Bootstrap.bootStrap",
                ],
                "source": fixture_tool.RUNTIME_PROBE,
            },
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
                    "key": "minecraft:stone",
                    "vanilla_id": 1,
                    "non_air": True,
                    "counted_fluid": False,
                    "random_block": False,
                    "random_fluid": False,
                },
                {
                    "key": "minecraft:waterlogged",
                    "vanilla_id": 2,
                    "non_air": True,
                    "counted_fluid": True,
                    "random_block": False,
                    "random_fluid": False,
                },
                {
                    "key": "minecraft:crop",
                    "vanilla_id": 3,
                    "non_air": True,
                    "counted_fluid": False,
                    "random_block": True,
                    "random_fluid": False,
                },
                {
                    "key": "minecraft:all-random",
                    "vanilla_id": 4,
                    "non_air": True,
                    "counted_fluid": True,
                    "random_block": True,
                    "random_fluid": True,
                },
            ],
        }

    def manifest(self) -> dict[str, object]:
        return {
            "schema": 1,
            "target": {
                "minecraft_version": "26.2",
                "protocol_version": 776,
                "data_version": 4903,
            },
            "generation_digest": fixture_tool.STATE_GENERATION_SHA256,
            "state_count": 5,
        }

    def fixture(self) -> str:
        return "\n".join(
            [
                "CRUCIBLE-SECTION-SEMANTIC-FIXTURE|1|26.2|776|4903",
                "PROVENANCE|"
                + fixture_tool.SOURCE_ARCHIVE_SHA256
                + "|"
                + fixture_tool.SOURCE_QUALIFICATION_SHA256
                + "|"
                + fixture_tool.RUNTIME_SERVER_SHA256
                + "|"
                + fixture_tool.STATE_GENERATION_SHA256,
                "STATE|air|0",
                "STATE|solid|1",
                "STATE|fluid|3",
                "STATE|random-block|5",
                "STATE|random-fluid|15",
                "BLOCK-FILL|homogeneous-air|air|0|0|0|0",
                "BLOCK-FILL|homogeneous-non-air|solid|4096|0|0|0",
                "BLOCK-ONE|fluid-one|fluid|0|1|1|0|0",
                "BLOCK-REVERSE|fluid-zero|fluid|0",
                "BLOCK-ONE|block-random-one|random-block|17|1|0|1|0",
                "BLOCK-ONE|fluid-random-one|random-fluid|255|1|1|1|1",
                "BIOME-FILL-ORDER|x-major-y-z",
                "BIOME-REPLACE|1|2|3|7|42",
                "",
            ]
        )

    def write_inputs(self, root: Path) -> tuple[Path, Path, Path]:
        runtime = root / "runtime.json"
        fixture = root / "fixture.txt"
        manifest = root / "manifest.json"
        runtime.write_text(json.dumps(self.runtime(), indent=2) + "\n", encoding="utf-8")
        fixture.write_text(self.fixture(), encoding="utf-8")
        manifest.write_text(json.dumps(self.manifest(), indent=2) + "\n", encoding="utf-8")
        return runtime, fixture, manifest

    def test_runtime_facts_bind_fixture_deterministically(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            runtime, fixture, manifest = self.write_inputs(Path(tmp))
            first = fixture_tool.qualify(runtime, fixture, manifest)
            second = fixture_tool.qualify(runtime, fixture, manifest)
            self.assertEqual(first, second)
            self.assertEqual(first["state_count"], 5)
            self.assertEqual(len(first["runtime_checked_block_cases"]), 6)
            representatives = {
                case["state_label"]: case["representative_vanilla_id"]
                for case in first["runtime_checked_block_cases"]
            }
            self.assertEqual(representatives["air"], 0)
            self.assertEqual(representatives["solid"], 1)
            self.assertEqual(representatives["fluid"], 2)
            self.assertEqual(representatives["random-block"], 3)
            self.assertEqual(representatives["random-fluid"], 4)

    def test_changed_expected_summary_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            runtime, fixture, manifest = self.write_inputs(Path(tmp))
            fixture.write_text(
                self.fixture().replace(
                    "BLOCK-ONE|fluid-one|fluid|0|1|1|0|0",
                    "BLOCK-ONE|fluid-one|fluid|0|1|0|0|0",
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "official-runtime facts imply"):
                fixture_tool.qualify(runtime, fixture, manifest)

    def test_wrong_runtime_server_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            runtime, fixture, manifest = self.write_inputs(root)
            value = self.runtime()
            provenance = value["provenance"]
            assert isinstance(provenance, dict)
            provenance["server_sha256"] = "0" * 64
            runtime.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "server SHA-256"):
                fixture_tool.qualify(runtime, fixture, manifest)

    def test_state_invariant_violation_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            runtime, fixture, manifest = self.write_inputs(root)
            value = self.runtime()
            states = value["states"]
            assert isinstance(states, list)
            bad = states[2]
            assert isinstance(bad, dict)
            bad["non_air"] = False
            runtime.write_text(json.dumps(value), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "counted_fluid implies non_air"):
                fixture_tool.qualify(runtime, fixture, manifest)


if __name__ == "__main__":
    unittest.main()
