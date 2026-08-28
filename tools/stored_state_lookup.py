#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from dataclasses import dataclass
from pathlib import Path

import state_data

SCHEMA = 2
GENERATOR_VERSION = "0.3.0"
LAYOUT = "mph-fnv1a64-splitmix64-structured-v2"
HASH_ALGORITHM = "fnv1a64-canonical-state-key-v1"
SLOT_HASH_ALGORITHM = "splitmix64-xor-displacement-v1"
FNV_OFFSET_BASIS = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3
MASK64 = (1 << 64) - 1
MAX_U16 = (1 << 16) - 1
MAX_NAME_IDS = 1 << 11
MAX_PROPERTY_PAIR_IDS = 1 << 9
MAX_PROPERTY_OCCURRENCES = 1 << 18
MAX_PROPERTIES_PER_STATE = 7


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_text(value: str) -> str:
    return sha256_bytes(value.encode("utf-8"))


def canonical_state_fingerprint(key: str) -> int:
    value = FNV_OFFSET_BASIS
    for byte in key.encode("utf-8"):
        value ^= byte
        value = (value * FNV_PRIME) & MASK64
    return value


def splitmix64(value: int) -> int:
    value = (value + 0x9E3779B97F4A7C15) & MASK64
    value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
    value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & MASK64
    return value ^ (value >> 31)


def next_power_of_two(value: int) -> int:
    if value <= 0:
        raise ValueError("power-of-two input must be positive")
    return 1 << (value - 1).bit_length()


def parse_canonical_key(key: str) -> tuple[str, tuple[tuple[str, str], ...]]:
    if "[" not in key:
        if "]" in key or not key:
            raise ValueError(f"malformed canonical state key: {key!r}")
        return key, ()
    if not key.endswith("]") or key.count("[") != 1:
        raise ValueError(f"malformed canonical state key: {key!r}")
    name, encoded = key[:-1].split("[", 1)
    if not name or not encoded:
        raise ValueError(f"malformed canonical state key: {key!r}")
    properties: list[tuple[str, str]] = []
    seen: set[str] = set()
    for item in encoded.split(","):
        if item.count("=") != 1:
            raise ValueError(f"malformed canonical state property: {item!r}")
        property_name, value = item.split("=", 1)
        if not property_name or not value or property_name in seen:
            raise ValueError(f"malformed canonical state property: {item!r}")
        seen.add(property_name)
        properties.append((property_name, value))
    if properties != sorted(properties):
        raise ValueError(f"canonical state properties are not sorted: {key!r}")
    return name, tuple(properties)


@dataclass(frozen=True)
class PerfectHash:
    bucket_count: int
    slot_count: int
    displacements: tuple[int, ...]
    slots: tuple[int, ...]
    max_displacement: int


def build_perfect_hash(fingerprints: list[int]) -> PerfectHash:
    if not fingerprints:
        raise ValueError("stored-state lookup requires at least one state")
    if len(set(fingerprints)) != len(fingerprints):
        raise ValueError("canonical-state FNV-64 collision in admitted target universe")

    slot_count = next_power_of_two(len(fingerprints))
    bucket_count = next_power_of_two((len(fingerprints) + 3) // 4)
    buckets: list[list[tuple[int, int]]] = [[] for _ in range(bucket_count)]
    for state_id, fingerprint in enumerate(fingerprints):
        buckets[fingerprint & (bucket_count - 1)].append((state_id, fingerprint))

    slots: list[int | None] = [None] * slot_count
    displacements = [0] * bucket_count
    max_displacement = 0
    for bucket_id, bucket in sorted(
        enumerate(buckets), key=lambda pair: (-len(pair[1]), pair[0])
    ):
        if not bucket:
            continue
        for displacement in range(MAX_U16 + 1):
            candidates: list[int] = []
            local: set[int] = set()
            valid = True
            for _, fingerprint in bucket:
                slot = splitmix64(fingerprint ^ displacement) & (slot_count - 1)
                if slots[slot] is not None or slot in local:
                    valid = False
                    break
                local.add(slot)
                candidates.append(slot)
            if not valid:
                continue
            displacements[bucket_id] = displacement
            max_displacement = max(max_displacement, displacement)
            for (state_id, _), slot in zip(bucket, candidates, strict=True):
                slots[slot] = state_id
            break
        else:
            raise ValueError(
                f"perfect-hash displacement exceeded u16 for bucket {bucket_id}"
            )

    return PerfectHash(
        bucket_count=bucket_count,
        slot_count=slot_count,
        displacements=tuple(displacements),
        slots=tuple(0 if state is None else state for state in slots),
        max_displacement=max_displacement,
    )


def pack_nine_bit(values: list[int]) -> bytes:
    accumulator = 0
    bits = 0
    output = bytearray()
    for value in values:
        if not 0 <= value < MAX_PROPERTY_PAIR_IDS:
            raise ValueError("property-pair ID exceeds frozen 9-bit representation")
        accumulator |= value << bits
        bits += 9
        while bits >= 8:
            output.append(accumulator & 0xFF)
            accumulator >>= 8
            bits -= 8
    if bits:
        output.append(accumulator & 0xFF)
    # The Rust reader always performs one checked little-endian u16 load per 9-bit value.
    output.append(0)
    return bytes(output)


def read_nine_bit(data: bytes, index: int) -> int:
    bit = index * 9
    byte = bit // 8
    shift = bit % 8
    if byte + 1 >= len(data):
        raise ValueError("packed 9-bit index exceeds generated data")
    word = data[byte] | (data[byte + 1] << 8)
    return (word >> shift) & 0x1FF


def load_manifest(path: Path) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError("state-data manifest root must be an object")
    return value


def build_artifacts(
    data: dict[str, object], manifest: dict[str, object]
) -> tuple[str, bytes, dict[str, object]]:
    state_data.validate(data)
    input_digest = state_data.digest(data)
    if manifest.get("input_digest") != input_digest:
        raise ValueError("qualified input digest does not match state-data manifest")
    if manifest.get("assignment_policy") != "vanilla-identity":
        raise ValueError("stored-state lookup requires vanilla-identity assignment")

    raw_states = data["states"]
    assert isinstance(raw_states, list)
    states = state_data.assign(raw_states, "vanilla-identity")
    if manifest.get("state_count") != len(states):
        raise ValueError("state count differs from state-data manifest")
    if len(states) > MAX_U16:
        raise ValueError("stored-state lookup requires state IDs addressable by u16")

    parsed: list[tuple[str, tuple[tuple[str, str], ...]]] = []
    fingerprints: list[int] = []
    for state_id, state in enumerate(states):
        if state.get("vanilla_id") != state_id:
            raise ValueError("vanilla-identity state assignment is not dense and exact")
        key = str(state["key"])
        parsed.append(parse_canonical_key(key))
        fingerprints.append(canonical_state_fingerprint(key))

    names = sorted({name for name, _ in parsed})
    pairs = sorted({pair for _, properties in parsed for pair in properties})
    if len(names) > MAX_NAME_IDS:
        raise ValueError("distinct block names exceed frozen 11-bit representation")
    if len(pairs) > MAX_PROPERTY_PAIR_IDS:
        raise ValueError("distinct property pairs exceed frozen 9-bit representation")
    name_ids = {name: index for index, name in enumerate(names)}
    pair_ids = {pair: index for index, pair in enumerate(pairs)}

    occurrences: list[int] = []
    descriptors: list[int] = []
    max_properties = 0
    for name, properties in parsed:
        if len(properties) > MAX_PROPERTIES_PER_STATE:
            raise ValueError("state exceeds frozen 3-bit property-count representation")
        property_start = len(occurrences)
        if property_start >= MAX_PROPERTY_OCCURRENCES:
            raise ValueError("property occurrences exceed frozen 18-bit representation")
        occurrences.extend(pair_ids[pair] for pair in properties)
        descriptors.append(
            name_ids[name] | (property_start << 11) | (len(properties) << 29)
        )
        max_properties = max(max_properties, len(properties))
    if len(occurrences) > MAX_PROPERTY_OCCURRENCES:
        raise ValueError("property occurrences exceed frozen 18-bit representation")

    perfect = build_perfect_hash(fingerprints)

    name_blob = bytearray()
    name_index = bytearray()
    for name in names:
        encoded = name.encode("utf-8")
        if len(name_blob) > MAX_U16 or len(encoded) > 0xFF:
            raise ValueError("block-name table exceeds frozen offset/length representation")
        name_index.extend(struct.pack("<HB", len(name_blob), len(encoded)))
        name_blob.extend(encoded)

    pair_blob = bytearray()
    pair_index = bytearray()
    for property_name, value in pairs:
        left = property_name.encode("utf-8")
        encoded = left + b"=" + value.encode("utf-8")
        if len(pair_blob) > MAX_U16 or len(left) > 0xFF or len(encoded) > 0xFF:
            raise ValueError("property-pair table exceeds frozen offset/length representation")
        pair_index.extend(struct.pack("<HBB", len(pair_blob), len(left), len(encoded)))
        pair_blob.extend(encoded)

    sections: list[tuple[str, bytes]] = [
        (
            "displacements",
            b"".join(struct.pack("<H", value) for value in perfect.displacements),
        ),
        ("slots", b"".join(struct.pack("<H", value) for value in perfect.slots)),
        (
            "state_descriptors",
            b"".join(struct.pack("<I", value) for value in descriptors),
        ),
        ("property_ids", pack_nine_bit(occurrences)),
        ("name_index", bytes(name_index)),
        ("pair_index", bytes(pair_index)),
        ("name_blob", bytes(name_blob)),
        ("pair_blob", bytes(pair_blob)),
    ]
    offsets: dict[str, dict[str, int]] = {}
    binary_parts: list[bytes] = []
    cursor = 0
    for name, payload in sections:
        offsets[name] = {"offset": cursor, "bytes": len(payload)}
        binary_parts.append(payload)
        cursor += len(payload)
    binary = b"".join(binary_parts)

    _validate_generated_model(
        binary,
        parsed,
        fingerprints,
        perfect,
        offsets,
        len(names),
        len(pairs),
        len(occurrences),
    )

    generation_digest = manifest.get("generation_digest")
    if not isinstance(generation_digest, str) or len(generation_digest) != 64:
        raise ValueError("state-data manifest has invalid generation_digest")

    result: dict[str, object] = {
        "schema": SCHEMA,
        "generator_version": GENERATOR_VERSION,
        "layout": LAYOUT,
        "hash_algorithm": HASH_ALGORITHM,
        "slot_hash_algorithm": SLOT_HASH_ALGORITHM,
        "target": manifest.get("target"),
        "assignment_policy": "vanilla-identity",
        "state_count": len(states),
        "state_data_input_sha256": input_digest,
        "state_data_generation_sha256": generation_digest,
        "bucket_count": perfect.bucket_count,
        "slot_count": perfect.slot_count,
        "max_displacement": perfect.max_displacement,
        "name_count": len(names),
        "property_pair_count": len(pairs),
        "property_occurrences": len(occurrences),
        "max_properties_per_state": max_properties,
        "binary_bytes": len(binary),
        "binary_sha256": sha256_bytes(binary),
        "offsets": offsets,
    }
    rust = _render_rust(result)
    result["rust_sha256"] = sha256_text(rust)
    result["lookup_digest"] = state_data.digest(result)
    return rust, binary, result


def _validate_generated_model(
    binary: bytes,
    parsed: list[tuple[str, tuple[tuple[str, str], ...]]],
    fingerprints: list[int],
    perfect: PerfectHash,
    offsets: dict[str, dict[str, int]],
    name_count: int,
    pair_count: int,
    occurrence_count: int,
) -> None:
    def read_u16(offset: int) -> int:
        return int.from_bytes(binary[offset : offset + 2], "little")

    def read_u32(offset: int) -> int:
        return int.from_bytes(binary[offset : offset + 4], "little")

    def matches(
        state_id: int, name: str, properties: tuple[tuple[str, str], ...]
    ) -> bool:
        descriptor_base = offsets["state_descriptors"]["offset"]
        descriptor = read_u32(descriptor_base + state_id * 4)
        name_id = descriptor & 0x7FF
        property_start = (descriptor >> 11) & 0x3FFFF
        property_len = descriptor >> 29
        if property_len != len(properties) or name_id >= name_count:
            return False

        index_offset = offsets["name_index"]["offset"] + name_id * 3
        blob_offset = read_u16(index_offset)
        name_len = binary[index_offset + 2]
        blob_base = offsets["name_blob"]["offset"]
        if binary[blob_base + blob_offset : blob_base + blob_offset + name_len] != name.encode():
            return False

        property_data = binary[
            offsets["property_ids"]["offset"] : offsets["property_ids"]["offset"]
            + offsets["property_ids"]["bytes"]
        ]
        pair_index_base = offsets["pair_index"]["offset"]
        pair_blob_base = offsets["pair_blob"]["offset"]
        for index, (property_name, value) in enumerate(properties):
            occurrence = property_start + index
            if occurrence >= occurrence_count:
                return False
            pair_id = read_nine_bit(property_data, occurrence)
            if pair_id >= pair_count:
                return False
            pair_index_offset = pair_index_base + pair_id * 4
            pair_offset = read_u16(pair_index_offset)
            property_name_len = binary[pair_index_offset + 2]
            total_len = binary[pair_index_offset + 3]
            pair = binary[
                pair_blob_base + pair_offset : pair_blob_base + pair_offset + total_len
            ]
            left = property_name.encode()
            if (
                len(left) != property_name_len
                or pair[:property_name_len] != left
                or pair[property_name_len : property_name_len + 1] != b"="
                or pair[property_name_len + 1 :] != value.encode()
            ):
                return False
        return True

    displacement_base = offsets["displacements"]["offset"]
    slot_base = offsets["slots"]["offset"]
    for state_id, ((name, properties), fingerprint) in enumerate(
        zip(parsed, fingerprints, strict=True)
    ):
        bucket = fingerprint & (perfect.bucket_count - 1)
        displacement = read_u16(displacement_base + bucket * 2)
        slot = splitmix64(fingerprint ^ displacement) & (perfect.slot_count - 1)
        candidate = read_u16(slot_base + slot * 2)
        if candidate != state_id or not matches(candidate, name, properties):
            raise ValueError(f"generated lookup failed exact state {state_id}")


def _render_rust(result: dict[str, object]) -> str:
    offsets = result["offsets"]
    assert isinstance(offsets, dict)

    def offset(name: str) -> int:
        value = offsets[name]
        assert isinstance(value, dict)
        raw = value["offset"]
        assert isinstance(raw, int)
        return raw

    bucket_count = int(result["bucket_count"])
    slot_count = int(result["slot_count"])
    return f'''//! @generated by `tools/stored_state_lookup.py` {GENERATOR_VERSION}; do not edit by hand.
//! Exact cold persisted-state lookup metadata for Minecraft Java 26.2.

pub(crate) const STORED_STATE_LOOKUP_INPUT_SHA256: &str =
    "{result['state_data_input_sha256']}";
pub(crate) const STORED_STATE_LOOKUP_BINARY_SHA256: &str =
    "{result['binary_sha256']}";
pub(crate) const STORED_STATE_LOOKUP_BYTES: usize = {result['binary_bytes']};
pub(crate) const STORED_STATE_LOOKUP_COUNT: usize = {result['state_count']};
pub(crate) const STORED_STATE_BUCKET_COUNT: usize = {bucket_count};
pub(crate) const STORED_STATE_BUCKET_MASK: u64 = {bucket_count - 1};
pub(crate) const STORED_STATE_SLOT_COUNT: usize = {slot_count};
pub(crate) const STORED_STATE_SLOT_MASK: u64 = {slot_count - 1};
pub(crate) const STORED_STATE_NAME_COUNT: usize = {result['name_count']};
pub(crate) const STORED_STATE_PROPERTY_PAIR_COUNT: usize = {result['property_pair_count']};
pub(crate) const STORED_STATE_PROPERTY_OCCURRENCES: usize = {result['property_occurrences']};
pub(crate) const STORED_STATE_MAX_PROPERTIES: usize = {result['max_properties_per_state']};
pub(crate) const STORED_STATE_DISPLACEMENTS_OFFSET: usize = {offset('displacements')};
pub(crate) const STORED_STATE_SLOTS_OFFSET: usize = {offset('slots')};
pub(crate) const STORED_STATE_DESCRIPTORS_OFFSET: usize = {offset('state_descriptors')};
pub(crate) const STORED_STATE_PROPERTY_IDS_OFFSET: usize = {offset('property_ids')};
pub(crate) const STORED_STATE_NAME_INDEX_OFFSET: usize = {offset('name_index')};
pub(crate) const STORED_STATE_PAIR_INDEX_OFFSET: usize = {offset('pair_index')};
pub(crate) const STORED_STATE_NAME_BLOB_OFFSET: usize = {offset('name_blob')};
pub(crate) const STORED_STATE_PAIR_BLOB_OFFSET: usize = {offset('pair_blob')};

pub(crate) static STORED_STATE_LOOKUP_DATA: &[u8; STORED_STATE_LOOKUP_BYTES] =
    include_bytes!("generated/26.2-stored-state-lookup.bin");
'''


def rendered_manifest(value: dict[str, object]) -> str:
    return json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def command_inspect(args: argparse.Namespace) -> int:
    data = state_data.load(Path(args.input))
    manifest = load_manifest(Path(args.state_manifest))
    _, _, result = build_artifacts(data, manifest)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


def command_generate(args: argparse.Namespace) -> int:
    data = state_data.load(Path(args.input))
    manifest = load_manifest(Path(args.state_manifest))
    rust, binary, result = build_artifacts(data, manifest)
    rust_output = Path(args.output)
    binary_output = Path(args.binary_output)
    result_path = Path(args.manifest)
    for path in (rust_output, binary_output, result_path):
        path.parent.mkdir(parents=True, exist_ok=True)
    rust_output.write_text(rust, encoding="utf-8")
    binary_output.write_bytes(binary)
    result_path.write_text(rendered_manifest(result), encoding="utf-8")
    print(
        f"generated {result['state_count']} exact states / {result['binary_bytes']} bytes / "
        f"sha256={result['binary_sha256']}"
    )
    return 0


def command_verify(args: argparse.Namespace) -> int:
    data = state_data.load(Path(args.input))
    manifest = load_manifest(Path(args.state_manifest))
    rust, binary, result = build_artifacts(data, manifest)
    checks: tuple[tuple[Path, bytes], ...] = (
        (Path(args.output), rust.encode("utf-8")),
        (Path(args.binary_output), binary),
        (Path(args.manifest), rendered_manifest(result).encode("utf-8")),
    )
    ok = True
    for path, expected in checks:
        if not path.is_file() or path.read_bytes() != expected:
            print(f"stale generated file: {path}", file=sys.stderr)
            ok = False
    return 0 if ok else 1


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Helve exact cold stored block-state lookup generator"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    for name, handler in {
        "inspect": command_inspect,
        "generate": command_generate,
        "verify": command_verify,
    }.items():
        subparser = subparsers.add_parser(name)
        subparser.add_argument("input")
        subparser.add_argument(
            "--state-manifest",
            default="vanilla/state-data/26.2-state-data-manifest.json",
        )
        if name in {"generate", "verify"}:
            subparser.add_argument("--output", required=True)
            subparser.add_argument("--binary-output", required=True)
            subparser.add_argument("--manifest", required=True)
        subparser.set_defaults(handler=handler)
    args = parser.parse_args()
    try:
        return int(args.handler(args))
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
