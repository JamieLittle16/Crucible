#!/usr/bin/env python3
"""Fail-closed CI policy checks for third-party code and dependencies."""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github" / "workflows"
LOCKFILE = ROOT / "Cargo.lock"
ALLOWLIST = ROOT / "config" / "dependency-allowlist.txt"
FULL_SHA = re.compile(r"^[0-9a-f]{40}$")
ALLOWLIST_ENTRY = re.compile(r"^[A-Za-z0-9_-]+@[0-9A-Za-z.+_-]+$")
CRATES_IO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"


def workflow_action_errors(workflows: Path = WORKFLOWS) -> list[str]:
    errors: list[str] = []
    for path in sorted(workflows.glob("*.yml")):
        for line_number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            stripped = raw.strip()
            if "uses:" not in stripped:
                continue
            _, value = stripped.split("uses:", 1)
            value = value.strip()
            if " #" in value:
                value = value.split(" #", 1)[0].rstrip()
            value = value.strip("'\"")
            if value.startswith("./"):
                continue
            if "@" not in value:
                errors.append(f"{path.relative_to(ROOT)}:{line_number}: action is not pinned: {value}")
                continue
            action, revision = value.rsplit("@", 1)
            if not action or FULL_SHA.fullmatch(revision) is None:
                errors.append(
                    f"{path.relative_to(ROOT)}:{line_number}: external action must use a full 40-hex commit SHA: {value}"
                )
    return errors


def load_allowlist(path: Path = ALLOWLIST) -> tuple[set[str], list[str]]:
    errors: list[str] = []
    entries: set[str] = set()
    for line_number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        value = raw.split("#", 1)[0].strip()
        if not value:
            continue
        if ALLOWLIST_ENTRY.fullmatch(value) is None:
            errors.append(f"{path.relative_to(ROOT)}:{line_number}: invalid allowlist entry {value!r}")
            continue
        if value in entries:
            errors.append(f"{path.relative_to(ROOT)}:{line_number}: duplicate allowlist entry {value}")
        entries.add(value)
    return entries, errors


def dependency_errors(
    lockfile: Path = LOCKFILE,
    allowlist_path: Path = ALLOWLIST,
) -> list[str]:
    errors: list[str] = []
    allowlist, allowlist_errors = load_allowlist(allowlist_path)
    errors.extend(allowlist_errors)

    with lockfile.open("rb") as handle:
        lock = tomllib.load(handle)
    packages = lock.get("package")
    if not isinstance(packages, list):
        return errors + ["Cargo.lock does not contain a package array"]

    observed_registry: set[str] = set()
    for package in packages:
        if not isinstance(package, dict):
            errors.append("Cargo.lock contains a non-table package entry")
            continue
        source = package.get("source")
        if source is None:
            continue
        name = package.get("name")
        version = package.get("version")
        if not isinstance(name, str) or not isinstance(version, str) or not isinstance(source, str):
            errors.append("Cargo.lock external package has malformed name/version/source")
            continue
        identity = f"{name}@{version}"
        if source.startswith("git+"):
            errors.append(f"git dependency is forbidden: {identity} ({source})")
            continue
        if source != CRATES_IO_SOURCE:
            errors.append(f"unapproved dependency source for {identity}: {source}")
            continue
        observed_registry.add(identity)
        if identity not in allowlist:
            errors.append(
                f"unreviewed crates.io dependency: {identity}; add an exact reviewed entry to config/dependency-allowlist.txt"
            )

    for stale in sorted(allowlist - observed_registry):
        errors.append(f"stale dependency allowlist entry: {stale}")
    return errors


def check() -> list[str]:
    errors = workflow_action_errors()
    errors.extend(dependency_errors())
    return errors


def main() -> int:
    errors = check()
    if errors:
        for error in errors:
            print(f"CI policy failure: {error}", file=sys.stderr)
        return 1
    print("CI policy: immutable Actions and dependency allowlist checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
