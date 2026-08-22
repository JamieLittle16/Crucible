#!/usr/bin/env python3
"""Fail-closed provenance verifier for Crucible section fixtures."""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path

REQUIRED_SOURCE_VARS = {
    "VAR-WORLD-SECTION-001",
    "VAR-WORLD-SECTION-003",
    "VAR-WORLD-SECTION-004",
    "VAR-WORLD-SECTION-005",
    "VAR-WORLD-SECTION-006",
    "VAR-WORLD-SECTION-007",
    "VAR-WORLD-SECTION-008",
    "VAR-WORLD-SECTION-009",
}


class FixtureError(ValueError):
    """Fixture provenance does not match committed qualification evidence."""


def parse_fixture(path: Path) -> tuple[str, dict[str, str], dict[str, tuple[str, str]]]:
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines:
        raise FixtureError("fixture is empty")
    header = lines[0].split("|")
    if len(header) != 4 or header[0] != "CRUCIBLE-SECTION-FIXTURE" or header[1] != "1":
        raise FixtureError("invalid section fixture header")
    provenance = header[3]
    if provenance not in {"source-reviewed", "runtime-observed"}:
        raise FixtureError(f"unknown fixture provenance: {provenance}")

    metadata: dict[str, str] = {}
    bindings: dict[str, tuple[str, str]] = {}
    for line_number, line in enumerate(lines[1:], start=2):
        if not line or line.startswith("#"):
            continue
        parts = line.split("|")
        if parts[0] == "M" and len(parts) == 3:
            if parts[1] in metadata:
                raise FixtureError(f"duplicate metadata {parts[1]} at line {line_number}")
            metadata[parts[1]] = parts[2]
        elif parts[0] == "V" and len(parts) == 4:
            if parts[1] in bindings:
                raise FixtureError(f"duplicate VAR binding {parts[1]} at line {line_number}")
            bindings[parts[1]] = (parts[2], parts[3])
    return provenance, metadata, bindings


def require(actual: object, expected: object, label: str) -> None:
    if actual != expected:
        raise FixtureError(f"{label} mismatch: expected {expected!r}, got {actual!r}")


def verify(root: Path, fixture_path: Path) -> None:
    provenance, metadata, bindings = parse_fixture(fixture_path)

    with (root / "vanilla/vanilla.lock.toml").open("rb") as handle:
        lock = tomllib.load(handle)
    manifest = json.loads(
        (root / "vanilla/state-data/26.2-state-data-manifest.json").read_text(encoding="utf-8")
    )

    require(metadata.get("minecraft_version"), lock["minecraft"], "fixture Minecraft version")
    require(int(metadata.get("protocol_version", "-1")), lock["protocol"], "fixture protocol")
    require(int(metadata.get("data_version", "-1")), lock["data_version"], "fixture data version")
    require(
        metadata.get("source_archive_sha256"),
        lock["source"]["archive_sha256"],
        "fixture source archive SHA-256",
    )
    require(
        metadata.get("official_server_sha256"),
        lock["runtime"]["server_sha256"],
        "fixture official server SHA-256",
    )
    require(
        metadata.get("state_data_input_sha256"),
        manifest["input_digest"],
        "fixture state-data input digest",
    )
    require(
        metadata.get("state_data_generation_sha256"),
        manifest["generation_digest"],
        "fixture state-data generation digest",
    )

    require(manifest["target"]["minecraft_version"], lock["minecraft"], "manifest Minecraft version")
    require(manifest["target"]["protocol_version"], lock["protocol"], "manifest protocol")
    require(manifest["target"]["data_version"], lock["data_version"], "manifest data version")
    require(
        manifest["source_provenance"]["source_archive_sha256"],
        lock["source"]["archive_sha256"],
        "manifest source archive SHA-256",
    )
    require(
        manifest["source_provenance"]["runtime_server_sha256"],
        lock["runtime"]["server_sha256"],
        "manifest runtime server SHA-256",
    )

    if provenance == "source-reviewed":
        missing = REQUIRED_SOURCE_VARS - bindings.keys()
        if missing:
            raise FixtureError(f"missing required source VAR bindings: {sorted(missing)}")

        for var_id, (normalized_sha, body_sha) in bindings.items():
            record_path = root / "vanilla/records/world/section" / f"{var_id}.json"
            if not record_path.is_file():
                raise FixtureError(f"fixture binds missing VAR record: {var_id}")
            record = json.loads(record_path.read_text(encoding="utf-8"))
            require(record.get("id"), var_id, f"{var_id} record id")
            require(record.get("status"), "VAR_REVIEWED", f"{var_id} review status")
            require(
                record["source"]["fingerprint_algorithm"],
                lock["atlas"]["fingerprint_algorithm"],
                f"{var_id} fingerprint algorithm",
            )
            require(
                normalized_sha,
                record["source"]["normalized_sha256"],
                f"{var_id} normalized SHA-256",
            )
            require(body_sha, record["source"]["body_sha256"], f"{var_id} body SHA-256")
    else:
        if not metadata.get("runtime_probe"):
            raise FixtureError("runtime-observed fixture is missing runtime_probe metadata")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("fixture", type=Path)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        verify(args.root.resolve(), args.fixture.resolve())
    except (FixtureError, KeyError, OSError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    print(f"section fixture provenance verified: {args.fixture}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
