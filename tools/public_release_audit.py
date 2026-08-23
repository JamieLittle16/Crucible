#!/usr/bin/env python3
"""Audit every reachable Git blob before Crucible is made public.

The normal repository guard protects the current tree. This tool is deliberately
history-aware: it also catches credentials or proprietary artifacts that were
committed and later deleted.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

FORBIDDEN_EXACT_PATHS = {"mc-src.zip"}
FORBIDDEN_PREFIXES = (
    ".crucible/",
    "vanilla/source/",
    "vanilla/artifacts/",
    "vanilla/private/",
)
FORBIDDEN_SUFFIXES = (
    ".jar",
    ".jks",
    ".keystore",
    ".p12",
    ".pfx",
    ".pem",
    ".key",
)

# Split literal credential sentinels where necessary so the scanner's own source
# cannot satisfy the signatures it is looking for.
PGP_PRIVATE_KEY_HEADER = b"-----BEGIN PGP PRIVATE " + b"KEY BLOCK-----"
LEGACY_PGP_SCANNER_LITERAL = b're.compile(rb"' + PGP_PRIVATE_KEY_HEADER + b'")'

SECRET_PATTERNS: tuple[tuple[str, re.Pattern[bytes]], ...] = (
    ("GitHub classic token", re.compile(rb"ghp_[A-Za-z0-9]{30,}")),
    ("GitHub fine-grained token", re.compile(rb"github_pat_[A-Za-z0-9_]{30,}")),
    ("AWS access key", re.compile(rb"AKIA[0-9A-Z]{16}")),
    ("Google API key", re.compile(rb"AIza[0-9A-Za-z_-]{35}")),
    ("Slack token", re.compile(rb"xox[baprs]-[A-Za-z0-9-]{10,}")),
    (
        "private key",
        re.compile(rb"-----BEGIN " + rb"(?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----"),
    ),
    ("PGP private key", re.compile(re.escape(PGP_PRIVATE_KEY_HEADER))),
)


@dataclass(frozen=True, order=True)
class Finding:
    blocking: bool
    kind: str
    object_id: str
    path: str
    detail: str


def run_git(root: Path, *args: str, input_bytes: bytes | None = None) -> bytes:
    completed = subprocess.run(
        ["git", "-C", str(root), *args],
        input=input_bytes,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        stderr = completed.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(f"git {' '.join(args)} failed: {stderr}")
    return completed.stdout


def normalize_path(path: str) -> str:
    normalized = path.replace("\\", "/")
    while normalized.startswith("./"):
        normalized = normalized[2:]
    return normalized


def forbidden_path_reason(path: str) -> str | None:
    normalized = normalize_path(path)
    lower = normalized.lower()
    basename = lower.rsplit("/", 1)[-1]
    if lower in FORBIDDEN_EXACT_PATHS:
        return "forbidden Mojang/source archive path"
    if any(lower.startswith(prefix) for prefix in FORBIDDEN_PREFIXES):
        return "forbidden private/source artifact directory"
    if lower.endswith(FORBIDDEN_SUFFIXES):
        return "forbidden binary/key artifact suffix"
    if basename == ".env" or basename.startswith(".env."):
        return "environment-secret file"
    return None


def credential_matches(content: bytes, path: str) -> list[str]:
    """Return credential signatures, excluding only the known legacy self-sentinel.

    Early revisions of this auditor embedded the PGP header literally inside the
    regex that detects PGP private keys. Those historical source blobs are not key
    material. Suppress that one shape only when the scanner path is exact, the old
    source literal is present, and the header occurs exactly once. Any additional
    header occurrence (for example an actual pasted key) remains blocking.
    """

    matches: list[str] = []
    normalized_path = normalize_path(path)
    for name, pattern in SECRET_PATTERNS:
        if pattern.search(content) is None:
            continue
        if (
            name == "PGP private key"
            and normalized_path == "tools/public_release_audit.py"
            and LEGACY_PGP_SCANNER_LITERAL in content
            and content.count(PGP_PRIVATE_KEY_HEADER) == 1
        ):
            continue
        matches.append(name)
    return matches


def historical_paths(root: Path) -> set[str]:
    """Return every pathname mentioned by reachable commit history.

    Enumerate commits first and ask `diff-tree` for NUL-delimited pathnames. This
    avoids `git log --name-only -z`'s presentation separators becoming accidental
    pathname bytes, while preserving arbitrary valid Git pathnames exactly.
    """

    commits = [
        line
        for line in run_git(root, "rev-list", "--all").decode("ascii").splitlines()
        if line
    ]
    paths: set[str] = set()
    for commit in commits:
        raw = run_git(
            root,
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "-r",
            "-z",
            commit,
            "--",
        )
        paths.update(
            normalize_path(value.decode("utf-8", errors="surrogateescape"))
            for value in raw.split(b"\0")
            if value
        )
    return paths


def history_objects(root: Path) -> tuple[dict[str, set[str]], dict[str, tuple[str, int]]]:
    raw = run_git(root, "rev-list", "--objects", "--all")
    paths_by_object: dict[str, set[str]] = {}
    object_ids: list[str] = []
    for line in raw.decode("utf-8", errors="surrogateescape").splitlines():
        if not line:
            continue
        object_id, separator, path = line.partition(" ")
        object_ids.append(object_id)
        if separator and path:
            paths_by_object.setdefault(object_id, set()).add(normalize_path(path))

    unique_ids = sorted(set(object_ids))
    batch = ("\n".join(unique_ids) + "\n").encode("ascii") if unique_ids else b""
    checked = run_git(
        root,
        "cat-file",
        "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        input_bytes=batch,
    )
    metadata: dict[str, tuple[str, int]] = {}
    for line in checked.decode("ascii").splitlines():
        object_id, object_type, size = line.split(" ", 2)
        metadata[object_id] = (object_type, int(size))
    return paths_by_object, metadata


def audit(root: Path) -> list[Finding]:
    root = Path(run_git(root, "rev-parse", "--show-toplevel").decode().strip())
    paths_by_object, metadata = history_objects(root)
    path_objects: dict[str, set[str]] = {}
    for object_id, paths in paths_by_object.items():
        object_type, _ = metadata.get(object_id, ("", 0))
        if object_type != "blob":
            continue
        for path in paths:
            path_objects.setdefault(path, set()).add(object_id)

    findings: set[Finding] = set()
    for path in sorted(historical_paths(root) | set(path_objects)):
        object_id = min(path_objects.get(path, {"<history-path>"}))
        reason = forbidden_path_reason(path)
        if reason is not None:
            findings.add(Finding(True, "forbidden-path", object_id, path, reason))
        elif path.startswith(".bootstrap/"):
            findings.add(
                Finding(
                    False,
                    "transport-history",
                    object_id,
                    path,
                    "temporary bootstrap transport payload; remove stale refs before public launch",
                )
            )

    for object_id, (object_type, _size) in metadata.items():
        if object_type != "blob":
            continue
        content = run_git(root, "cat-file", "blob", object_id)
        paths = sorted(paths_by_object.get(object_id) or {"<unpathed blob>"})
        for path in paths:
            matched = credential_matches(content, path)
            if not matched:
                continue
            findings.add(
                Finding(
                    True,
                    "credential-pattern",
                    object_id,
                    path,
                    ", ".join(matched),
                )
            )

    return sorted(findings, key=lambda finding: (not finding.blocking, finding.path, finding.kind))


def main() -> int:
    parser = argparse.ArgumentParser(
        description="scan all reachable Git history for public-release blockers"
    )
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    args = parser.parse_args()

    try:
        findings = audit(args.repo_root)
    except (OSError, RuntimeError) as error:
        print(f"public-release audit error: {error}", file=sys.stderr)
        return 2

    blocking = [finding for finding in findings if finding.blocking]
    review = [finding for finding in findings if not finding.blocking]

    for finding in blocking:
        print(
            f"BLOCK {finding.kind}: {finding.path} "
            f"[{finding.object_id[:12]}] — {finding.detail}"
        )
    for finding in review:
        print(
            f"REVIEW {finding.kind}: {finding.path} "
            f"[{finding.object_id[:12]}] — {finding.detail}"
        )

    if blocking:
        print(f"public-release audit: FAIL ({len(blocking)} blocking finding(s))")
        return 1
    print(
        "public-release audit: PASS "
        f"({len(review)} non-blocking historical transport finding(s))"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
