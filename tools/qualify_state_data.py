#!/usr/bin/env python3
"""Bind official-runtime block-state facts to reviewed source qualification.

`official_state_data.py` is intentionally an independent runtime oracle. Its output is useful
for probing and CI, but it is not production-qualified by itself. This tool joins that runtime
observation with the fingerprint-only source qualification produced from the pinned Vanilla
Atlas and emits the only normalized dataset eligible for frozen target-data generation.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
BINDER_VERSION = "0.1.0"
RUNTIME_PROBE_ID = "official-runtime-reflection-probe-v1"
FLAG_NAMES = ("non_air", "counted_fluid", "random_block", "random_fluid")


def canonical_json_bytes(value: object) -> bytes:
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8")


def digest(value: object) -> str:
    return hashlib.sha256(canonical_json_bytes(value)).hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object in {path}")
    return value


def load_lock(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        value = tomllib.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"expected TOML table in {path}")
    return value


def require_equal(label: str, actual: object, expected: object) -> None:
    if actual != expected:
        raise ValueError(f"{label} mismatch: expected {expected!r}, got {actual!r}")


def expected_target(lock: dict[str, Any]) -> dict[str, object]:
    return {
        "minecraft_version": str(lock["minecraft"]),
        "protocol_version": int(lock["protocol"]),
        "data_version": int(lock["data_version"]),
    }


def validate_runtime_dataset(data: dict[str, Any], lock: dict[str, Any]) -> None:
    require_equal("runtime dataset schema", data.get("schema"), SCHEMA_VERSION)
    require_equal("runtime target", data.get("target"), expected_target(lock))

    provenance = data.get("provenance")
    if not isinstance(provenance, dict):
        raise ValueError("runtime dataset requires provenance object")
    require_equal("runtime probe", provenance.get("source"), RUNTIME_PROBE_ID)
    require_equal(
        "official server SHA-256",
        provenance.get("server_sha256"),
        str(lock["runtime"]["server_sha256"]),
    )

    states = data.get("states")
    if not isinstance(states, list) or not states:
        raise ValueError("runtime dataset requires non-empty states")

    keys: set[str] = set()
    vanilla_ids: set[int] = set()
    for index, state in enumerate(states):
        if not isinstance(state, dict):
            raise ValueError(f"runtime state {index} is not an object")
        key = state.get("key")
        vanilla_id = state.get("vanilla_id")
        if not isinstance(key, str) or not key:
            raise ValueError(f"runtime state {index} has invalid key")
        if key in keys:
            raise ValueError(f"duplicate runtime state key: {key}")
        keys.add(key)
        if not isinstance(vanilla_id, int) or vanilla_id < 0:
            raise ValueError(f"runtime state {key} has invalid vanilla_id")
        if vanilla_id in vanilla_ids:
            raise ValueError(f"duplicate runtime vanilla_id: {vanilla_id}")
        vanilla_ids.add(vanilla_id)
        for flag in FLAG_NAMES:
            if not isinstance(state.get(flag), bool):
                raise ValueError(f"runtime state {key} missing boolean {flag}")
        if state["counted_fluid"] and not state["non_air"]:
            raise ValueError(f"{key}: counted_fluid requires non_air")
        if state["random_fluid"] and not state["counted_fluid"]:
            raise ValueError(f"{key}: random_fluid requires counted_fluid")

    ordered_ids = sorted(vanilla_ids)
    if ordered_ids != list(range(len(states))):
        raise ValueError("runtime vanilla state IDs are not dense 0..N-1")

    air_key = data.get("air_key")
    if not isinstance(air_key, str) or air_key not in keys:
        raise ValueError("runtime air_key is absent from the state universe")


def validate_source_qualification(
    qualification: dict[str, Any],
    spec: dict[str, Any],
    lock: dict[str, Any],
) -> None:
    require_equal("source qualification schema", qualification.get("schema"), SCHEMA_VERSION)
    require_equal("source qualification target", qualification.get("target"), expected_target(lock))

    source = qualification.get("source")
    if not isinstance(source, dict):
        raise ValueError("source qualification requires source object")
    require_equal(
        "qualified source archive",
        source.get("archive_sha256"),
        str(lock["source"]["archive_sha256"]),
    )

    atlas = qualification.get("atlas")
    if not isinstance(atlas, dict):
        raise ValueError("source qualification requires atlas object")
    require_equal("qualified Atlas schema", atlas.get("schema"), int(lock["atlas"]["schema"]))
    require_equal("qualified Atlas version", atlas.get("version"), str(lock["atlas"]["version"]))
    require_equal(
        "qualified fingerprint algorithm",
        atlas.get("fingerprint_algorithm"),
        str(lock["atlas"]["fingerprint_algorithm"]),
    )

    require_equal("source qualification spec schema", spec.get("schema"), SCHEMA_VERSION)
    require_equal("source qualification spec target", spec.get("target"), expected_target(lock))
    require_equal("source qualification spec digest", qualification.get("spec_sha256"), digest(spec))

    supplied_digest = qualification.get("qualification_digest")
    if not isinstance(supplied_digest, str) or not supplied_digest:
        raise ValueError("source qualification requires qualification_digest")
    digest_input = {key: value for key, value in qualification.items() if key != "qualification_digest"}
    require_equal("source qualification digest", supplied_digest, digest(digest_input))

    spec_locators = spec.get("locators")
    evidence = qualification.get("evidence")
    if not isinstance(spec_locators, list) or not spec_locators:
        raise ValueError("source qualification spec requires locators")
    if not isinstance(evidence, list):
        raise ValueError("source qualification requires evidence array")

    expected_by_id: dict[str, dict[str, Any]] = {}
    for locator in spec_locators:
        if not isinstance(locator, dict):
            raise ValueError("source qualification spec locator is not an object")
        locator_id = locator.get("id")
        if not isinstance(locator_id, str) or not locator_id:
            raise ValueError("source qualification spec locator requires id")
        if locator_id in expected_by_id:
            raise ValueError(f"duplicate qualification spec locator: {locator_id}")
        expected_by_id[locator_id] = locator

    actual_by_id: dict[str, dict[str, Any]] = {}
    for item in evidence:
        if not isinstance(item, dict):
            raise ValueError("source qualification evidence item is not an object")
        evidence_id = item.get("id")
        if not isinstance(evidence_id, str) or not evidence_id:
            raise ValueError("source qualification evidence item requires id")
        if evidence_id in actual_by_id:
            raise ValueError(f"duplicate source qualification evidence: {evidence_id}")
        actual_by_id[evidence_id] = item

    require_equal("source qualification evidence IDs", set(actual_by_id), set(expected_by_id))
    for evidence_id, locator in expected_by_id.items():
        item = actual_by_id[evidence_id]
        require_equal(
            f"{evidence_id} classification",
            item.get("classification"),
            locator.get("classification"),
        )
        require_equal(f"{evidence_id} role", item.get("role"), locator.get("role"))
        surface = item.get("surface")
        if not isinstance(surface, dict):
            raise ValueError(f"{evidence_id} is missing resolved source surface")
        require_equal(f"{evidence_id} surface kind", surface.get("kind"), locator.get("kind"))
        require_equal(f"{evidence_id} owner", surface.get("owner"), locator.get("owner"))
        if locator.get("kind") in {"field", "method"}:
            require_equal(f"{evidence_id} name", surface.get("name"), locator.get("name"))
        if locator.get("kind") == "method":
            require_equal(
                f"{evidence_id} parameter count",
                surface.get("param_count"),
                locator.get("param_count"),
            )
            normalized_sha = surface.get("normalized_sha256")
            body_sha = surface.get("body_sha256")
            if not isinstance(normalized_sha, str) or len(normalized_sha) != 64:
                raise ValueError(f"{evidence_id} has invalid normalized method fingerprint")
            if not isinstance(body_sha, str) or len(body_sha) != 64:
                raise ValueError(f"{evidence_id} has invalid raw body fingerprint")


def bind(
    runtime_data: dict[str, Any],
    source_qualification: dict[str, Any],
    spec: dict[str, Any],
    lock: dict[str, Any],
) -> dict[str, Any]:
    validate_runtime_dataset(runtime_data, lock)
    validate_source_qualification(source_qualification, spec, lock)

    runtime_provenance = runtime_data["provenance"]
    assert isinstance(runtime_provenance, dict)
    source_digest = source_qualification["qualification_digest"]
    assert isinstance(source_digest, str)

    result: dict[str, Any] = {
        "schema": SCHEMA_VERSION,
        "target": runtime_data["target"],
        "air_key": runtime_data["air_key"],
        "provenance": {
            "qualification": "source+official-runtime",
            "binder_version": BINDER_VERSION,
            "source_archive_sha256": str(lock["source"]["archive_sha256"]),
            "source_qualification_digest": source_digest,
            "runtime_probe": RUNTIME_PROBE_ID,
            "runtime_server_sha256": str(runtime_provenance["server_sha256"]),
            "runtime_server_mappings_sha256": runtime_provenance.get("server_mappings_sha256"),
            "runtime_name_mapping": runtime_provenance.get("name_mapping"),
            "runtime_startup_sequence": runtime_provenance.get("startup_sequence"),
            "raw_runtime_input_digest": digest(runtime_data),
        },
        "states": runtime_data["states"],
    }
    result["provenance"]["qualified_input_digest"] = digest(result)
    return result


def rendered(value: dict[str, Any]) -> str:
    return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runtime-data", required=True)
    parser.add_argument("--source-qualification", required=True)
    parser.add_argument("--spec", default="vanilla/state-data/source-qualification-spec.json")
    parser.add_argument("--lock", default="vanilla/vanilla.lock.toml")
    parser.add_argument("--output", required=True)
    parser.add_argument(
        "--verify",
        action="store_true",
        help="fail unless output is byte-identical to a fresh binding",
    )
    args = parser.parse_args()

    try:
        runtime_data = load_json(Path(args.runtime_data))
        source_qualification = load_json(Path(args.source_qualification))
        spec = load_json(Path(args.spec))
        lock = load_lock(Path(args.lock))
        value = bind(runtime_data, source_qualification, spec, lock)
        text = rendered(value)
        output = Path(args.output)
        if args.verify:
            if not output.is_file() or output.read_text(encoding="utf-8") != text:
                raise ValueError("qualified state dataset differs from bound source/runtime evidence")
            print(
                "verified qualified state data: "
                f"{value['provenance']['qualified_input_digest']}"
            )
            return 0
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(text, encoding="utf-8")
        print(
            "wrote qualified state data: "
            f"{value['provenance']['qualified_input_digest']}"
        )
        return 0
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
