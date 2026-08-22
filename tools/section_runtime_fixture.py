#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

MINECRAFT_VERSION = "26.2"
PROTOCOL_VERSION = 776
DATA_VERSION = 4903
FIXTURE_MAGIC = "CRUCIBLE-SECTION-SEMANTIC-FIXTURE"
FIXTURE_SCHEMA = 1
RUNTIME_SCHEMA = 1
EVIDENCE_SCHEMA = 1
SOURCE_ARCHIVE_SHA256 = "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
SOURCE_QUALIFICATION_SHA256 = "5d312d6025fa6556feaf5fa26c80577dcb024e7e5be5cd1bda98d101367600c8"
RUNTIME_SERVER_SHA256 = "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5"
STATE_GENERATION_SHA256 = "79e5803347d6fb6f7ffccea4cef783998a1c6469ed869d26fa48ab5f2328cd3b"
RUNTIME_PROBE = "official-runtime-reflection-probe-v1"


@dataclass(frozen=True)
class StateFacts:
    key: str
    vanilla_id: int
    non_air: bool
    counted_fluid: bool
    random_block: bool
    random_fluid: bool

    @property
    def flags(self) -> int:
        return (
            int(self.non_air)
            | (int(self.counted_fluid) << 1)
            | (int(self.random_block) << 2)
            | (int(self.random_fluid) << 3)
        )


@dataclass(frozen=True)
class FixtureState:
    label: str
    flags: int


@dataclass(frozen=True)
class FixtureCase:
    kind: str
    name: str
    state_label: str | None
    cell: int | None
    expected: tuple[int, int, bool, bool] | None


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_json(path: Path) -> tuple[dict[str, Any], bytes]:
    raw = path.read_bytes()
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise ValueError(f"{path}: top-level JSON must be an object")
    return value, raw


def require_target(target: Any, label: str) -> None:
    if target != {
        "minecraft_version": MINECRAFT_VERSION,
        "protocol_version": PROTOCOL_VERSION,
        "data_version": DATA_VERSION,
    }:
        raise ValueError(f"{label}: target pin differs from Minecraft 26.2 / 776 / 4903")


def load_runtime(path: Path, *, expected_state_count: int) -> tuple[list[StateFacts], str]:
    data, raw = read_json(path)
    if data.get("schema") != RUNTIME_SCHEMA:
        raise ValueError("runtime data: unsupported schema")
    require_target(data.get("target"), "runtime data")
    if data.get("air_key") != "minecraft:air":
        raise ValueError("runtime data: air key differs from minecraft:air")

    provenance = data.get("provenance")
    if not isinstance(provenance, dict):
        raise ValueError("runtime data: missing provenance")
    if provenance.get("server_sha256") != RUNTIME_SERVER_SHA256:
        raise ValueError("runtime data: official server SHA-256 differs from pin")
    if provenance.get("source") != RUNTIME_PROBE:
        raise ValueError("runtime data: unexpected probe identity")
    if provenance.get("startup_sequence") != [
        "SharedConstants.tryDetectVersion",
        "Bootstrap.bootStrap",
    ]:
        raise ValueError("runtime data: startup sequence differs from qualified probe")

    raw_states = data.get("states")
    if not isinstance(raw_states, list):
        raise ValueError("runtime data: states must be a list")
    if len(raw_states) != expected_state_count:
        raise ValueError(
            f"runtime data: expected {expected_state_count} states, got {len(raw_states)}"
        )

    states: list[StateFacts] = []
    seen_keys: set[str] = set()
    for expected_id, state in enumerate(raw_states):
        if not isinstance(state, dict):
            raise ValueError(f"runtime data: state {expected_id} is not an object")
        vanilla_id = state.get("vanilla_id")
        if vanilla_id != expected_id:
            raise ValueError(
                f"runtime data: state IDs are not dense at {expected_id}: got {vanilla_id!r}"
            )
        key = state.get("key")
        if not isinstance(key, str) or not key:
            raise ValueError(f"runtime data: state {expected_id} has invalid key")
        if key in seen_keys:
            raise ValueError(f"runtime data: duplicate state key {key}")
        seen_keys.add(key)

        values = {}
        for fact in ("non_air", "counted_fluid", "random_block", "random_fluid"):
            value = state.get(fact)
            if not isinstance(value, bool):
                raise ValueError(f"runtime data: state {expected_id} has non-boolean {fact}")
            values[fact] = value
        facts = StateFacts(key=key, vanilla_id=expected_id, **values)
        if facts.counted_fluid and not facts.non_air:
            raise ValueError("runtime data: counted_fluid implies non_air")
        if facts.random_block and not facts.non_air:
            raise ValueError("runtime data: random_block implies non_air")
        if facts.random_fluid and not facts.counted_fluid:
            raise ValueError("runtime data: random_fluid implies counted_fluid")
        states.append(facts)

    if states[0].key != "minecraft:air" or states[0].flags != 0:
        raise ValueError("runtime data: state 0 is not semantic air")
    return states, sha256_bytes(raw)


def load_expected_state_count(manifest_path: Path) -> int:
    manifest, _ = read_json(manifest_path)
    require_target(manifest.get("target"), "state-data manifest")
    if manifest.get("generation_digest") != STATE_GENERATION_SHA256:
        raise ValueError("state-data manifest: generation digest differs from pin")
    state_count = manifest.get("state_count")
    if not isinstance(state_count, int) or state_count <= 0:
        raise ValueError("state-data manifest: invalid state_count")
    return state_count


def parse_bool(value: str, label: str) -> bool:
    if value == "0":
        return False
    if value == "1":
        return True
    raise ValueError(f"fixture: invalid {label}: {value!r}")


def parse_fixture(path: Path) -> tuple[list[FixtureState], list[FixtureCase], str]:
    raw = path.read_bytes()
    text = raw.decode("utf-8")
    lines = [line for line in text.splitlines() if line and not line.startswith("#")]
    if len(lines) < 3:
        raise ValueError("fixture: expected header, provenance and semantic cases")

    header = lines[0].split("|")
    if header != [
        FIXTURE_MAGIC,
        str(FIXTURE_SCHEMA),
        MINECRAFT_VERSION,
        str(PROTOCOL_VERSION),
        str(DATA_VERSION),
    ]:
        raise ValueError("fixture: target/schema header differs from pin")

    provenance = lines[1].split("|")
    if provenance != [
        "PROVENANCE",
        SOURCE_ARCHIVE_SHA256,
        SOURCE_QUALIFICATION_SHA256,
        RUNTIME_SERVER_SHA256,
        STATE_GENERATION_SHA256,
    ]:
        raise ValueError("fixture: provenance differs from qualified evidence")

    states: list[FixtureState] = []
    cases: list[FixtureCase] = []
    labels: set[str] = set()
    for line_number, line in enumerate(lines[2:], start=3):
        parts = line.split("|")
        kind = parts[0]
        if kind == "STATE" and len(parts) == 3:
            label = parts[1]
            if not label or label in labels:
                raise ValueError(f"fixture line {line_number}: duplicate/empty state label")
            try:
                flags = int(parts[2])
            except ValueError as error:
                raise ValueError(f"fixture line {line_number}: invalid flags") from error
            if not 0 <= flags <= 15:
                raise ValueError(f"fixture line {line_number}: flags outside four-bit domain")
            labels.add(label)
            states.append(FixtureState(label, flags))
            continue

        if kind == "BLOCK-FILL" and len(parts) == 7:
            cases.append(
                FixtureCase(
                    kind=kind,
                    name=parts[1],
                    state_label=parts[2],
                    cell=None,
                    expected=(
                        int(parts[3]),
                        int(parts[4]),
                        parse_bool(parts[5], "random-block presence"),
                        parse_bool(parts[6], "random-fluid presence"),
                    ),
                )
            )
            continue
        if kind == "BLOCK-ONE" and len(parts) == 8:
            cases.append(
                FixtureCase(
                    kind=kind,
                    name=parts[1],
                    state_label=parts[2],
                    cell=int(parts[3]),
                    expected=(
                        int(parts[4]),
                        int(parts[5]),
                        parse_bool(parts[6], "random-block presence"),
                        parse_bool(parts[7], "random-fluid presence"),
                    ),
                )
            )
            continue
        if kind == "BLOCK-REVERSE" and len(parts) == 4:
            cases.append(
                FixtureCase(
                    kind=kind,
                    name=parts[1],
                    state_label=parts[2],
                    cell=int(parts[3]),
                    expected=(0, 0, False, False),
                )
            )
            continue
        if kind in {"BIOME-FILL-ORDER", "BIOME-REPLACE"}:
            # These are source-backed fixtures. The state-data runtime probe does not observe biome
            # resolver order or section biome mutation, so they are intentionally not upgraded to
            # runtime-fact evidence by this tool.
            continue
        raise ValueError(f"fixture line {line_number}: unsupported record")

    if not states or not cases:
        raise ValueError("fixture: expected state bindings and block cases")
    return states, cases, sha256_bytes(raw)


def expected_summary(facts: StateFacts, multiplicity: int) -> tuple[int, int, bool, bool]:
    return (
        multiplicity if facts.non_air else 0,
        multiplicity if facts.counted_fluid else 0,
        facts.random_block and multiplicity > 0,
        facts.random_fluid and multiplicity > 0,
    )


def qualify(
    runtime_path: Path,
    fixture_path: Path,
    manifest_path: Path,
) -> dict[str, Any]:
    state_count = load_expected_state_count(manifest_path)
    states, runtime_digest = load_runtime(runtime_path, expected_state_count=state_count)
    fixture_states, cases, fixture_digest = parse_fixture(fixture_path)

    by_flags: dict[int, StateFacts] = {}
    for state in states:
        by_flags.setdefault(state.flags, state)

    representatives: dict[str, StateFacts] = {}
    for fixture_state in fixture_states:
        state = by_flags.get(fixture_state.flags)
        if state is None:
            raise ValueError(
                f"fixture state {fixture_state.label!r}: official runtime has no state with flags "
                f"{fixture_state.flags}"
            )
        representatives[fixture_state.label] = state

    checked_cases: list[dict[str, Any]] = []
    for case in cases:
        assert case.state_label is not None
        facts = representatives.get(case.state_label)
        if facts is None:
            raise ValueError(f"fixture case {case.name!r}: unknown state label {case.state_label!r}")
        if case.cell is not None and not 0 <= case.cell < 4096:
            raise ValueError(f"fixture case {case.name!r}: cell outside 4096-cell domain")

        if case.kind == "BLOCK-FILL":
            observed = expected_summary(facts, 4096)
        elif case.kind == "BLOCK-ONE":
            observed = expected_summary(facts, 1)
        elif case.kind == "BLOCK-REVERSE":
            observed = (0, 0, False, False)
        else:
            raise AssertionError(case.kind)
        if observed != case.expected:
            raise ValueError(
                f"fixture case {case.name!r}: expected {case.expected}, official-runtime facts imply {observed}"
            )
        checked_cases.append(
            {
                "name": case.name,
                "kind": case.kind,
                "state_label": case.state_label,
                "representative_vanilla_id": facts.vanilla_id,
                "representative_key": facts.key,
                "flags": facts.flags,
            }
        )

    return {
        "schema": EVIDENCE_SCHEMA,
        "qualification": "official-runtime-facts-fixture",
        "target": {
            "minecraft_version": MINECRAFT_VERSION,
            "protocol_version": PROTOCOL_VERSION,
            "data_version": DATA_VERSION,
        },
        "runtime_probe": RUNTIME_PROBE,
        "runtime_server_sha256": RUNTIME_SERVER_SHA256,
        "runtime_input_sha256": runtime_digest,
        "source_archive_sha256": SOURCE_ARCHIVE_SHA256,
        "source_qualification_sha256": SOURCE_QUALIFICATION_SHA256,
        "state_data_generation_sha256": STATE_GENERATION_SHA256,
        "fixture_sha256": fixture_digest,
        "state_count": state_count,
        "runtime_checked_block_cases": checked_cases,
        "runtime_checked_sem_ids": [
            "SEM-WORLD-SECTION-005",
            "SEM-WORLD-SECTION-006",
            "SEM-WORLD-SECTION-007",
            "SEM-WORLD-SECTION-008",
            "SEM-WORLD-SECTION-009",
            "SEM-WORLD-SECTION-010",
            "SEM-WORLD-SECTION-012",
        ],
        "source_only_sem_ids": [
            "SEM-WORLD-SECTION-003",
            "SEM-WORLD-SECTION-004",
            "SEM-WORLD-SECTION-015",
            "SEM-WORLD-SECTION-016",
        ],
    }


def render(evidence: dict[str, Any]) -> str:
    return json.dumps(evidence, indent=2, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Bind section semantic fixtures to the pinned official 26.2 runtime facts"
    )
    parser.add_argument("--runtime-data", required=True)
    parser.add_argument("--fixture", required=True)
    parser.add_argument(
        "--manifest", default="vanilla/state-data/26.2-state-data-manifest.json"
    )
    parser.add_argument("--output", required=True)
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()

    try:
        evidence = qualify(
            Path(args.runtime_data), Path(args.fixture), Path(args.manifest)
        )
        output = Path(args.output)
        expected = render(evidence)
        if args.verify:
            if not output.is_file():
                raise ValueError(f"verification output is missing: {output}")
            if output.read_text(encoding="utf-8") != expected:
                raise ValueError(f"runtime fixture evidence is stale: {output}")
            print(
                f"verified runtime fixture evidence: {len(evidence['runtime_checked_block_cases'])} "
                f"block cases / {evidence['state_count']} states"
            )
            return 0
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(expected, encoding="utf-8")
        print(
            f"qualified runtime fixture evidence: {len(evidence['runtime_checked_block_cases'])} "
            f"block cases / {evidence['state_count']} states -> {output}"
        )
        return 0
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
