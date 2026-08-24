#!/usr/bin/env python3
"""Build and finalize the supplemental R1B Configuration source-closure review.

The existing 25-entry R1B review covers the direct Configuration spine. This tool binds the
source bodies hidden behind delegation edges that the first review deliberately exposed: packet
codec constructors/writers, task-queue helpers, registry/tag container construction and the nested
prepared-spawn state machine.

`prepare` writes a source-rich dossier plus a source-free worksheet/INDEXED record set outside the
repository. `finalize` consumes only the source-free worksheet/record set and emits VAR_REVIEWED
records suitable for an independent `vanilla_source_gate.py` run.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence

try:
    from . import r1b_configuration_source_probe as source_probe
    from . import vanilla_atlas
except ImportError:
    import r1b_configuration_source_probe as source_probe  # type: ignore[no-redef]
    import vanilla_atlas  # type: ignore[no-redef]

SCHEMA = 1
GATE_ID = "GATE-NET-CONFIG-CLOSURE-26_2-001"
PREPARED_KIND = "r1b-configuration-source-closure-review"
WORKSHEET_KIND = "r1b-configuration-source-closure-worksheet"
COMMIT_POLICY = "EPHEMERAL_DO_NOT_COMMIT"
REPO_ROOT = Path(__file__).resolve().parents[1]


@dataclass(frozen=True)
class Candidate:
    var_id: str
    type_name: str
    method_name: str
    param_count: int
    semantic_rules: tuple[str, ...]
    review_focus: tuple[str, ...]


CANDIDATES: tuple[Candidate, ...] = (
    Candidate("VAR-NET-R1B-CLOSURE-ENABLED-FEATURES-READ-001", "net.minecraft.network.protocol.configuration.ClientboundUpdateEnabledFeaturesPacket", "ClientboundUpdateEnabledFeaturesPacket", 1, ("SEM-NET-R1B-002",), ("Bind the delegated enabled-feature collection decoder and identifier element law.",)),
    Candidate("VAR-NET-R1B-CLOSURE-ENABLED-FEATURES-WRITE-001", "net.minecraft.network.protocol.configuration.ClientboundUpdateEnabledFeaturesPacket", "write", 1, ("SEM-NET-R1B-002",), ("Bind the delegated enabled-feature collection encoder and element ordering.",)),
    Candidate("VAR-NET-R1B-CLOSURE-UPDATE-TAGS-READ-001", "net.minecraft.network.protocol.common.ClientboundUpdateTagsPacket", "ClientboundUpdateTagsPacket", 1, ("SEM-NET-R1B-007",), ("Bind the outer registry-key to NetworkPayload map decoder.",)),
    Candidate("VAR-NET-R1B-CLOSURE-UPDATE-TAGS-WRITE-001", "net.minecraft.network.protocol.common.ClientboundUpdateTagsPacket", "write", 1, ("SEM-NET-R1B-007",), ("Bind the outer registry-key to NetworkPayload map encoder.",)),
    Candidate("VAR-NET-R1B-CLOSURE-CLIENT-INFO-PACKET-READ-001", "net.minecraft.network.protocol.common.ServerboundClientInformationPacket", "ServerboundClientInformationPacket", 1, ("SEM-NET-R1B-004",), ("Bind the packet decoder to ClientInformation construction.",)),
    Candidate("VAR-NET-R1B-CLOSURE-CLIENT-INFO-PACKET-WRITE-001", "net.minecraft.network.protocol.common.ServerboundClientInformationPacket", "write", 1, ("SEM-NET-R1B-004",), ("Bind the packet encoder to ClientInformation write order.",)),
    Candidate("VAR-NET-R1B-CLOSURE-CLIENT-INFO-READ-001", "net.minecraft.server.level.ClientInformation", "ClientInformation", 1, ("SEM-NET-R1B-004",), ("Bind language/view/chat/model/main-hand/filtering/listing/particle decode order and language bound.",)),
    Candidate("VAR-NET-R1B-CLOSURE-CLIENT-INFO-WRITE-001", "net.minecraft.server.level.ClientInformation", "write", 1, ("SEM-NET-R1B-004",), ("Bind ClientInformation encode order for wire-law symmetry.",)),
    Candidate("VAR-NET-R1B-CLOSURE-CLIENT-INFO-DEFAULT-001", "net.minecraft.server.level.ClientInformation", "createDefault", 0, ("SEM-NET-R1B-004",), ("Bind the initial default ClientInformation values used before replacement.",)),
    Candidate("VAR-NET-R1B-CLOSURE-BRAND-READ-001", "net.minecraft.network.protocol.common.custom.BrandPayload", "BrandPayload", 1, ("SEM-NET-R1B-003",), ("Bind brand payload string decoding behind STREAM_CODEC.",)),
    Candidate("VAR-NET-R1B-CLOSURE-BRAND-WRITE-001", "net.minecraft.network.protocol.common.custom.BrandPayload", "write", 1, ("SEM-NET-R1B-002", "SEM-NET-R1B-003"), ("Bind brand payload string encoding behind STREAM_CODEC.",)),
    Candidate("VAR-NET-R1B-CLOSURE-CONFIG-OPTIONAL-TASKS-001", "net.minecraft.server.network.ServerConfigurationPacketListenerImpl", "addOptionalTasks", 0, ("SEM-NET-R1B-002", "SEM-NET-R1B-008"), ("Bind optional code-of-conduct/resource-pack task insertion conditions.",)),
    Candidate("VAR-NET-R1B-CLOSURE-CONFIG-RETURN-WORLD-001", "net.minecraft.server.network.ServerConfigurationPacketListenerImpl", "returnToWorld", 0, ("SEM-NET-R1B-002", "SEM-NET-R1B-009", "SEM-NET-R1B-010"), ("Bind initial-login PrepareSpawn/JoinWorld task enqueue order versus return-to-world path.",)),
    Candidate("VAR-NET-R1B-CLOSURE-CONFIG-START-NEXT-001", "net.minecraft.server.network.ServerConfigurationPacketListenerImpl", "startNextTask", 0, ("SEM-NET-R1B-002", "SEM-NET-R1B-006", "SEM-NET-R1B-010"), ("Bind single-current-task queue progression and task start ordering.",)),
    Candidate("VAR-NET-R1B-CLOSURE-CONFIG-FINISH-TASK-001", "net.minecraft.server.network.ServerConfigurationPacketListenerImpl", "finishCurrentTask", 1, ("SEM-NET-R1B-006", "SEM-NET-R1B-010", "SEM-NET-R1B-011"), ("Bind wrong-task rejection and transition to the next Configuration task.",)),
    Candidate("VAR-NET-R1B-CLOSURE-CONFIG-TICK-001", "net.minecraft.server.network.ServerConfigurationPacketListenerImpl", "tick", 0, ("SEM-NET-R1B-009", "SEM-NET-R1B-010"), ("Bind tick-driven task completion and prepared-spawn keep-alive coupling.",)),
    Candidate("VAR-NET-R1B-CLOSURE-REGISTRY-PACK-ALL-001", "net.minecraft.core.RegistrySynchronization", "packRegistries", 4, ("SEM-NET-R1B-005", "SEM-NET-R1B-006", "SEM-NET-R1B-007"), ("Bind the synchronized-registry iteration that feeds registry_data publication.",)),
    Candidate("VAR-NET-R1B-CLOSURE-REGISTRY-PACK-ONE-001", "net.minecraft.core.RegistrySynchronization", "packRegistry", 5, ("SEM-NET-R1B-005", "SEM-NET-R1B-006", "SEM-NET-R1B-007"), ("Bind known-pack content elision and packed registry entry construction.",)),
    Candidate("VAR-NET-R1B-CLOSURE-PACKED-REGISTRY-CODEC-001", "net.minecraft.core.RegistrySynchronization$PackedRegistryEntry", "<clinit>", 0, ("SEM-NET-R1B-007",), ("Bind Identifier plus optional Tag packed-entry wire structure.",)),
    Candidate("VAR-NET-R1B-CLOSURE-TAGS-SERIALIZE-ALL-001", "net.minecraft.tags.TagNetworkSerialization", "serializeTagsToNetwork", 1, ("SEM-NET-R1B-007",), ("Bind registry-key to non-empty network-tag payload selection.",)),
    Candidate("VAR-NET-R1B-CLOSURE-TAGS-SERIALIZE-ONE-001", "net.minecraft.tags.TagNetworkSerialization", "serializeToNetwork", 1, ("SEM-NET-R1B-007",), ("Bind tag identifier to integer registry-id list construction and reference-holder requirement.",)),
    Candidate("VAR-NET-R1B-CLOSURE-TAG-PAYLOAD-WRITE-001", "net.minecraft.tags.TagNetworkSerialization$NetworkPayload", "write", 1, ("SEM-NET-R1B-007",), ("Bind nested tag map encoding through identifier/int-id-list helpers.",)),
    Candidate("VAR-NET-R1B-CLOSURE-TAG-PAYLOAD-READ-001", "net.minecraft.tags.TagNetworkSerialization$NetworkPayload", "read", 1, ("SEM-NET-R1B-007",), ("Bind nested tag map decoding for exact external field law.",)),
    Candidate("VAR-NET-R1B-CLOSURE-PREPARE-SPAWN-SPAWN-PLAYER-001", "net.minecraft.server.network.config.PrepareSpawnTask", "spawnPlayer", 2, ("SEM-NET-R1B-009", "SEM-NET-R1B-011"), ("Bind the hard Ready-state precondition before the Play handoff may spawn.",)),
    Candidate("VAR-NET-R1B-CLOSURE-PREPARE-SPAWN-KEEPALIVE-001", "net.minecraft.server.network.config.PrepareSpawnTask", "keepAlive", 0, ("SEM-NET-R1B-009",), ("Bind outer prepared-spawn keep-alive delegation.",)),
    Candidate("VAR-NET-R1B-CLOSURE-PREPARE-SPAWN-PREPARING-TICK-001", "net.minecraft.server.network.config.PrepareSpawnTask$Preparing", "tick", 0, ("SEM-NET-R1B-009",), ("Bind radius-3 PLAYER_SPAWN ticket/load future and Ready transition.",)),
    Candidate("VAR-NET-R1B-CLOSURE-PREPARE-SPAWN-READY-KEEPALIVE-001", "net.minecraft.server.network.config.PrepareSpawnTask$Ready", "keepAlive", 0, ("SEM-NET-R1B-009",), ("Bind radius-3 PLAYER_SPAWN ticket renewal while Configuration remains active.",)),
    Candidate("VAR-NET-R1B-CLOSURE-PREPARE-SPAWN-READY-SPAWN-001", "net.minecraft.server.network.config.PrepareSpawnTask$Ready", "spawn", 2, ("SEM-NET-R1B-009", "SEM-NET-R1B-011"), ("Bind radius-3 entity readiness, player construction and placeNewPlayer handoff.",)),
    Candidate("VAR-NET-R1B-CLOSURE-BYTEBUF-COLLECTION-001", "net.minecraft.network.codec.ByteBufCodecs", "collection", 3, ("SEM-NET-R1B-005", "SEM-NET-R1B-007"), ("Bind VarInt collection count, max enforcement, bounded initial allocation and element loop.",)),
    Candidate("VAR-NET-R1B-CLOSURE-BYTEBUF-LIST-UNBOUNDED-001", "net.minecraft.network.codec.ByteBufCodecs", "list", 0, ("SEM-NET-R1B-005", "SEM-NET-R1B-007"), ("Bind the default list codec to collection semantics with the default maximum.",)),
    Candidate("VAR-NET-R1B-CLOSURE-BYTEBUF-LIST-BOUNDED-001", "net.minecraft.network.codec.ByteBufCodecs", "list", 1, ("SEM-NET-R1B-005",), ("Bind bounded list codec construction used by serverbound known-pack selection.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-WRITE-COLLECTION-001", "net.minecraft.network.FriendlyByteBuf", "writeCollection", 2, ("SEM-NET-R1B-002",), ("Bind enabled-feature VarInt count and identifier iteration on the clientbound encoder.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-READ-COLLECTION-001", "net.minecraft.network.FriendlyByteBuf", "readCollection", 2, ("SEM-NET-R1B-002",), ("Bind collection count and element iteration for the packet decoder law.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-WRITE-MAP-001", "net.minecraft.network.FriendlyByteBuf", "writeMap", 3, ("SEM-NET-R1B-007",), ("Bind update-tags map count and key/value iteration.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-READ-MAP-001", "net.minecraft.network.FriendlyByteBuf", "readMap", 2, ("SEM-NET-R1B-007",), ("Bind update-tags map decode count and key/value iteration.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-WRITE-INT-ID-LIST-001", "net.minecraft.network.FriendlyByteBuf", "writeIntIdList", 1, ("SEM-NET-R1B-007",), ("Bind tag integer-id list VarInt count and element encoding.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-READ-INT-ID-LIST-001", "net.minecraft.network.FriendlyByteBuf", "readIntIdList", 0, ("SEM-NET-R1B-007",), ("Bind tag integer-id list VarInt count and element decoding.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-WRITE-IDENTIFIER-001", "net.minecraft.network.FriendlyByteBuf", "writeIdentifier", 1, ("SEM-NET-R1B-002", "SEM-NET-R1B-007"), ("Bind identifier serialization used by features and tags.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-READ-IDENTIFIER-001", "net.minecraft.network.FriendlyByteBuf", "readIdentifier", 0, ("SEM-NET-R1B-007",), ("Bind identifier decoding used by tag payloads.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-WRITE-RESOURCE-KEY-001", "net.minecraft.network.FriendlyByteBuf", "writeResourceKey", 1, ("SEM-NET-R1B-007",), ("Bind registry resource-key encoding for update-tags outer keys.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-READ-REGISTRY-KEY-001", "net.minecraft.network.FriendlyByteBuf", "readRegistryKey", 0, ("SEM-NET-R1B-007",), ("Bind registry resource-key decoding for update-tags outer keys.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-READ-UTF-BOUNDED-001", "net.minecraft.network.FriendlyByteBuf", "readUtf", 1, ("SEM-NET-R1B-004",), ("Bind explicit maximum-length UTF decoding used by ClientInformation language.",)),
    Candidate("VAR-NET-R1B-CLOSURE-FRIENDLY-READ-ENUM-001", "net.minecraft.network.FriendlyByteBuf", "readEnum", 1, ("SEM-NET-R1B-004",), ("Bind enum ordinal decoding used by ClientInformation fields.",)),
)


class ClosureError(RuntimeError):
    pass


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode()


def _external_fresh_dir(path: Path) -> Path:
    if path.exists() or path.is_symlink():
        raise ClosureError(f"output directory must not already exist: {path}")
    resolved = path.resolve(strict=False)
    repo = REPO_ROOT.resolve(strict=True)
    try:
        resolved.relative_to(repo)
    except ValueError:
        return resolved
    raise ClosureError("source-closure review contains official source text and must live outside the repository")


def _resolve(conn: Any, candidate: Candidate) -> Any:
    rows = conn.execute(
        """SELECT m.id,t.qualified_name,m.name,m.signature,m.start_line,m.end_line,f.path
           FROM methods m JOIN types t ON t.id=m.type_id JOIN source_files f ON f.id=t.file_id
           WHERE t.qualified_name=? AND m.name=? AND m.param_count=?
           ORDER BY m.start_line""",
        (candidate.type_name, candidate.method_name, candidate.param_count),
    ).fetchall()
    if len(rows) != 1:
        identities = [f"{row['qualified_name']}#{row['signature']}" for row in rows]
        raise ClosureError(
            f"{candidate.var_id}: selector {candidate.type_name}#{candidate.method_name}/{candidate.param_count} "
            f"resolved {len(rows)} methods: {identities}"
        )
    return rows[0]


def _source_excerpt(archive: zipfile.ZipFile, row: Any) -> str:
    path = str(row["path"])
    try:
        text = archive.read(path).decode("utf-8", errors="strict")
    except KeyError as error:
        raise ClosureError(f"source member missing: {path}") from error
    lines = text.splitlines()
    start = int(row["start_line"])
    end = int(row["end_line"])
    if not (1 <= start <= end <= len(lines)):
        raise ClosureError(f"invalid Atlas line range for {path}: {start}-{end}")
    return "\n".join(lines[start - 1 : end]) + "\n"


def prepare(output_dir: Path, db: Path, source: Path, lock: Path) -> dict[str, object]:
    output = _external_fresh_dir(output_dir)
    output.mkdir(parents=True)
    try:
        conn = vanilla_atlas.connect_db(db)
        source_sha = source_probe.require_pinned_source(conn, source, lock)
        with zipfile.ZipFile(source) as archive:
            dossier_candidates: list[dict[str, object]] = []
            worksheet_candidates: list[dict[str, object]] = []
            records: list[dict[str, object]] = []
            methods: list[dict[str, str]] = []
            for candidate in CANDIDATES:
                row = _resolve(conn, candidate)
                record = source_probe.record_template(conn, row, candidate.var_id)
                source_record = dict(record["source"])
                identity = f"{source_record['type']}#{source_record['signature']}"
                hazards = sorted(set(record.get("atlas_observed_hazards", [])))
                excerpt = _source_excerpt(archive, row)
                dossier_candidates.append({
                    "var_id": candidate.var_id,
                    "source_identity": identity,
                    "path": str(row["path"]),
                    "start_line": int(row["start_line"]),
                    "end_line": int(row["end_line"]),
                    "source": source_record,
                    "atlas_observed_hazards": hazards,
                    "semantic_rule_candidates": list(candidate.semantic_rules),
                    "review_focus": list(candidate.review_focus),
                    "source_excerpt": excerpt,
                    "source_excerpt_sha256": sha256_bytes(excerpt.encode()),
                })
                worksheet_candidates.append({
                    "var_id": candidate.var_id,
                    "source_identity": identity,
                    "source": source_record,
                    "atlas_observed_hazards": hazards,
                    "semantic_rule_candidates": list(candidate.semantic_rules),
                    "review_focus": list(candidate.review_focus),
                    "decision": {"source_inspected": False, "accepted": False, "reviewer": "", "note": "", "hazards_reviewed": [], "semantic_rules": []},
                })
                record.pop("atlas_observed_hazards", None)
                records.append(record)
                methods.append({"query": identity, "var_id": candidate.var_id})
        conn.close()
        dossier = {"schema": SCHEMA, "kind": PREPARED_KIND, "commit_policy": COMMIT_POLICY, "contains_official_source_text": True, "source_archive_sha256": source_sha, "candidate_count": len(CANDIDATES), "candidates": dossier_candidates}
        worksheet = {"schema": SCHEMA, "kind": WORKSHEET_KIND, "contains_official_source_text": False, "source_archive_sha256": source_sha, "candidate_count": len(CANDIDATES), "candidates": worksheet_candidates}
        gate = {"schema": SCHEMA, "id": GATE_ID, "minimum_status": "VAR_REVIEWED", "require_semantic_rules": True, "require_hazards_reviewed": True, "methods": methods}
        (output / "records").mkdir()
        (output / "gate").mkdir()
        for record in records:
            (output / "records" / f"{record['id']}.json").write_bytes(canonical_json(record))
        (output / "review-dossier.json").write_bytes(canonical_json(dossier))
        (output / "review-worksheet.json").write_bytes(canonical_json(worksheet))
        (output / "gate" / f"{GATE_ID}.json").write_bytes(canonical_json(gate))
        manifest = {"schema": SCHEMA, "kind": PREPARED_KIND, "commit_policy": COMMIT_POLICY, "contains_official_source_text": True, "candidate_count": len(CANDIDATES), "source_archive_sha256": source_sha, "artifacts": {"review_dossier": "review-dossier.json", "review_worksheet": "review-worksheet.json", "indexed_records": "records", "gate": f"gate/{GATE_ID}.json"}}
        (output / "manifest.json").write_bytes(canonical_json(manifest))
        return manifest
    except Exception:
        shutil.rmtree(output, ignore_errors=True)
        raise


def finalize(review_dir: Path, output_dir: Path) -> None:
    if output_dir.exists() or output_dir.is_symlink():
        raise ClosureError(f"finalized output must not already exist: {output_dir}")
    worksheet = json.loads((review_dir / "review-worksheet.json").read_text(encoding="utf-8"))
    if worksheet.get("kind") != WORKSHEET_KIND or worksheet.get("candidate_count") != len(CANDIDATES):
        raise ClosureError("worksheet identity/cardinality mismatch")
    entries = worksheet.get("candidates")
    if not isinstance(entries, list) or len(entries) != len(CANDIDATES):
        raise ClosureError("worksheet candidates mismatch")
    indexed: dict[str, dict[str, object]] = {}
    for path in (review_dir / "records").glob("*.json"):
        record = json.loads(path.read_text(encoding="utf-8"))
        indexed[str(record["id"])] = record
    output_dir.mkdir(parents=True)
    (output_dir / "records").mkdir()
    (output_dir / "gate").mkdir()
    for candidate, entry in zip(CANDIDATES, entries):
        if entry.get("var_id") != candidate.var_id:
            raise ClosureError(f"worksheet order/id mismatch at {candidate.var_id}")
        decision = entry.get("decision")
        if not isinstance(decision, dict):
            raise ClosureError(f"{candidate.var_id}: decision object missing")
        if decision.get("source_inspected") is not True or decision.get("accepted") is not True:
            raise ClosureError(f"{candidate.var_id}: source must be explicitly inspected and accepted")
        reviewer = decision.get("reviewer")
        note = decision.get("note")
        if not isinstance(reviewer, str) or not reviewer.strip() or not isinstance(note, str) or not note.strip():
            raise ClosureError(f"{candidate.var_id}: reviewer and note are required")
        observed = set(entry.get("atlas_observed_hazards", []))
        reviewed = decision.get("hazards_reviewed")
        if not isinstance(reviewed, list) or any(not isinstance(item, str) or not item for item in reviewed):
            raise ClosureError(f"{candidate.var_id}: hazards_reviewed must be a string array")
        if observed - set(reviewed):
            raise ClosureError(f"{candidate.var_id}: undispositioned hazards: {sorted(observed - set(reviewed))}")
        semantic_rules = decision.get("semantic_rules")
        allowed = set(candidate.semantic_rules)
        if not isinstance(semantic_rules, list) or not semantic_rules or any(rule not in allowed for rule in semantic_rules):
            raise ClosureError(f"{candidate.var_id}: semantic_rules must be a non-empty subset of {sorted(allowed)}")
        record = indexed.get(candidate.var_id)
        if record is None:
            raise ClosureError(f"{candidate.var_id}: INDEXED record missing")
        source = entry.get("source")
        if source != record.get("source"):
            raise ClosureError(f"{candidate.var_id}: worksheet/record source identity drift")
        record["status"] = "VAR_REVIEWED"
        record["hazards_reviewed"] = sorted(set(reviewed))
        record["semantic_rules"] = sorted(set(semantic_rules))
        record["evidence"] = ["R1B supplemental source-closure review"]
        record["notes"] = [f"Reviewer: {reviewer.strip()}", note.strip()]
        (output_dir / "records" / f"{candidate.var_id}.json").write_bytes(canonical_json(record))
    shutil.copyfile(review_dir / "gate" / f"{GATE_ID}.json", output_dir / "gate" / f"{GATE_ID}.json")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="r1b-configuration-source-closure")
    sub = parser.add_subparsers(dest="command", required=True)
    prepare_parser = sub.add_parser("prepare")
    prepare_parser.add_argument("--output-dir", type=Path, required=True)
    prepare_parser.add_argument("--db", type=Path, default=source_probe.DEFAULT_DB)
    prepare_parser.add_argument("--source", type=Path, default=source_probe.DEFAULT_SOURCE)
    prepare_parser.add_argument("--lock", type=Path, default=source_probe.DEFAULT_LOCK)
    finalize_parser = sub.add_parser("finalize")
    finalize_parser.add_argument("--review-dir", type=Path, required=True)
    finalize_parser.add_argument("--output-dir", type=Path, required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "prepare":
            manifest = prepare(args.output_dir, args.db, args.source, args.lock)
            print(f"closure_review={args.output_dir}")
            print(f"candidates={manifest['candidate_count']}")
            print("contains_official_source_text=true")
            print("commit_policy=EPHEMERAL_DO_NOT_COMMIT")
            print(f"worksheet={args.output_dir / 'review-worksheet.json'}")
        else:
            finalize(args.review_dir, args.output_dir)
            print(f"reviewed_closure={args.output_dir}")
            print("next_source_gate_command=python3 tools/vanilla_source_gate.py " f"--db {source_probe.DEFAULT_DB} --gate {args.output_dir / 'gate' / (GATE_ID + '.json')} " f"--records {args.output_dir / 'records'} --output {args.output_dir / (GATE_ID + '-source-admission.json')}")
        return 0
    except (OSError, json.JSONDecodeError, zipfile.BadZipFile, ClosureError, source_probe.ProbeError) as error:
        print(f"R1B Configuration source-closure error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
