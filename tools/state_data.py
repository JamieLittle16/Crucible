#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

SCHEMA = 1
GENERATOR_VERSION = "0.2.0"
FLAG_BITS = {
    "non_air": 0,
    "counted_fluid": 1,
    "random_block": 2,
    "random_fluid": 3,
}


def canonical_json(value: object) -> bytes:
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode()


def digest(value: object) -> str:
    return hashlib.sha256(canonical_json(value)).hexdigest()


def validate(data: dict[str, object]) -> None:
    if data.get("schema") != SCHEMA:
        raise ValueError("unsupported state-data schema")

    target = data.get("target")
    if not isinstance(target, dict):
        raise ValueError("target must be an object")
    for key in ("minecraft_version", "protocol_version", "data_version"):
        if key not in target:
            raise ValueError(f"missing target.{key}")

    states = data.get("states")
    if not isinstance(states, list) or not states:
        raise ValueError("states must be a non-empty list")

    keys: set[str] = set()
    vanilla_ids: set[int] = set()
    for index, state in enumerate(states):
        if not isinstance(state, dict):
            raise ValueError(f"state {index} is not an object")

        key = state.get("key")
        vanilla_id = state.get("vanilla_id")
        if not isinstance(key, str) or not key:
            raise ValueError(f"state {index} has invalid key")
        if key in keys:
            raise ValueError(f"duplicate state key: {key}")
        keys.add(key)

        if not isinstance(vanilla_id, int) or vanilla_id < 0:
            raise ValueError(f"state {key} has invalid vanilla_id")
        if vanilla_id in vanilla_ids:
            raise ValueError(f"duplicate vanilla_id: {vanilla_id}")
        vanilla_ids.add(vanilla_id)

        for fact in FLAG_BITS:
            if not isinstance(state.get(fact), bool):
                raise ValueError(f"state {key} missing boolean {fact}")

        if state["counted_fluid"] and not state["non_air"]:
            raise ValueError(f"{key}: counted_fluid requires non_air")
        if state["random_fluid"] and not state["counted_fluid"]:
            raise ValueError(f"{key}: random_fluid requires counted_fluid")


def load(path: Path) -> dict[str, object]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError("state-data root must be an object")
    validate(data)
    return data


def assign(states: list[dict[str, object]], policy: str) -> list[dict[str, object]]:
    if policy == "vanilla-identity":
        ordered = sorted(states, key=lambda state: int(state["vanilla_id"]))
        ids = [int(state["vanilla_id"]) for state in ordered]
        if ids != list(range(len(ordered))):
            raise ValueError("vanilla-identity requires dense vanilla IDs 0..N-1")
        return ordered
    if policy == "canonical-key":
        return sorted(states, key=lambda state: str(state["key"]))
    raise ValueError(f"unknown assignment policy: {policy}")


def representation_for(state_count: int) -> str:
    return "u16" if state_count <= (1 << 16) else "u32"


def packed_flags(state: dict[str, object]) -> int:
    result = 0
    for name, bit in FLAG_BITS.items():
        if bool(state[name]):
            result |= 1 << bit
    return result


def _repr_conversions(representation: str) -> tuple[str, str, str]:
    if representation == "u16":
        return (
            "usize::from(self.0)",
            "u32::from(id.0)",
            "#[expect(clippy::cast_possible_truncation, reason = \"range checked against the generated u16 state universe\")]\n            Some(Self(raw as BlockStateRepr))",
        )
    return (
        "usize::try_from(self.0).expect(\"u32 BlockStateId requires a usize-capable target\")",
        "id.0",
        "Some(Self(raw))",
    )


def render_rust(data: dict[str, object], policy: str) -> tuple[str, dict[str, object]]:
    raw_states = data["states"]
    assert isinstance(raw_states, list)
    states = assign(raw_states, policy)
    state_count = len(states)
    representation = representation_for(state_count)

    key_to_internal = {str(state["key"]): index for index, state in enumerate(states)}
    air_key = str(data.get("air_key", "minecraft:air"))
    if air_key not in key_to_internal:
        raise ValueError(f"air_key not present: {air_key}")

    identity_mapping = all(
        index == int(state["vanilla_id"]) for index, state in enumerate(states)
    )
    manifest: dict[str, object] = {
        "schema": SCHEMA,
        "generator_version": GENERATOR_VERSION,
        "target": data["target"],
        "source_provenance": data.get("provenance"),
        "assignment_policy": policy,
        "state_count": state_count,
        "repr": representation,
        "air_key": air_key,
        "air_internal_id": key_to_internal[air_key],
        "input_digest": digest(data),
        "mapping": "identity" if identity_mapping else "translated",
    }
    manifest["generation_digest"] = digest(manifest)

    flag_values = ", ".join(str(packed_flags(state)) for state in states)
    to_vanilla = ", ".join(str(int(state["vanilla_id"])) for state in states)
    as_usize, identity_to_vanilla, checked_constructor = _repr_conversions(representation)

    if identity_mapping:
        conversion = f"""
#[inline]
#[must_use]
pub const fn to_vanilla_state_id(id: BlockStateId) -> u32 {{
    {identity_to_vanilla}
}}

#[inline]
#[must_use]
pub const fn from_vanilla_state_id(raw: u32) -> Option<BlockStateId> {{
    BlockStateId::new(raw)
}}
"""
    else:
        max_vanilla = max(int(state["vanilla_id"]) for state in states)
        from_vanilla: list[int | None] = [None] * (max_vanilla + 1)
        for internal, state in enumerate(states):
            from_vanilla[int(state["vanilla_id"])] = internal
        if any(value is None for value in from_vanilla):
            raise ValueError(
                "translated mapping currently requires dense external vanilla IDs"
            )
        from_values = ", ".join(str(value) for value in from_vanilla)
        conversion = f"""
pub static CRUCIBLE_TO_VANILLA_STATE_ID: [u32; BLOCK_STATE_COUNT] = [{to_vanilla}];
pub static VANILLA_TO_CRUCIBLE_STATE_ID: [u32; {len(from_vanilla)}] = [{from_values}];

#[inline]
#[must_use]
pub fn to_vanilla_state_id(id: BlockStateId) -> u32 {{
    CRUCIBLE_TO_VANILLA_STATE_ID[id.as_usize()]
}}

#[inline]
#[must_use]
pub fn from_vanilla_state_id(raw: u32) -> Option<BlockStateId> {{
    let index = usize::try_from(raw).ok()?;
    let mapped = *VANILLA_TO_CRUCIBLE_STATE_ID.get(index)?;
    BlockStateId::new(mapped)
}}
"""

    code = f'''//! @generated by tools/state_data.py {GENERATOR_VERSION}; do not edit by hand.
//! Target-version block-state identity and section-mutation facts.

#![forbid(unsafe_code)]

use crucible_world_contract::{{BlockStateFacts, SectionStateFacts}};

pub type BlockStateRepr = {representation};
pub const BLOCK_STATE_COUNT: usize = {state_count};
pub const STATE_DATA_INPUT_SHA256: &str = "{manifest['input_digest']}";
pub const STATE_DATA_GENERATION_SHA256: &str = "{manifest['generation_digest']}";
pub const STATE_ID_ASSIGNMENT: &str = "{policy}";
pub const STATE_EXTERNAL_MAPPING: &str = "{manifest['mapping']}";

const _: () = assert!(BLOCK_STATE_COUNT > 0);
const _: () = assert!(BLOCK_STATE_COUNT - 1 <= BlockStateRepr::MAX as usize);

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlockStateId(BlockStateRepr);

impl BlockStateId {{
    /// Validates a numeric state identity at a cold/untrusted boundary.
    #[must_use]
    pub const fn new(raw: u32) -> Option<Self> {{
        if raw < BLOCK_STATE_COUNT as u32 {{
            {checked_constructor}
        }} else {{
            None
        }}
    }}

    /// Returns the dense table index used by generated HOT metadata.
    #[inline]
    #[must_use]
    pub fn as_usize(self) -> usize {{
        {as_usize}
    }}
}}

pub const AIR: BlockStateId = BlockStateId({key_to_internal[air_key]});
pub static STATE_MUTATION_FLAGS: [u8; BLOCK_STATE_COUNT] = [{flag_values}];
{conversion}
/// Zero-sized provider for target-version section mutation facts.
#[derive(Clone, Copy, Debug, Default)]
pub struct GeneratedStateFacts;

impl BlockStateFacts<BlockStateId> for GeneratedStateFacts {{
    #[inline]
    fn facts(&self, state: BlockStateId) -> SectionStateFacts {{
        let bits = STATE_MUTATION_FLAGS[state.as_usize()];
        SectionStateFacts::new(
            bits & 1 != 0,
            bits & 2 != 0,
            bits & 4 != 0,
            bits & 8 != 0,
        )
    }}
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn generated_identity_bounds_hold() {{
        assert_eq!(AIR, BlockStateId::new({key_to_internal[air_key]}).expect("generated air is in range"));
        assert!(BlockStateId::new(BLOCK_STATE_COUNT as u32 - 1).is_some());
        assert!(BlockStateId::new(BLOCK_STATE_COUNT as u32).is_none());
        assert_eq!(STATE_MUTATION_FLAGS.len(), BLOCK_STATE_COUNT);
    }}

    #[test]
    fn external_identity_round_trip_is_exact() {{
        for raw in 0..BLOCK_STATE_COUNT as u32 {{
            let state = from_vanilla_state_id(raw).expect("generated external id is in range");
            assert_eq!(to_vanilla_state_id(state), raw);
        }}
    }}
}}
'''
    return code, manifest


def command_inspect(args: argparse.Namespace) -> int:
    data = load(Path(args.input))
    raw_states = data["states"]
    assert isinstance(raw_states, list)
    states = assign(raw_states, args.assignment)
    print(
        json.dumps(
            {
                "state_count": len(states),
                "repr": representation_for(len(states)),
                "assignment": args.assignment,
                "input_digest": digest(data),
                "vanilla_id_min": min(int(state["vanilla_id"]) for state in states),
                "vanilla_id_max": max(int(state["vanilla_id"]) for state in states),
            },
            indent=2,
        )
    )
    return 0


def command_generate(args: argparse.Namespace) -> int:
    data = load(Path(args.input))
    code, manifest = render_rust(data, args.assignment)
    output = Path(args.output)
    manifest_path = Path(args.manifest)
    output.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(code, encoding="utf-8")
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        f"generated {manifest['state_count']} states as {manifest['repr']}; "
        f"{manifest['mapping']} external mapping"
    )
    return 0


def command_verify(args: argparse.Namespace) -> int:
    data = load(Path(args.input))
    code, manifest = render_rust(data, args.assignment)
    expected_manifest = json.dumps(manifest, indent=2, sort_keys=True) + "\n"
    ok = True
    for path, expected in (
        (Path(args.output), code),
        (Path(args.manifest), expected_manifest),
    ):
        if not path.is_file() or path.read_text(encoding="utf-8") != expected:
            print(f"stale generated file: {path}", file=sys.stderr)
            ok = False
    return 0 if ok else 1


def main() -> int:
    parser = argparse.ArgumentParser(description="Crucible target block-state data generator")
    subparsers = parser.add_subparsers(dest="command", required=True)
    commands = {
        "inspect": command_inspect,
        "generate": command_generate,
        "verify": command_verify,
    }
    for name, handler in commands.items():
        subparser = subparsers.add_parser(name)
        subparser.add_argument("input")
        subparser.add_argument(
            "--assignment",
            choices=("vanilla-identity", "canonical-key"),
            default="vanilla-identity",
        )
        if name in ("generate", "verify"):
            subparser.add_argument("--output", required=True)
            subparser.add_argument("--manifest", required=True)
        subparser.set_defaults(handler=handler)

    args = parser.parse_args()
    try:
        return int(args.handler(args))
    except (ValueError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
