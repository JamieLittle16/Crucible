#!/usr/bin/env python3
"""Finalize the pinned target block-state substrate from both official oracles.

This is orchestration only. The three underlying tools remain independently testable:

1. `state_source_qualification.py` resolves/fingerprints the pinned official source Atlas.
2. `official_state_data.py` observes the complete target registry in the official runtime.
3. `qualify_state_data.py` joins those independent evidence paths.
4. `state_data.py` generates the compact Rust target substrate.

Ordinary builds consume only the committed generated Rust/manifest. They never require Mojang
artifacts or the local Atlas.
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Sequence

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ROOT / "tools"
DEFAULT_LOCK = ROOT / "vanilla" / "vanilla.lock.toml"
DEFAULT_SPEC = ROOT / "vanilla" / "state-data" / "source-qualification-spec.json"
DEFAULT_ATLAS = ROOT / ".crucible" / "vanilla" / "atlas.sqlite"
DEFAULT_SOURCE_QUALIFICATION = (
    ROOT / "vanilla" / "state-data" / "26.2-source-qualification.json"
)
DEFAULT_RAW_RUNTIME = ROOT / ".crucible" / "vanilla" / "26.2-block-states.raw.json"
DEFAULT_QUALIFIED_RUNTIME = (
    ROOT / ".crucible" / "vanilla" / "26.2-block-states.qualified.json"
)
DEFAULT_RUST_OUTPUT = ROOT / "crates" / "data" / "crucible-generated" / "src" / "lib.rs"
DEFAULT_MANIFEST = ROOT / "vanilla" / "state-data" / "26.2-state-data-manifest.json"


def run(argv: Sequence[str]) -> None:
    completed = subprocess.run(list(argv), cwd=ROOT, check=False)
    if completed.returncode != 0:
        raise ValueError(
            f"command failed with exit {completed.returncode}: "
            + " ".join(str(part) for part in argv)
        )


def load_lock(path: Path) -> dict[str, object]:
    with path.open("rb") as handle:
        value = tomllib.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"expected TOML table in {path}")
    return value


def load_json(path: Path) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object in {path}")
    return value


def qualify_source(
    *,
    lock: Path,
    spec: Path,
    atlas: Path,
    output: Path,
    verify: bool,
) -> None:
    argv = [
        sys.executable,
        str(TOOLS / "state_source_qualification.py"),
        "--lock",
        str(lock),
        "--spec",
        str(spec),
        "--atlas",
        str(atlas),
        "--output",
        str(output),
    ]
    if verify:
        argv.append("--verify")
    run(argv)


def obtain_runtime(
    *,
    lock: Path,
    raw_runtime: Path,
    provided_runtime: Path | None,
    cache: Path,
    verify: bool,
) -> None:
    if provided_runtime is not None:
        if not provided_runtime.is_file():
            raise ValueError(f"provided runtime data does not exist: {provided_runtime}")
        if provided_runtime.resolve() != raw_runtime.resolve():
            raw_runtime.parent.mkdir(parents=True, exist_ok=True)
            raw_runtime.write_bytes(provided_runtime.read_bytes())
        return

    if verify:
        if not raw_runtime.is_file():
            raise ValueError(
                "verify requires an existing raw runtime dataset or --runtime-data"
            )
        return

    lock_data = load_lock(lock)
    version = str(lock_data["minecraft"])
    run(
        [
            sys.executable,
            str(TOOLS / "official_state_data.py"),
            "--version",
            version,
            "--cache",
            str(cache),
            "--output",
            str(raw_runtime),
        ]
    )


def bind_runtime(
    *,
    lock: Path,
    spec: Path,
    source_qualification: Path,
    raw_runtime: Path,
    qualified_runtime: Path,
    verify: bool,
) -> None:
    argv = [
        sys.executable,
        str(TOOLS / "qualify_state_data.py"),
        "--runtime-data",
        str(raw_runtime),
        "--source-qualification",
        str(source_qualification),
        "--spec",
        str(spec),
        "--lock",
        str(lock),
        "--output",
        str(qualified_runtime),
    ]
    if verify:
        argv.append("--verify")
    run(argv)


def generate(
    *,
    qualified_runtime: Path,
    rust_output: Path,
    manifest: Path,
    assignment: str,
    verify: bool,
) -> None:
    command = "verify" if verify else "generate"
    run(
        [
            sys.executable,
            str(TOOLS / "state_data.py"),
            command,
            str(qualified_runtime),
            "--assignment",
            assignment,
            "--output",
            str(rust_output),
            "--manifest",
            str(manifest),
        ]
    )


def validate_final_chain(
    *,
    lock: Path,
    source_qualification: Path,
    qualified_runtime: Path,
    manifest: Path,
) -> None:
    lock_data = load_lock(lock)
    source = load_json(source_qualification)
    qualified = load_json(qualified_runtime)
    generated = load_json(manifest)

    expected_target = {
        "minecraft_version": str(lock_data["minecraft"]),
        "protocol_version": int(lock_data["protocol"]),
        "data_version": int(lock_data["data_version"]),
    }
    for label, value in (
        ("source qualification", source.get("target")),
        ("qualified dataset", qualified.get("target")),
        ("generated manifest", generated.get("target")),
    ):
        if value != expected_target:
            raise ValueError(f"{label} target differs from vanilla.lock.toml")

    provenance = qualified.get("provenance")
    if not isinstance(provenance, dict):
        raise ValueError("qualified dataset is missing provenance")
    if provenance.get("source_qualification_digest") != source.get("qualification_digest"):
        raise ValueError("qualified dataset is not bound to the selected source qualification")

    generated_provenance = generated.get("source_provenance")
    if generated_provenance != provenance:
        raise ValueError("generated manifest did not preserve qualified dataset provenance")

    state_count = generated.get("state_count")
    representation = generated.get("repr")
    mapping = generated.get("mapping")
    if not isinstance(state_count, int) or state_count <= 0:
        raise ValueError("generated manifest has invalid state count")
    if representation not in {"u16", "u32"}:
        raise ValueError("generated manifest has invalid state representation")
    if mapping not in {"identity", "translated"}:
        raise ValueError("generated manifest has invalid external mapping mode")

    print(
        "qualified target state substrate: "
        f"{state_count} states / {representation} / {mapping} mapping / "
        f"generation {generated['generation_digest']}"
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Finalize source+runtime-qualified target block-state data"
    )
    parser.add_argument("--lock", default=str(DEFAULT_LOCK))
    parser.add_argument("--spec", default=str(DEFAULT_SPEC))
    parser.add_argument("--atlas", default=str(DEFAULT_ATLAS))
    parser.add_argument(
        "--source-qualification",
        default=str(DEFAULT_SOURCE_QUALIFICATION),
    )
    parser.add_argument("--runtime-data")
    parser.add_argument("--raw-runtime", default=str(DEFAULT_RAW_RUNTIME))
    parser.add_argument("--qualified-runtime", default=str(DEFAULT_QUALIFIED_RUNTIME))
    parser.add_argument("--rust-output", default=str(DEFAULT_RUST_OUTPUT))
    parser.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    parser.add_argument(
        "--cache",
        default=str(ROOT / ".crucible" / "vanilla" / "downloads"),
    )
    parser.add_argument(
        "--assignment",
        choices=("vanilla-identity", "canonical-key"),
        default="vanilla-identity",
    )
    parser.add_argument(
        "--verify",
        action="store_true",
        help="verify every existing artifact byte-for-byte instead of regenerating",
    )
    args = parser.parse_args()

    lock = Path(args.lock)
    spec = Path(args.spec)
    atlas = Path(args.atlas)
    source_qualification = Path(args.source_qualification)
    raw_runtime = Path(args.raw_runtime)
    qualified_runtime = Path(args.qualified_runtime)
    rust_output = Path(args.rust_output)
    manifest = Path(args.manifest)
    provided_runtime = Path(args.runtime_data) if args.runtime_data else None
    cache = Path(args.cache)

    try:
        qualify_source(
            lock=lock,
            spec=spec,
            atlas=atlas,
            output=source_qualification,
            verify=args.verify,
        )
        obtain_runtime(
            lock=lock,
            raw_runtime=raw_runtime,
            provided_runtime=provided_runtime,
            cache=cache,
            verify=args.verify,
        )
        bind_runtime(
            lock=lock,
            spec=spec,
            source_qualification=source_qualification,
            raw_runtime=raw_runtime,
            qualified_runtime=qualified_runtime,
            verify=args.verify,
        )
        generate(
            qualified_runtime=qualified_runtime,
            rust_output=rust_output,
            manifest=manifest,
            assignment=args.assignment,
            verify=args.verify,
        )
        validate_final_chain(
            lock=lock,
            source_qualification=source_qualification,
            qualified_runtime=qualified_runtime,
            manifest=manifest,
        )
        return 0
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
