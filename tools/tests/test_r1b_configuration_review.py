from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools import r1b_configuration_bundle_review as bundle_review
from tools import r1b_configuration_review as review
from tools import r1b_configuration_source_probe as source_probe


REPO_ROOT = Path(__file__).resolve().parents[2]
PLAN_PATH = REPO_ROOT / "vanilla/reviews/network/r1b-configuration-review-plan.json"
SEMANTICS_PATH = REPO_ROOT / "vanilla/semantics/network/R1_CONFIGURATION_SEMANTICS.md"


def write_json(path: Path, value: object) -> bytes:
    raw = bundle_review.pretty_bytes(value)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw)
    return raw


def synthetic_source(index: int) -> dict[str, str]:
    return {
        "type": f"net.minecraft.synthetic.Type{index}",
        "signature": f"method{index}()",
        "fingerprint_algorithm": "java-token-v2-literal-sensitive",
        "normalized_sha256": f"{index + 1:064x}",
        "body_sha256": f"{index + 101:064x}",
    }


def write_review_pack(root: Path) -> Path:
    pack = root / "review-pack"
    records = pack / "records"
    gate_dir = pack / "gate"
    records.mkdir(parents=True)
    gate_dir.mkdir()
    record_files: list[dict[str, str]] = []
    candidates: list[dict[str, object]] = []
    for index, (var_id, query) in enumerate(source_probe.CANDIDATES):
        source = synthetic_source(index)
        classifications = ["CLIENT_OBSERVABLE"] if index % 2 == 0 else []
        record = {
            "schema": 1,
            "id": var_id,
            "status": "INDEXED",
            "source": source,
            "classifications": classifications,
            "hazards_reviewed": [],
            "semantic_rules": [],
            "evidence": [],
            "notes": [],
        }
        relative = Path("records") / f"{var_id}.json"
        raw = write_json(pack / relative, record)
        record_files.append(
            {"var_id": var_id, "path": str(relative), "sha256": review.sha256_bytes(raw)}
        )
        candidates.append(
            {
                "var_id": var_id,
                "query": query,
                "source": source,
                "classifications": classifications,
                "atlas_observed_hazards": ["CODEC"] if index == 0 else [],
                "suggested_record_path": f"vanilla/records/network/r1/configuration/{var_id}.json",
            }
        )

    gate = {
        "schema": 1,
        "id": bundle_review.GATE_ID,
        "frontier": bundle_review.FRONTIER_NAME,
        "minimum_status": "VAR_REVIEWED",
        "require_semantic_rules": True,
        "require_hazards_reviewed": True,
        "methods": [
            {"query": query, "var_id": var_id} for var_id, query in source_probe.CANDIDATES
        ],
    }
    gate_relative = Path("gate") / f"{bundle_review.GATE_ID}.json"
    gate_raw = write_json(pack / gate_relative, gate)
    manifest = {
        "schema": 1,
        "kind": bundle_review.REVIEW_PACK_KIND,
        "commit_policy": bundle_review.COMMIT_POLICY,
        "contains_official_source_text": False,
        "source": {
            "minecraft": "26.2",
            "protocol": 776,
            "data_version": 4903,
            "source_archive_sha256": "1" * 64,
            "fingerprint_algorithm": "java-token-v2-literal-sensitive",
        },
        "ephemeral_bundle_sha256": "2" * 64,
        "frontier": {},
        "play_bootstrap": {},
        "review_candidates": candidates,
        "generated": {
            "record_files": record_files,
            "gate_file": {
                "path": str(gate_relative),
                "sha256": review.sha256_bytes(gate_raw),
                "suggested_repository_path": str(bundle_review.GATE_PATH),
            },
        },
        "review_requirements": {},
    }
    write_json(pack / "manifest.json", manifest)
    return pack


def accepted_worksheet(path: Path) -> dict[str, object]:
    worksheet = json.loads(path.read_text(encoding="utf-8"))
    for candidate in worksheet["candidates"]:
        decision = candidate["decision"]
        decision["source_inspected"] = True
        decision["accepted"] = True
        decision["hazards_reviewed"] = list(candidate["atlas_observed_hazards"])
        decision["semantic_rules"] = [candidate["semantic_rule_candidates"][0]]
        decision["notes"] = []
    path.write_bytes(review.pretty_bytes(worksheet))
    return worksheet


class R1BConfigurationReviewTests(unittest.TestCase):
    def test_review_plan_exactly_covers_source_candidates_and_source_semantics(self) -> None:
        plan, _raw = review.load_review_plan(PLAN_PATH, SEMANTICS_PATH)
        self.assertEqual(plan["capture_semantic_rules"], ["SEM-NET-R1B-012"])
        self.assertEqual(
            [(item["var_id"], item["query"]) for item in plan["candidates"]],
            list(source_probe.CANDIDATES),
        )
        linked = {
            rule for item in plan["candidates"] for rule in item["semantic_rule_candidates"]
        }
        self.assertEqual(linked, {f"SEM-NET-R1B-{index:03d}" for index in range(1, 12)})

    def test_prepare_creates_blank_source_free_bound_worksheet(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack = write_review_pack(root)
            worksheet_path = root / "worksheet.json"
            worksheet = review.prepare_worksheet(
                review_pack=pack,
                output=worksheet_path,
                plan_path=PLAN_PATH,
                semantics_path=SEMANTICS_PATH,
            )
            self.assertFalse(worksheet["contains_official_source_text"])
            self.assertEqual(len(worksheet["candidates"]), len(source_probe.CANDIDATES))
            for candidate in worksheet["candidates"]:
                self.assertEqual(
                    candidate["decision"],
                    {
                        "source_inspected": None,
                        "accepted": None,
                        "hazards_reviewed": [],
                        "semantic_rules": [],
                        "notes": [],
                    },
                )

    def test_finalize_promotes_only_completed_manual_review(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack = write_review_pack(root)
            worksheet_path = root / "worksheet.json"
            review.prepare_worksheet(
                review_pack=pack,
                output=worksheet_path,
                plan_path=PLAN_PATH,
                semantics_path=SEMANTICS_PATH,
            )
            accepted_worksheet(worksheet_path)
            output = root / "reviewed"
            manifest = review.finalize_review(
                review_pack=pack,
                worksheet_path=worksheet_path,
                output_dir=output,
                plan_path=PLAN_PATH,
                semantics_path=SEMANTICS_PATH,
            )
            self.assertFalse(manifest["contains_official_source_text"])
            self.assertEqual(len(manifest["records"]), len(source_probe.CANDIDATES))
            first_id = source_probe.CANDIDATES[0][0]
            first = json.loads((output / "records" / f"{first_id}.json").read_text())
            self.assertEqual(first["status"], "VAR_REVIEWED")
            self.assertEqual(first["hazards_reviewed"], ["CODEC"])
            self.assertTrue(first["semantic_rules"])
            gate = json.loads(
                (output / "gate" / f"{bundle_review.GATE_ID}.json").read_text()
            )
            self.assertEqual(gate["minimum_status"], "VAR_REVIEWED")
            self.assertTrue(gate["require_hazards_reviewed"])
            self.assertTrue(gate["require_semantic_rules"])

    def test_finalize_rejects_uninspected_source(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack = write_review_pack(root)
            worksheet_path = root / "worksheet.json"
            review.prepare_worksheet(
                review_pack=pack,
                output=worksheet_path,
                plan_path=PLAN_PATH,
                semantics_path=SEMANTICS_PATH,
            )
            worksheet = accepted_worksheet(worksheet_path)
            worksheet["candidates"][0]["decision"]["source_inspected"] = False
            worksheet_path.write_bytes(review.pretty_bytes(worksheet))
            with self.assertRaisesRegex(review.ReviewError, "source_inspected"):
                review.finalize_review(
                    review_pack=pack,
                    worksheet_path=worksheet_path,
                    output_dir=root / "reviewed",
                    plan_path=PLAN_PATH,
                    semantics_path=SEMANTICS_PATH,
                )

    def test_finalize_rejects_missing_observed_hazard_disposition(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack = write_review_pack(root)
            worksheet_path = root / "worksheet.json"
            review.prepare_worksheet(
                review_pack=pack,
                output=worksheet_path,
                plan_path=PLAN_PATH,
                semantics_path=SEMANTICS_PATH,
            )
            worksheet = accepted_worksheet(worksheet_path)
            worksheet["candidates"][0]["decision"]["hazards_reviewed"] = []
            worksheet_path.write_bytes(review.pretty_bytes(worksheet))
            with self.assertRaisesRegex(review.ReviewError, "lack explicit disposition"):
                review.finalize_review(
                    review_pack=pack,
                    worksheet_path=worksheet_path,
                    output_dir=root / "reviewed",
                    plan_path=PLAN_PATH,
                    semantics_path=SEMANTICS_PATH,
                )

    def test_finalize_rejects_capture_only_semantic_linkage(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack = write_review_pack(root)
            worksheet_path = root / "worksheet.json"
            review.prepare_worksheet(
                review_pack=pack,
                output=worksheet_path,
                plan_path=PLAN_PATH,
                semantics_path=SEMANTICS_PATH,
            )
            worksheet = accepted_worksheet(worksheet_path)
            worksheet["candidates"][0]["decision"]["semantic_rules"] = ["SEM-NET-R1B-012"]
            worksheet_path.write_bytes(review.pretty_bytes(worksheet))
            with self.assertRaisesRegex(review.ReviewError, "capture-only"):
                review.finalize_review(
                    review_pack=pack,
                    worksheet_path=worksheet_path,
                    output_dir=root / "reviewed",
                    plan_path=PLAN_PATH,
                    semantics_path=SEMANTICS_PATH,
                )

    def test_finalize_rejects_review_pack_drift_after_worksheet_creation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack = write_review_pack(root)
            worksheet_path = root / "worksheet.json"
            review.prepare_worksheet(
                review_pack=pack,
                output=worksheet_path,
                plan_path=PLAN_PATH,
                semantics_path=SEMANTICS_PATH,
            )
            accepted_worksheet(worksheet_path)
            manifest_path = pack / "manifest.json"
            manifest = json.loads(manifest_path.read_text())
            manifest["ephemeral_bundle_sha256"] = "3" * 64
            write_json(manifest_path, manifest)
            with self.assertRaisesRegex(review.ReviewError, "review_pack_manifest_sha256"):
                review.finalize_review(
                    review_pack=pack,
                    worksheet_path=worksheet_path,
                    output_dir=root / "reviewed",
                    plan_path=PLAN_PATH,
                    semantics_path=SEMANTICS_PATH,
                )

    def test_finalize_rejects_unknown_semantic_rule(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pack = write_review_pack(root)
            worksheet_path = root / "worksheet.json"
            review.prepare_worksheet(
                review_pack=pack,
                output=worksheet_path,
                plan_path=PLAN_PATH,
                semantics_path=SEMANTICS_PATH,
            )
            worksheet = accepted_worksheet(worksheet_path)
            worksheet["candidates"][0]["decision"]["semantic_rules"] = ["SEM-NET-R1B-999"]
            worksheet_path.write_bytes(review.pretty_bytes(worksheet))
            with self.assertRaisesRegex(review.ReviewError, "unknown SEM"):
                review.finalize_review(
                    review_pack=pack,
                    worksheet_path=worksheet_path,
                    output_dir=root / "reviewed",
                    plan_path=PLAN_PATH,
                    semantics_path=SEMANTICS_PATH,
                )


if __name__ == "__main__":
    unittest.main()
