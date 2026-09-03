#!/usr/bin/env python3
"""Run the complete local R2C world-state review-to-Atlas admission chain.

This is operator composition only. It regenerates the exact current source-review dossier from the
pinned local Minecraft 26.2 source archive, applies the committed source-free human review and
semantic decisions, materializes the canonical source-free VAR/SEM staging bundle, and runs the
independent Vanilla Atlas gate.

Official source excerpts exist only inside temporary local storage. The published tar.gz contains
only source-free review/admission evidence and is safe to upload for inspection. This tool never
promotes evidence into the repository and never authorizes runtime behavior by itself.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tarfile
import tempfile
from pathlib import Path
from typing import Sequence

try:
    from . import r2c_world_state_admission_apply as admission_apply
    from . import r2c_world_state_admission_materialize as materialize
    from . import r2c_world_state_admission_prepare as prepare
    from . import r2c_world_state_parent_review_complete as parent_complete
    from . import r2c_world_state_source_gate as bound_gate
    from . import r2c_world_state_source_review_bundle as source_bundle
except ImportError:  # Direct ``python3 tools/...`` execution.
    import r2c_world_state_admission_apply as admission_apply  # type: ignore[no-redef]
    import r2c_world_state_admission_materialize as materialize  # type: ignore[no-redef]
    import r2c_world_state_admission_prepare as prepare  # type: ignore[no-redef]
    import r2c_world_state_parent_review_complete as parent_complete  # type: ignore[no-redef]
    import r2c_world_state_source_gate as bound_gate  # type: ignore[no-redef]
    import r2c_world_state_source_review_bundle as source_bundle  # type: ignore[no-redef]

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DB = source_bundle.DEFAULT_DB
DEFAULT_SOURCE = source_bundle.DEFAULT_SOURCE
DEFAULT_LOCK = source_bundle.DEFAULT_LOCK
DEFAULT_PLAN = source_bundle.discovery.DEFAULT_PLAN
DEFAULT_PARENT_DECISIONS = (
    REPO_ROOT / "vanilla/reviews/network/r2c-world-state-parent-review-decisions.json"
)
DEFAULT_SEMANTIC_DECISIONS = (
    REPO_ROOT / "vanilla/reviews/network/r2c-world-state-semantic-admission-decisions.json"
)
RUN_MANIFEST = "admission-run-manifest.json"
GATE_REPORT = "gate-report.json"
PARENT_REVIEW_RESULT = "review/parent-review-result.json"
PREPARED_WORKSHEET = "review/prepared-admission-worksheet.json"
COMPLETED_WORKSHEET = "review/completed-admission-worksheet.json"
ARTIFACT_KIND = "r2c-world-state-local-admission-artifact"
ARTIFACT_COMMIT_POLICY = "SOURCE_FREE_UPLOAD_EVIDENCE_NOT_REPOSITORY_PROMOTION"


class LocalAdmissionError(RuntimeError):
    """Fail-closed local R2C admission-composition error."""


def _pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _fresh_external_output(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise LocalAdmissionError(f"output archive must not already exist: {path}")
    resolved = path.expanduser().resolve(strict=False)
    repository = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repository)
    except ValueError:
        pass
    else:
        raise LocalAdmissionError("source-admission upload artifact must live outside the repository")
    parent = resolved.parent
    if parent.exists() and parent.is_symlink():
        raise LocalAdmissionError(f"output parent must not be a symlink: {parent}")
    parent.mkdir(parents=True, exist_ok=True)
    return resolved


def _copy_source_free(source: Path, destination: Path) -> None:
    if source.is_symlink() or not source.is_file():
        raise LocalAdmissionError(f"source-free handoff must be a real file: {source}")
    raw = source.read_bytes()
    try:
        text = raw.decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise LocalAdmissionError(f"source-free handoff is not UTF-8: {source}") from error
    if "source_excerpt" in text:
        raise LocalAdmissionError(f"official-source excerpt field leaked into upload evidence: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(raw)


def _copy_staging(staging: Path, export_root: Path) -> int:
    if staging.is_symlink() or not staging.is_dir():
        raise LocalAdmissionError(f"materialized staging root is invalid: {staging}")
    files = 0
    for path in sorted(staging.rglob("*")):
        if path.is_symlink():
            raise LocalAdmissionError(f"materialized staging contains a symlink: {path}")
        if path.is_dir():
            continue
        if not path.is_file():
            raise LocalAdmissionError(f"materialized staging contains a non-file entry: {path}")
        relative = path.relative_to(staging)
        _copy_source_free(path, export_root / "staging" / relative)
        files += 1
    if files == 0:
        raise LocalAdmissionError("materialized staging contains no files")
    return files


def _file_manifest(root: Path) -> list[dict[str, object]]:
    result: list[dict[str, object]] = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise LocalAdmissionError(f"upload evidence contains a symlink: {path}")
        if not path.is_file() or path.name == RUN_MANIFEST:
            continue
        result.append(
            {
                "path": path.relative_to(root).as_posix(),
                "size": path.stat().st_size,
                "sha256": _sha256_file(path),
            }
        )
    return result


def _write_archive(export_root: Path, output: Path) -> int:
    with tempfile.TemporaryDirectory(prefix=f".{output.name}.staging-", dir=output.parent) as temporary:
        staged = Path(temporary) / output.name
        with tarfile.open(staged, mode="w:gz") as archive:
            for path in sorted(export_root.rglob("*")):
                if path.is_file():
                    archive.add(path, arcname=path.relative_to(export_root).as_posix(), recursive=False)
        try:
            with tarfile.open(staged, mode="r:gz") as archive:
                members = [member for member in archive.getmembers() if member.isfile()]
                if not members:
                    raise LocalAdmissionError("staged source-admission archive is empty")
                names = {member.name for member in members}
                if RUN_MANIFEST not in names or GATE_REPORT not in names:
                    raise LocalAdmissionError("staged source-admission archive is missing its evidence seal")
        except tarfile.TarError as error:
            raise LocalAdmissionError(f"cannot reopen staged source-admission archive: {error}") from error
        staged.replace(output)
        return len(members)


def run(
    *,
    output: Path,
    db: Path,
    source: Path,
    lock: Path,
    plan: Path = DEFAULT_PLAN,
    parent_decisions: Path = DEFAULT_PARENT_DECISIONS,
    semantic_decisions: Path = DEFAULT_SEMANTIC_DECISIONS,
) -> dict[str, object]:
    """Execute the exact current review/materialization/gate chain and publish source-free evidence."""
    output = _fresh_external_output(output)
    with tempfile.TemporaryDirectory(prefix="helve-r2c-local-admission-") as temporary:
        root = Path(temporary)
        source_review_archive = root / "source-review.tar.gz"
        parent_root = root / "parent-review"
        prepared = root / "prepared-admission-worksheet.json"
        completed = root / "completed-admission-worksheet.json"
        staging = root / "staging"
        export_root = root / "upload"
        export_root.mkdir()

        source_summary = source_bundle.build_bundle(
            output=source_review_archive,
            db=db,
            source=source,
            lock=lock,
            plan=plan,
        )
        parent_summary = parent_complete.complete_bundle(
            source_review_archive,
            parent_decisions,
            parent_root,
        )
        parent_result = parent_root / parent_complete.REVIEW_RESULT
        prepare_summary = prepare.prepare(parent_result, prepared)
        apply_summary = admission_apply.apply(
            worksheet=prepared,
            decisions=semantic_decisions,
            output=completed,
        )
        materialize_summary = materialize.materialize(parent_result, completed, staging)
        report = bound_gate.evaluate_bound(db_path=db, staging_dir=staging)
        report_raw = _pretty_bytes(report)

        _copy_source_free(parent_result, export_root / PARENT_REVIEW_RESULT)
        _copy_source_free(prepared, export_root / PREPARED_WORKSHEET)
        _copy_source_free(completed, export_root / COMPLETED_WORKSHEET)
        staging_files = _copy_staging(staging, export_root)
        (export_root / GATE_REPORT).write_bytes(report_raw)

        manifest: dict[str, object] = {
            "schema": 1,
            "kind": ARTIFACT_KIND,
            "commit_policy": ARTIFACT_COMMIT_POLICY,
            "contains_official_source_text": False,
            "repository_promotion_performed": False,
            "source_gate_admitted": report.get("admitted") is True,
            "source_archive_sha256": source_bundle.packer.EXPECTED_SOURCE_SHA256,
            "source_review_bundle_manifest_sha256": source_summary["bundle_manifest_sha256"],
            "parent_review_decisions_sha256": _sha256_file(parent_decisions),
            "semantic_decisions_sha256": _sha256_file(semantic_decisions),
            "parent_review_result_sha256": parent_summary["parent_review_result_sha256"],
            "prepared_worksheet_sha256": prepare_summary["sha256"],
            "completed_worksheet_sha256": apply_summary["sha256"],
            "materialization_manifest_sha256": materialize_summary["manifest_sha256"],
            "gate_report_sha256": hashlib.sha256(report_raw).hexdigest(),
            "var_records": materialize_summary["var_records"],
            "semantic_rules": materialize_summary["semantic_rules"],
            "staging_files": staging_files,
            "files": _file_manifest(export_root),
        }
        (export_root / RUN_MANIFEST).write_bytes(_pretty_bytes(manifest))
        archive_files = _write_archive(export_root, output)

    return {
        "output": str(output),
        "sha256": _sha256_file(output),
        "archive_regular_files": archive_files,
        "source_gate_admitted": manifest["source_gate_admitted"],
        "var_records": manifest["var_records"],
        "semantic_rules": manifest["semantic_rules"],
        "materialization_manifest_sha256": manifest["materialization_manifest_sha256"],
        "gate_report_sha256": manifest["gate_report_sha256"],
        "contains_official_source_text": False,
        "repository_promotion_performed": False,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    parser.add_argument("--parent-decisions", type=Path, default=DEFAULT_PARENT_DECISIONS)
    parser.add_argument("--semantic-decisions", type=Path, default=DEFAULT_SEMANTIC_DECISIONS)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        summary = run(
            output=args.output,
            db=args.db,
            source=args.source,
            lock=args.lock,
            plan=args.plan,
            parent_decisions=args.parent_decisions,
            semantic_decisions=args.semantic_decisions,
        )
    except (
        LocalAdmissionError,
        OSError,
        source_bundle.BundleError,
        source_bundle.discovery.DiscoveryError,
        source_bundle.packer.ReviewPackError,
        parent_complete.CompleteError,
        prepare.PrepareError,
        admission_apply.ApplyError,
        materialize.MaterializeError,
        bound_gate.BoundGateError,
        bound_gate.promote.PromoteError,
        bound_gate.source_gate.GateError,
        json.JSONDecodeError,
    ) as error:
        print(f"R2C local world-state admission failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0 if summary["source_gate_admitted"] is True else 2


if __name__ == "__main__":
    raise SystemExit(main())
