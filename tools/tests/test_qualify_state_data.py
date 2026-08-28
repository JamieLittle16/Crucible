from __future__ import annotations

import importlib.util
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


binder = load_module("helve_qualify_state_data", ROOT / "tools" / "qualify_state_data.py")
generator = load_module("helve_state_data_for_binding", ROOT / "tools" / "state_data.py")


class QualifiedStateDataTests(unittest.TestCase):
    def lock(self) -> dict[str, object]:
        return {
            "minecraft": "26.2",
            "protocol": 776,
            "data_version": 4903,
            "source": {
                "archive_sha256": "a" * 64,
            },
            "runtime": {
                "server_sha256": "b" * 64,
            },
            "atlas": {
                "schema": 1,
                "version": "0.1.1",
                "fingerprint_algorithm": "java-token-v2-literal-sensitive",
            },
        }

    def spec(self) -> dict[str, object]:
        return {
            "schema": 1,
            "target": {
                "minecraft_version": "26.2",
                "protocol_version": 776,
                "data_version": 4903,
            },
            "locators": [
                {
                    "id": "STATE-SOURCE-NON-AIR",
                    "kind": "method",
                    "owner": "example.State",
                    "name": "isAir",
                    "param_count": 0,
                    "classification": "SEMANTIC_TARGET_DATA",
                    "role": "air predicate",
                },
                {
                    "id": "STATE-SOURCE-REGISTRY",
                    "kind": "field",
                    "owner": "example.Block",
                    "name": "REGISTRY",
                    "classification": "SEMANTIC_TARGET_DATA",
                    "role": "state enumeration",
                },
            ],
        }

    def source_qualification(self) -> dict[str, object]:
        spec = self.spec()
        value: dict[str, object] = {
            "schema": 1,
            "target": spec["target"],
            "source": {
                "archive_sha256": "a" * 64,
                "java_files": 1,
            },
            "atlas": {
                "schema": 1,
                "version": "0.1.1",
                "fingerprint_algorithm": "java-token-v2-literal-sensitive",
            },
            "spec_sha256": binder.digest(spec),
            "evidence": [
                {
                    "id": "STATE-SOURCE-NON-AIR",
                    "classification": "SEMANTIC_TARGET_DATA",
                    "role": "air predicate",
                    "surface": {
                        "kind": "method",
                        "owner": "example.State",
                        "name": "isAir",
                        "param_count": 0,
                        "body_sha256": "c" * 64,
                        "normalized_sha256": "d" * 64,
                    },
                },
                {
                    "id": "STATE-SOURCE-REGISTRY",
                    "classification": "SEMANTIC_TARGET_DATA",
                    "role": "state enumeration",
                    "surface": {
                        "kind": "field",
                        "owner": "example.Block",
                        "name": "REGISTRY",
                    },
                },
            ],
        }
        value["qualification_digest"] = binder.digest(value)
        return value

    def runtime_data(self) -> dict[str, object]:
        return {
            "schema": 1,
            "target": {
                "minecraft_version": "26.2",
                "protocol_version": 776,
                "data_version": 4903,
            },
            "air_key": "minecraft:air",
            "provenance": {
                "server_sha256": "b" * 64,
                "server_mappings_sha256": None,
                "name_mapping": "identity-unobfuscated",
                "source": "official-runtime-reflection-probe-v1",
                "startup_sequence": [
                    "SharedConstants.tryDetectVersion",
                    "Bootstrap.bootStrap",
                ],
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
            ],
        }

    def test_binding_is_deterministic_and_generator_compatible(self) -> None:
        runtime = self.runtime_data()
        source = self.source_qualification()
        spec = self.spec()
        lock = self.lock()

        first = binder.bind(runtime, source, spec, lock)
        second = binder.bind(runtime, source, spec, lock)
        self.assertEqual(first, second)
        provenance = first["provenance"]
        assert isinstance(provenance, dict)
        self.assertEqual(provenance["qualification"], "source+official-runtime")
        self.assertEqual(
            provenance["source_qualification_digest"],
            source["qualification_digest"],
        )
        self.assertEqual(provenance["runtime_server_sha256"], "b" * 64)

        generator.validate(first)
        rust, manifest = generator.render_rust(first, "vanilla-identity")
        self.assertIn("pub type BlockStateRepr = u16;", rust)
        self.assertEqual(manifest["state_count"], 2)
        self.assertEqual(manifest["mapping"], "identity")
        self.assertEqual(manifest["source_provenance"], provenance)

    def test_wrong_official_server_fails_closed(self) -> None:
        runtime = self.runtime_data()
        provenance = runtime["provenance"]
        assert isinstance(provenance, dict)
        provenance["server_sha256"] = "e" * 64
        with self.assertRaisesRegex(ValueError, "official server SHA-256 mismatch"):
            binder.bind(runtime, self.source_qualification(), self.spec(), self.lock())

    def test_forged_source_qualification_digest_fails_closed(self) -> None:
        source = self.source_qualification()
        source["qualification_digest"] = "f" * 64
        with self.assertRaisesRegex(ValueError, "source qualification digest mismatch"):
            binder.bind(self.runtime_data(), source, self.spec(), self.lock())

    def test_changed_source_spec_invalidates_qualification(self) -> None:
        spec = self.spec()
        locators = spec["locators"]
        assert isinstance(locators, list)
        locator = locators[0]
        assert isinstance(locator, dict)
        locator["role"] = "changed semantic role"
        with self.assertRaisesRegex(ValueError, "source qualification spec digest mismatch"):
            binder.bind(
                self.runtime_data(),
                self.source_qualification(),
                spec,
                self.lock(),
            )

    def test_missing_source_evidence_fails_closed(self) -> None:
        source = self.source_qualification()
        evidence = source["evidence"]
        assert isinstance(evidence, list)
        evidence.pop()
        source_without_digest = {
            key: value for key, value in source.items() if key != "qualification_digest"
        }
        source["qualification_digest"] = binder.digest(source_without_digest)
        with self.assertRaisesRegex(ValueError, "source qualification evidence IDs mismatch"):
            binder.bind(self.runtime_data(), source, self.spec(), self.lock())


if __name__ == "__main__":
    unittest.main()
