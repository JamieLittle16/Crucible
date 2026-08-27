from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2b_play_entry_finalize as finalize
from tools import r2b_play_entry_final_seams_source_review as seams


REPO_ROOT = Path(__file__).resolve().parents[2]


def _source(seed: str) -> dict[str, str]:
    digest = hashlib.sha256(seed.encode()).hexdigest()
    return {
        "type": f"example.{seed}",
        "signature": "write()",
        "fingerprint_algorithm": "java-token-v2-literal-sensitive",
        "normalized_sha256": digest,
        "body_sha256": digest,
    }


def _final_seam_pair() -> tuple[dict[str, object], dict[str, object]]:
    prior = {
        "id": finalize.WIRE_117_ID,
        "candidate_count": finalize.WIRE_117_COUNT,
        "dossier_sha256": finalize.WIRE_117_DOSSIER_SHA256,
    }
    dossier_items = []
    worksheet_items = []
    for index, group in enumerate(sorted(finalize.EXPECTED_FINAL_SEAM_GROUPS), start=1):
        candidate_id = f"DISC-NET-R2B-PLAY-FINAL-SEAM-{index:03d}"
        source = _source(group)
        common = {
            "candidate_id": candidate_id,
            "group_ids": [group],
            "source_identity": f"{source['type']}#{source['signature']}",
            "source": source,
            "atlas_observed_hazards": ["CODEC"] if index == 1 else [],
            "review_focus": ["test fixture"],
        }
        dossier_items.append(
            {
                **common,
                "path": f"src/{index}.java",
                "start_line": 1,
                "end_line": 2,
                "source_excerpt": "fixture\n",
                "source_excerpt_sha256": hashlib.sha256(b"fixture\n").hexdigest(),
            }
        )
        worksheet_items.append(
            {
                **common,
                "decision": {
                    "source_inspected": True,
                    "accepted": True,
                    "hazards_reviewed": list(common["atlas_observed_hazards"]),
                    "followup_dependencies": [],
                    "semantic_observations": ["exact selected seam closed"],
                    "note": "reviewed fixture",
                },
            }
        )
    dossier = {
        "schema": 1,
        "id": seams.REVIEW_ID,
        "kind": seams.PREPARED_KIND,
        "commit_policy": seams.COMMIT_POLICY,
        "contains_official_source_text": True,
        "source_archive_sha256": finalize.SOURCE_SHA256,
        "prior_review": prior,
        "scope": "fixture",
        "candidate_count": len(dossier_items),
        "group_counts": {group: 1 for group in finalize.EXPECTED_FINAL_SEAM_GROUPS},
        "candidates": dossier_items,
    }
    worksheet = {
        "schema": 1,
        "id": seams.REVIEW_ID,
        "kind": seams.WORKSHEET_KIND,
        "contains_official_source_text": False,
        "source_archive_sha256": finalize.SOURCE_SHA256,
        "prior_review": prior,
        "scope": "fixture",
        "candidate_count": len(worksheet_items),
        "group_counts": {group: 1 for group in finalize.EXPECTED_FINAL_SEAM_GROUPS},
        "candidates": worksheet_items,
    }
    return dossier, worksheet


def _write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")


def _varint(value: int) -> bytes:
    result = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            result.append(byte | 0x80)
        else:
            result.append(byte)
            return bytes(result)


def _oracle() -> dict[str, object]:
    artifacts = []
    for index, (name, packet_id) in enumerate(finalize.EXPECTED_ORACLE.items()):
        body = _varint(packet_id) + bytes([index + 1, index + 2])
        _, width = finalize._decode_varint_prefix(body)
        artifacts.append(
            {
                "name": name,
                "semantic_group": "COMMAND_TREE" if name == "commands" else "SYNCHRONIZED_RECIPES",
                "phase": "play",
                "direction": "clientbound",
                "packet_id": packet_id,
                "play_body_index": index,
                "packet_id_bytes": width,
                "body_bytes": len(body),
                "body_sha256": hashlib.sha256(body).hexdigest(),
                "body_hex": body.hex(),
            }
        )
    return {
        "schema": 1,
        "kind": "r2b-play-bootstrap-oracle-v1",
        "oracle_only": True,
        "production_admitted": False,
        "target": {
            "minecraft": "26.2",
            "protocol": 776,
            "source_archive_sha256": finalize.SOURCE_SHA256,
            "capture_sha256": finalize.CAPTURE_SHA256,
        },
        "selected_capture_profile": {
            "player_name": "Stato16",
            "offline_profile_uuid": "fixture-offline",
            "session_uuid": "fixture-session",
        },
        "semantic_authority": "fixture",
        "artifacts": artifacts,
    }


class R2BPlayEntryFinalizeTests(unittest.TestCase):
    def test_repo_semantic_contract_contains_complete_selected_rule_set(self) -> None:
        result = finalize.validate_semantics(
            REPO_ROOT / "vanilla/semantics/network/R2B_PLAY_ENTRY_SEMANTICS.md",
            lock_path=REPO_ROOT / "vanilla/vanilla.lock.toml",
        )
        self.assertEqual(result["semantic_rules"], list(finalize.SEMANTIC_RULES))
        self.assertEqual(result["target"]["source_archive_sha256"], finalize.SOURCE_SHA256)

    def test_final_seams_require_complete_explicit_review(self) -> None:
        dossier, worksheet = _final_seam_pair()
        with tempfile.TemporaryDirectory() as tmp:
            dossier_path = Path(tmp) / "dossier.json"
            worksheet_path = Path(tmp) / "worksheet.json"
            _write_json(dossier_path, dossier)
            _write_json(worksheet_path, worksheet)
            result = finalize.validate_final_seams(dossier_path, worksheet_path)
        self.assertEqual(result["candidate_count"], 2)
        self.assertEqual(set(result["group_ids"]), finalize.EXPECTED_FINAL_SEAM_GROUPS)
        self.assertEqual(len(result["source_records"]), 2)

    def test_final_seams_reject_unreviewed_hazard(self) -> None:
        dossier, worksheet = _final_seam_pair()
        worksheet["candidates"][0]["decision"]["hazards_reviewed"] = []
        with tempfile.TemporaryDirectory() as tmp:
            dossier_path = Path(tmp) / "dossier.json"
            worksheet_path = Path(tmp) / "worksheet.json"
            _write_json(dossier_path, dossier)
            _write_json(worksheet_path, worksheet)
            with self.assertRaisesRegex(finalize.FinalizeError, "every observed hazard"):
                finalize.validate_final_seams(dossier_path, worksheet_path)

    def test_final_seams_reject_unresolved_dependency(self) -> None:
        dossier, worksheet = _final_seam_pair()
        worksheet["candidates"][0]["decision"]["followup_dependencies"] = ["escape"]
        with tempfile.TemporaryDirectory() as tmp:
            dossier_path = Path(tmp) / "dossier.json"
            worksheet_path = Path(tmp) / "worksheet.json"
            _write_json(dossier_path, dossier)
            _write_json(worksheet_path, worksheet)
            with self.assertRaisesRegex(finalize.FinalizeError, "unresolved final-seam"):
                finalize.validate_final_seams(dossier_path, worksheet_path)

    def test_final_seams_reject_tampered_source_record(self) -> None:
        dossier, worksheet = _final_seam_pair()
        original = worksheet["candidates"][0]["source"]
        worksheet["candidates"][0]["source"] = {**original, "body_sha256": "0" * 64}
        with tempfile.TemporaryDirectory() as tmp:
            dossier_path = Path(tmp) / "dossier.json"
            worksheet_path = Path(tmp) / "worksheet.json"
            _write_json(dossier_path, dossier)
            _write_json(worksheet_path, worksheet)
            with self.assertRaisesRegex(finalize.FinalizeError, "does not match source dossier"):
                finalize.validate_final_seams(dossier_path, worksheet_path)

    def test_oracle_validates_bodies_ids_and_hashes(self) -> None:
        value = _oracle()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "oracle.json"
            _write_json(path, value)
            result = finalize.validate_oracle(path)
        self.assertEqual([item["name"] for item in result["artifacts"]], ["commands", "update-recipes"])

    def test_oracle_rejects_tampered_body(self) -> None:
        value = _oracle()
        value["artifacts"][0]["body_sha256"] = "0" * 64
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "oracle.json"
            _write_json(path, value)
            with self.assertRaisesRegex(finalize.FinalizeError, "body length/hash mismatch"):
                finalize.validate_oracle(path)

    def test_pinned_prior_review_digests_are_exact(self) -> None:
        self.assertEqual(
            finalize.FINAL_67_DOSSIER_SHA256,
            "aecd49dd79962a905b5edb8f586c109ce731c304afe4f0f7c3ad3275c906b8b9",
        )
        self.assertEqual(
            finalize.WIRE_117_DOSSIER_SHA256,
            "93999fca0a4c69eda607e729af61c74e7ce40c96bf4201516904fabf79bc2e3a",
        )


if __name__ == "__main__":
    unittest.main()
