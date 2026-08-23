#!/usr/bin/env python3
"""Compose Crucible's independent R0 status evidence gates into one exact admission session."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any

TOOLS = Path(__file__).resolve().parent
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from protocol_capture_admission import (  # noqa: E402
    EvidenceConvergenceError,
    crosscheck_capture,
)
from protocol_codegen import CodegenError, generate as generate_protocol  # noqa: E402
from protocol_contract import ContractError  # noqa: E402
from vanilla_source_gate import GateError, evaluate as evaluate_source_gate  # noqa: E402

SCHEMA = 1
KIND = "r0-status-admission-v1"
HEX_64 = re.compile(r"^[0-9a-f]{64}$")


class R0AdmissionError(ValueError):
    """Raised when independent R0 evidence cannot be bound to one exact target instance."""


def _object(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise R0AdmissionError(f"{label} must be an object")
    return value


def _string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise R0AdmissionError(f"{label} must be a non-empty string")
    return value


def _integer(value: object, label: str) -> int:
    if type(value) is not int:
        raise R0AdmissionError(f"{label} must be an integer")
    return value


def _sha256(value: object, label: str) -> str:
    digest = _string(value, label)
    if HEX_64.fullmatch(digest) is None:
        raise R0AdmissionError(f"{label} must be lowercase SHA-256")
    return digest


def _read_json(path: Path, label: str) -> dict[str, Any]:
    if path.is_symlink() or not path.is_file():
        raise R0AdmissionError(f"{label} must be a real non-symlink file: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise R0AdmissionError(f"could not read {label} {path}: {error}") from error
    return _object(value, label)


def _atlas_protocol(value: object) -> int:
    if type(value) is int:
        result = value
    elif isinstance(value, str) and value.isdecimal():
        result = int(value)
    else:
        raise R0AdmissionError("source gate protocol_version is not a canonical integer")
    if result < 0:
        raise R0AdmissionError("source gate protocol_version must be non-negative")
    return result


def _source_identity(source_report: dict[str, Any]) -> dict[str, object]:
    source = _object(source_report.get("source"), "source admission report source")
    return {
        "minecraft": _string(
            source.get("minecraft_version"), "source admission minecraft_version"
        ),
        "protocol": _atlas_protocol(source.get("protocol_version")),
        "source_archive_sha256": _sha256(
            source.get("archive_sha256"), "source admission archive_sha256"
        ),
        "fingerprint_algorithm": _string(
            source.get("fingerprint_algorithm"),
            "source admission fingerprint_algorithm",
        ),
    }


def _contract_identity(contract: dict[str, Any]) -> dict[str, object]:
    target = _object(contract.get("target"), "protocol contract target")
    return {
        "minecraft": _string(target.get("minecraft"), "protocol contract target.minecraft"),
        "protocol": _integer(target.get("protocol"), "protocol contract target.protocol"),
        "source_archive_sha256": _sha256(
            target.get("source_archive_sha256"),
            "protocol contract target.source_archive_sha256",
        ),
        "fingerprint_algorithm": _string(
            target.get("fingerprint_algorithm"),
            "protocol contract target.fingerprint_algorithm",
        ),
    }


def _required_methods(source_report: dict[str, Any]) -> tuple[dict[str, object], ...]:
    methods = source_report.get("required_methods")
    if not isinstance(methods, list) or not methods:
        raise R0AdmissionError("source admission report has no required methods")
    admitted: list[dict[str, object]] = []
    seen: set[str] = set()
    for index, value in enumerate(methods):
        method = _object(value, f"source admission required_methods[{index}]")
        var_id = _string(method.get("var_id"), f"required_methods[{index}].var_id")
        if var_id in seen:
            raise R0AdmissionError(f"source admission report duplicates VAR id {var_id}")
        seen.add(var_id)
        semantic_rules = method.get("semantic_rules")
        if (
            not isinstance(semantic_rules, list)
            or not semantic_rules
            or any(not isinstance(rule, str) or not rule for rule in semantic_rules)
        ):
            raise R0AdmissionError(
                f"required_methods[{index}].semantic_rules must be a non-empty array of non-empty strings"
            )
        admitted.append(
            {
                "var_id": var_id,
                "record_sha256": _sha256(
                    method.get("record_sha256"),
                    f"required_methods[{index}].record_sha256",
                ),
                "source": _string(
                    method.get("source"), f"required_methods[{index}].source"
                ),
                "normalized_sha256": _sha256(
                    method.get("normalized_sha256"),
                    f"required_methods[{index}].normalized_sha256",
                ),
                "body_sha256": _sha256(
                    method.get("body_sha256"),
                    f"required_methods[{index}].body_sha256",
                ),
                "semantic_rules": sorted(semantic_rules),
            }
        )
    admitted.sort(key=lambda item: str(item["var_id"]))
    return tuple(admitted)


def _contract_source_records(contract: dict[str, Any]) -> tuple[str, ...]:
    packets = contract.get("packets")
    if not isinstance(packets, list) or not packets:
        raise R0AdmissionError("protocol contract has no packets")
    records: set[str] = set()
    for index, value in enumerate(packets):
        packet = _object(value, f"protocol contract packets[{index}]")
        source_records = packet.get("source_records")
        if not isinstance(source_records, list) or not source_records:
            raise R0AdmissionError(
                f"protocol contract packets[{index}].source_records must be non-empty"
            )
        for record_index, record in enumerate(source_records):
            records.add(
                _string(
                    record,
                    f"protocol contract packets[{index}].source_records[{record_index}]",
                )
            )
    return tuple(sorted(records))


def _session_digest(report: dict[str, object]) -> str:
    canonical = json.dumps(
        report, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("ascii")
    return hashlib.sha256(canonical).hexdigest()


def admit_r0_session(
    *,
    db_path: Path,
    source_gate_path: Path,
    contract_path: Path,
    capture_path: Path,
    generated_rust_path: Path,
    lock_path: Path,
    records_root: Path,
) -> dict[str, object]:
    """Run every independent R0 evidence gate and bind their exact identities into one report."""
    if db_path.is_symlink() or not db_path.is_file():
        raise R0AdmissionError(
            f"Atlas database must be a real non-symlink file: {db_path}"
        )

    source_report = evaluate_source_gate(
        db_path=db_path,
        gate_path=source_gate_path,
        records_dir=records_root,
    )
    if source_report.get("admitted") is not True:
        failures = source_report.get("failures")
        detail = (
            "; ".join(str(item) for item in failures)
            if isinstance(failures, list)
            else "unknown failure"
        )
        raise R0AdmissionError(f"source admission gate rejected R0 evidence: {detail}")

    gate_id = _string(source_report.get("gate_id"), "source admission gate_id")
    gate_sha256 = _sha256(source_report.get("gate_sha256"), "source admission gate_sha256")
    source_identity = _source_identity(source_report)
    required_methods = _required_methods(source_report)

    contract = _read_json(contract_path, "protocol contract")
    contract_identity = _contract_identity(contract)
    if contract_identity != source_identity:
        raise R0AdmissionError(
            "source admission identity does not match finite protocol contract target"
        )

    gated_var_ids = {str(item["var_id"]) for item in required_methods}
    contract_records = _contract_source_records(contract)
    ungated = sorted(set(contract_records) - gated_var_ids)
    if ungated:
        raise R0AdmissionError(
            "protocol contract cites VAR records not admitted by the current source gate: "
            + ", ".join(ungated)
        )

    convergence = crosscheck_capture(
        contract_path,
        capture_path,
        lock_path=lock_path,
        records_root=records_root,
    )
    if convergence.get("minecraft") != source_identity["minecraft"] or convergence.get(
        "protocol"
    ) != source_identity["protocol"]:
        raise R0AdmissionError(
            "capture convergence summary does not match source admission target"
        )

    rendered = generate_protocol(
        contract_path,
        lock_path=lock_path,
        records_root=records_root,
        output_path=generated_rust_path,
        check=True,
    )
    generated_sha256 = hashlib.sha256(rendered.encode("utf-8")).hexdigest()

    report: dict[str, object] = {
        "schema": SCHEMA,
        "kind": KIND,
        "target": source_identity,
        "source_gate": {
            "id": gate_id,
            "sha256": gate_sha256,
            "required_methods": list(required_methods),
        },
        "contract": {
            "id": _string(convergence.get("contract_id"), "convergence contract_id"),
            "source_records": list(contract_records),
        },
        "capture": {
            "sha256": _sha256(
                convergence.get("capture_sha256"), "convergence capture_sha256"
            ),
            "client_to_server_frames": _integer(
                convergence.get("client_to_server_frames"),
                "convergence client_to_server_frames",
            ),
            "server_to_client_frames": _integer(
                convergence.get("server_to_client_frames"),
                "convergence server_to_client_frames",
            ),
            "frames_matched": _integer(
                convergence.get("frames_matched"), "convergence frames_matched"
            ),
        },
        "generated_rust": {"sha256": generated_sha256},
    }
    report["session_sha256"] = _session_digest(report)
    return report


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", type=Path, required=True)
    parser.add_argument("--source-gate", type=Path, required=True)
    parser.add_argument("--contract", type=Path, required=True)
    parser.add_argument("--capture", type=Path, required=True)
    parser.add_argument("--generated-rust", type=Path, required=True)
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--records-root", type=Path, default=Path("vanilla/records"))
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    try:
        report = admit_r0_session(
            db_path=args.db,
            source_gate_path=args.source_gate,
            contract_path=args.contract,
            capture_path=args.capture,
            generated_rust_path=args.generated_rust,
            lock_path=args.lock,
            records_root=args.records_root,
        )
    except (
        R0AdmissionError,
        GateError,
        ContractError,
        EvidenceConvergenceError,
        CodegenError,
        OSError,
        KeyError,
        TypeError,
    ) as error:
        print(f"R0 admission error: {error}", file=sys.stderr)
        return 1

    encoded = json.dumps(report, sort_keys=True, separators=(",", ":")) + "\n"
    if args.output is None:
        print(encoded, end="")
    else:
        if args.output.exists() and args.output.is_symlink():
            print(
                f"R0 admission error: refusing to replace symlink output: {args.output}",
                file=sys.stderr,
            )
            return 1
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
