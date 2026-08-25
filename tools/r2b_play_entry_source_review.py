#!/usr/bin/env python3
"""Prepare the final bounded source-rich review for R2B fresh-player Play entry.

This is the first review pack intended to close directly into the canonical R2B Play-entry gate. It
unifies the historical 27-body first pass and 35-body follow-up with only the remaining delegate
closure required by the hardened source-free probe. Before selector preflight it deterministically
indexes initialized instance fields so anonymous source implementations are reviewable through the
ordinary Atlas fingerprint machinery.

The source-rich dossier is ephemeral and must never be committed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sqlite3
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

try:
    from . import r1b_configuration_source_probe as source_probe
    from . import r1b_play_entry_followup_source_review as followup
    from . import r1b_play_entry_source_review as first_pass
    from . import vanilla_atlas
    from . import vanilla_instance_field_index as field_index
except ImportError:
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]
    import r1b_play_entry_followup_source_review as followup  # type: ignore[no-redef]
    import r1b_play_entry_source_review as first_pass  # type: ignore[no-redef]
    import vanilla_atlas  # type: ignore[no-redef]
    import vanilla_instance_field_index as field_index  # type: ignore[no-redef]

SCHEMA = 1
REVIEW_ID = "REVIEW-NET-R2B-PLAY-ENTRY-FINAL-26_2-001"
PREPARED_KIND = "r2b-play-entry-final-source-review"
WORKSHEET_KIND = "r2b-play-entry-final-source-review-worksheet"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
REPO_ROOT = Path(__file__).resolve().parents[1]
ROUTE_DISPOSITIONS = (
    "MANDATORY",
    "CONDITIONAL",
    "DEFAULT_EMPTY",
    "INTERNAL_ONLY",
    "OUTBOUND_IRRELEVANT",
    "DELEGATED_REVIEW_REQUIRED",
)
INVENTORY_MENU_TYPE = "net.minecraft.world.inventory.InventoryMenu"


@dataclass(frozen=True)
class Candidate:
    var_id: str
    type_name: str
    method_name: str
    param_count: int
    review_focus: tuple[str, ...]
    exact_signature: str | None = None


class R2BPlayEntryReviewError(RuntimeError):
    """Fail-closed final Play-entry source review error."""


def _copy_candidate(value: object) -> Candidate:
    return Candidate(
        var_id=str(getattr(value, "var_id")),
        type_name=str(getattr(value, "type_name")),
        method_name=str(getattr(value, "method_name")),
        param_count=int(getattr(value, "param_count")),
        review_focus=tuple(str(item) for item in getattr(value, "review_focus")),
        exact_signature=getattr(value, "exact_signature", None),
    )


def selector_key(candidate: Candidate) -> tuple[object, ...]:
    if candidate.exact_signature is not None:
        return ("exact", candidate.type_name, candidate.exact_signature)
    return ("arity", candidate.type_name, candidate.method_name, candidate.param_count)


def _fixed_closure_candidates() -> tuple[Candidate, ...]:
    return (
        Candidate(
            var_id="DISC-NET-R2B-PLAY-INVENTORY-SET-SYNCHRONIZER-001",
            type_name="net.minecraft.world.inventory.AbstractContainerMenu",
            method_name="setSynchronizer",
            param_count=1,
            exact_signature="setSynchronizer(final ContainerSynchronizer synchronizer)",
            review_focus=(
                "Prove the exact synchronizer-install transition reached from ServerPlayer.initMenu and whether installation immediately triggers the full initial remote snapshot.",
            ),
        ),
        Candidate(
            var_id="DISC-NET-R2B-PLAY-INVENTORY-SEND-ALL-001",
            type_name="net.minecraft.world.inventory.AbstractContainerMenu",
            method_name="sendAllDataToRemote",
            param_count=0,
            exact_signature="sendAllDataToRemote()",
            review_focus=(
                "Bind the initial slot/carried/data snapshot and the exact sendInitialData callback reached after synchronizer installation.",
            ),
        ),
        Candidate(
            var_id="DISC-NET-R2B-PLAY-INVENTORY-SYNCHRONIZER-FIELD-001",
            type_name="net.minecraft.server.level.ServerPlayer",
            method_name="<fieldinit:containerSynchronizer>",
            param_count=0,
            exact_signature="<fieldinit:containerSynchronizer>()",
            review_focus=(
                "Inspect the real anonymous ContainerSynchronizer implementation bound by initMenu, including the packet(s) emitted by sendInitialData and any selected-route data-slot branch.",
            ),
        ),
        Candidate(
            var_id="DISC-NET-R2B-PLAY-INVENTORY-SYNCHRONIZER-CONTRACT-001",
            type_name="net.minecraft.world.inventory.ContainerSynchronizer",
            method_name="sendInitialData",
            param_count=4,
            exact_signature=(
                "sendInitialData(AbstractContainerMenu container , List < ItemStack > slotItems , "
                "ItemStack carried , int [ ] dataSlots)"
            ),
            review_focus=(
                "Bind the callback contract used by AbstractContainerMenu so the anonymous field implementation is linked to the exact initial-data dispatch surface rather than inferred by name alone.",
            ),
        ),
    )


def _inventory_menu_candidates(conn: sqlite3.Connection) -> tuple[Candidate, ...]:
    rows = conn.execute(
        """SELECT m.signature,m.param_count
           FROM methods m JOIN types t ON t.id=m.type_id
           WHERE t.qualified_name=? AND m.is_constructor=1
           ORDER BY m.signature""",
        (INVENTORY_MENU_TYPE,),
    ).fetchall()
    if not rows:
        raise R2BPlayEntryReviewError("InventoryMenu has no Atlas constructor rows")
    result: list[Candidate] = []
    for index, row in enumerate(rows, start=1):
        signature = str(row[0])
        result.append(
            Candidate(
                var_id=f"DISC-NET-R2B-PLAY-INVENTORY-MENU-CONSTRUCTOR-{index:03d}",
                type_name=INVENTORY_MENU_TYPE,
                method_name="InventoryMenu",
                param_count=int(row[1]),
                exact_signature=signature,
                review_focus=(
                    "Determine which InventoryMenu constructor represents the selected fresh/default player menu and prove whether it installs any DataSlot values that make initial container-data packets observable.",
                ),
            )
        )
    return tuple(result)


def candidates(conn: sqlite3.Connection) -> tuple[Candidate, ...]:
    combined = (
        tuple(_copy_candidate(item) for item in first_pass.CANDIDATES)
        + tuple(_copy_candidate(item) for item in followup.CANDIDATES)
        + _fixed_closure_candidates()
        + _inventory_menu_candidates(conn)
    )
    ids = [candidate.var_id for candidate in combined]
    if len(ids) != len(set(ids)):
        raise R2BPlayEntryReviewError("final review contains duplicate candidate ids")

    by_selector: dict[tuple[object, ...], Candidate] = {}
    duplicates: list[str] = []
    for candidate in combined:
        key = selector_key(candidate)
        previous = by_selector.get(key)
        if previous is not None:
            duplicates.append(f"{previous.var_id} == {candidate.var_id}: {key}")
        else:
            by_selector[key] = candidate
    if duplicates:
        raise R2BPlayEntryReviewError(
            "final review contains duplicate effective selectors:\n  - " + "\n  - ".join(duplicates)
        )
    return combined


def pretty_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _external_fresh_dir(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise R2BPlayEntryReviewError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repo = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repo)
    except ValueError:
        return resolved
    raise R2BPlayEntryReviewError(
        "R2B Play-entry review contains official source text and must live outside the repository"
    )


def _resolve(conn: sqlite3.Connection, candidate: Candidate) -> sqlite3.Row:
    select = """SELECT m.id,t.qualified_name,m.name,m.signature,m.param_count,
                       m.start_line,m.end_line,f.path
                FROM methods m JOIN types t ON t.id=m.type_id
                JOIN source_files f ON f.id=t.file_id"""
    if candidate.exact_signature is not None:
        rows = conn.execute(
            select + " WHERE t.qualified_name=? AND m.signature=? ORDER BY m.start_line",
            (candidate.type_name, candidate.exact_signature),
        ).fetchall()
        if len(rows) != 1:
            identities = [f"{row['qualified_name']}#{row['signature']}" for row in rows]
            raise R2BPlayEntryReviewError(
                f"{candidate.var_id}: exact selector {candidate.type_name}#"
                f"{candidate.exact_signature} resolved {len(rows)} methods: {identities}"
            )
        row = rows[0]
        if row["name"] != candidate.method_name or int(row["param_count"]) != candidate.param_count:
            raise R2BPlayEntryReviewError(
                f"{candidate.var_id}: exact signature disagrees with declared method/arity"
            )
        return row
    rows = conn.execute(
        select + " WHERE t.qualified_name=? AND m.name=? AND m.param_count=? ORDER BY m.start_line",
        (candidate.type_name, candidate.method_name, candidate.param_count),
    ).fetchall()
    if len(rows) != 1:
        identities = [f"{row['qualified_name']}#{row['signature']}" for row in rows]
        raise R2BPlayEntryReviewError(
            f"{candidate.var_id}: {candidate.type_name}#{candidate.method_name}/"
            f"{candidate.param_count} resolved {len(rows)} methods: {identities}"
        )
    return rows[0]


def _resolve_all(
    conn: sqlite3.Connection, selected: Sequence[Candidate]
) -> list[tuple[Candidate, sqlite3.Row]]:
    resolved: list[tuple[Candidate, sqlite3.Row]] = []
    failures: list[str] = []
    for candidate in selected:
        try:
            resolved.append((candidate, _resolve(conn, candidate)))
        except R2BPlayEntryReviewError as error:
            failures.append(str(error))
    if failures:
        raise R2BPlayEntryReviewError(
            "selector preflight failed:\n" + "\n".join(f"  - {item}" for item in failures)
        )
    return resolved


def _source_excerpt(archive: zipfile.ZipFile, row: sqlite3.Row) -> str:
    path = str(row["path"])
    try:
        text = archive.read(path).decode("utf-8", errors="strict")
    except KeyError as error:
        raise R2BPlayEntryReviewError(f"source member missing: {path}") from error
    lines = text.splitlines()
    start, end = int(row["start_line"]), int(row["end_line"])
    if not (1 <= start <= end <= len(lines)):
        raise R2BPlayEntryReviewError(f"invalid Atlas line range for {path}: {start}-{end}")
    return "\n".join(lines[start - 1 : end]) + "\n"


def prepare(output_dir: Path, db: Path, source: Path, lock: Path) -> dict[str, object]:
    output = _external_fresh_dir(output_dir)

    # Deterministically augment local generated Atlas state before any final selector is resolved.
    # This does not materialize source in Git and uses the same source-identity checks as the review.
    field_report = field_index.index_instance_fields(source, db, check=False)

    conn: sqlite3.Connection | None = None
    output.mkdir(parents=True)
    try:
        conn = vanilla_atlas.connect_db(db)
        source_sha = source_probe.require_pinned_source(conn, source, lock)
        selected = candidates(conn)
        resolved = _resolve_all(conn, selected)

        dossier_candidates: list[dict[str, object]] = []
        worksheet_candidates: list[dict[str, object]] = []
        with zipfile.ZipFile(source) as archive:
            for candidate, row in resolved:
                template = source_probe.record_template(conn, row, candidate.var_id)
                source_record = dict(template["source"])
                hazards = sorted(set(template.get("atlas_observed_hazards", [])))
                identity = f"{source_record['type']}#{source_record['signature']}"
                excerpt = _source_excerpt(archive, row)
                common = {
                    "candidate_id": candidate.var_id,
                    "source_identity": identity,
                    "source": source_record,
                    "atlas_observed_hazards": hazards,
                    "review_focus": list(candidate.review_focus),
                }
                dossier_candidates.append(
                    {
                        **common,
                        "path": str(row["path"]),
                        "start_line": int(row["start_line"]),
                        "end_line": int(row["end_line"]),
                        "source_excerpt": excerpt,
                        "source_excerpt_sha256": sha256_bytes(excerpt.encode("utf-8")),
                    }
                )
                worksheet_candidates.append(
                    {
                        **common,
                        "decision": {
                            "source_inspected": False,
                            "accepted": False,
                            "route_disposition": "",
                            "hazards_reviewed": [],
                            "followup_dependencies": [],
                            "semantic_observations": [],
                            "note": "",
                        },
                    }
                )

        dossier = {
            "schema": SCHEMA,
            "id": REVIEW_ID,
            "kind": PREPARED_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "source_archive_sha256": source_sha,
            "candidate_count": len(selected),
            "historical_first_pass_count": len(first_pass.CANDIDATES),
            "historical_followup_count": len(followup.CANDIDATES),
            "closure_count": len(selected) - len(first_pass.CANDIDATES) - len(followup.CANDIDATES),
            "route_dispositions": list(ROUTE_DISPOSITIONS),
            "instance_field_index": field_report,
            "candidates": dossier_candidates,
        }
        worksheet = {
            "schema": SCHEMA,
            "id": REVIEW_ID,
            "kind": WORKSHEET_KIND,
            "contains_official_source_text": False,
            "source_archive_sha256": source_sha,
            "candidate_count": len(selected),
            "route_dispositions": list(ROUTE_DISPOSITIONS),
            "candidates": worksheet_candidates,
        }
        (output / "review-dossier.json").write_bytes(pretty_bytes(dossier))
        (output / "review-worksheet.json").write_bytes(pretty_bytes(worksheet))
        manifest = {
            "schema": SCHEMA,
            "id": REVIEW_ID,
            "kind": PREPARED_KIND,
            "commit_policy": COMMIT_POLICY,
            "contains_official_source_text": True,
            "candidate_count": len(selected),
            "source_archive_sha256": source_sha,
            "artifacts": {
                "review_dossier": "review-dossier.json",
                "review_worksheet": "review-worksheet.json",
            },
            "next_required_step": (
                "Inspect every exact body, close only genuinely material delegates, then finalize "
                "directly into R2B SEM/VAR/GATE artifacts. Stale historical partial worksheets are "
                "not accepted by the finalizer."
            ),
        }
        (output / "manifest.json").write_bytes(pretty_bytes(manifest))
        return manifest
    except Exception:
        shutil.rmtree(output, ignore_errors=True)
        raise
    finally:
        if conn is not None:
            conn.close()


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r2b-play-entry-source-review")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--db", type=Path, default=source_probe.DEFAULT_DB)
    parser.add_argument("--source", type=Path, default=source_probe.DEFAULT_SOURCE)
    parser.add_argument("--lock", type=Path, default=source_probe.DEFAULT_LOCK)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        manifest = prepare(args.output_dir, args.db, args.source, args.lock)
    except (
        OSError,
        json.JSONDecodeError,
        sqlite3.Error,
        zipfile.BadZipFile,
        R2BPlayEntryReviewError,
        field_index.InstanceFieldIndexError,
        source_probe.ProbeError,
    ) as error:
        print(f"R2B Play-entry source-review error: {error}", file=sys.stderr)
        return 2
    print(f"r2b_play_entry_review={args.output_dir}")
    print(f"candidates={manifest['candidate_count']}")
    print("contains_official_source_text=true")
    print("commit_policy=EPHEMERAL_DO_NOT_COMMIT")
    print(f"worksheet={args.output_dir / 'review-worksheet.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
