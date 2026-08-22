#!/usr/bin/env python3
"""Write a self-describing manifest for representative-set qualification artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

SCHEMA = 2
KIND = "section-representative-set-workflow-artifact"


class ArtifactManifestError(ValueError):
    """Raised when completed qualification evidence is internally inconsistent."""


def _canonical_digest(value: object) -> str:
    payload = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _command(*command: str) -> str:
    completed = subprocess.run(
        command,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    return completed.stdout.strip()


def _load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ArtifactManifestError(f"{path} must contain a JSON object")
    return value


def collect_provenance() -> dict[str, object]:
    commit = os.environ.get("GITHUB_SHA") or _command("git", "rev-parse", "HEAD")
    return {
        "repository_commit_sha": commit,
        "github_run_id": os.environ.get("GITHUB_RUN_ID", "local"),
        "github_run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", "local"),
        "python_version": sys.version.split()[0],
        "rustc_version": _command("rustc", "--version", "--verbose"),
        "java_version": _command("java", "-version"),
    }


def build_manifest(
    root: Path,
    *,
    provenance: dict[str, object],
) -> dict[str, object]:
    admission_path = root / "population-admission.json"
    set_path = root / "corpus-set.json"
    admission: dict[str, Any] | None = None
    if admission_path.is_file():
        admission = _load_json(admission_path)

    qualification_complete = bool(
        admission is not None
        and admission.get("kind") == "section-representative-set-admission"
        and admission.get("decision_eligible") is True
        and admission.get("benchmark_handoff_eligible") is True
        and set_path.is_file()
    )

    identities: dict[str, object] = {}
    if admission is not None:
        for key in (
            "population_sha256",
            "set_evidence_sha256",
            "set_file_sha256",
            "admission_sha256",
        ):
            value = admission.get(key)
            if not isinstance(value, str) or len(value) != 64:
                raise ArtifactManifestError(
                    f"population admission has invalid {key}: {value!r}"
                )
            identities[key] = value

    if qualification_complete:
        if _sha256(set_path) != identities["set_file_sha256"]:
            raise ArtifactManifestError(
                "corpus-set.json changed after population admission"
            )
        if _sha256(admission_path) == identities["admission_sha256"]:
            raise ArtifactManifestError(
                "admission_sha256 is a canonical record digest, not a raw file digest"
            )

    entries = []
    output_path = root / "artifact-manifest.json"
    for path in sorted(root.rglob("*")):
        if path.is_file() and path != output_path:
            entries.append(
                {
                    "path": path.relative_to(root).as_posix(),
                    "size": path.stat().st_size,
                    "sha256": _sha256(path),
                }
            )

    result: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "qualification_complete": qualification_complete,
        "decision_eligible": bool(
            qualification_complete and admission and admission.get("decision_eligible")
        ),
        "benchmark_handoff_eligible": bool(
            qualification_complete
            and admission
            and admission.get("benchmark_handoff_eligible")
        ),
        "provenance": provenance,
        "identities": identities,
        "files": entries,
    }
    result["manifest_sha256"] = _canonical_digest(result)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    args = parser.parse_args()
    try:
        args.root.mkdir(parents=True, exist_ok=True)
        result = build_manifest(args.root, provenance=collect_provenance())
        output = args.root / "artifact-manifest.json"
        output.write_text(
            json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (
        OSError,
        json.JSONDecodeError,
        subprocess.SubprocessError,
        ArtifactManifestError,
    ) as error:
        print(f"representative artifact manifest error: {error}", file=sys.stderr)
        return 1

    print(
        "representative artifact manifest: "
        f"complete={result['qualification_complete']} files={len(result['files'])} "
        f"manifest={result['manifest_sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
