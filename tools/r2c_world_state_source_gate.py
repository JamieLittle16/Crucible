#!/usr/bin/env python3
"""Run the independent Vanilla Atlas gate while binding the complete R2C staging manifest.

The generic source gate proves the required VAR methods against the pinned Atlas. This R2C wrapper
adds one required provenance edge: the emitted report also hashes the exact #220 materialization
manifest, which in turn content-addresses the staged VAR records, semantic Markdown, and gate.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Sequence

try:
    from . import r2c_world_state_admission_materialize as materialize
    from . import r2c_world_state_admission_promote as promote
    from . import vanilla_atlas as atlas
    from . import vanilla_source_gate as source_gate
except ImportError:  # Direct ``python3 tools/...`` execution.
    import r2c_world_state_admission_materialize as materialize  # type: ignore[no-redef]
    import r2c_world_state_admission_promote as promote  # type: ignore[no-redef]
    import vanilla_atlas as atlas  # type: ignore[no-redef]
    import vanilla_source_gate as source_gate  # type: ignore[no-redef]


class BoundGateError(RuntimeError):
    """Fail-closed R2C bound source-gate error."""


def evaluate_bound(*, db_path: Path, staging_dir: Path) -> dict[str, object]:
    staged, manifest_raw, _manifest = promote._validate_staging(staging_dir)
    if "gate.json" not in staged:
        raise BoundGateError("validated staging bundle omitted gate.json")
    report = source_gate.evaluate(
        db_path=db_path,
        gate_path=staging_dir / "gate.json",
        records_dir=staging_dir / "records",
    )
    if report.get("gate_id") != materialize.GATE_ID:
        raise BoundGateError("generic source gate returned the wrong gate identity")
    if report.get("gate_sha256") != promote._sha256(staged["gate.json"]):
        raise BoundGateError("generic source gate did not bind the exact staged gate")
    result = dict(report)
    result.update(
        {
            "materialization_id": materialize.ID,
            "materialization_manifest_sha256": promote._sha256(manifest_raw),
            "source_free_bundle_bound": True,
        }
    )
    return result


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", type=Path, default=atlas.default_db())
    parser.add_argument("--staging-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    if args.output.exists() or args.output.is_symlink():
        print(f"R2C bound source gate refuses to overwrite output: {args.output}", file=sys.stderr)
        return 1
    try:
        report = evaluate_bound(db_path=args.db, staging_dir=args.staging_dir)
    except (BoundGateError, promote.PromoteError, source_gate.GateError, OSError, json.JSONDecodeError) as error:
        print(f"R2C bound source admission error: {error}", file=sys.stderr)
        return 1
    encoded = json.dumps(report, indent=2, sort_keys=True) + "\n"
    try:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    except OSError as error:
        print(f"R2C bound source admission output failed: {error}", file=sys.stderr)
        return 1
    return 0 if report.get("admitted") is True else 2


if __name__ == "__main__":
    raise SystemExit(main())
