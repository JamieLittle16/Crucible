from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_delegate_closure_source_review as closure
from tools import r2c_world_state_source_review_delegate_finalize as finalize


def pretty(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def source(identity: str, token: str) -> dict[str, str]:
    owner, signature = identity.split("#", 1)
    return {
        "type": owner,
        "signature": signature,
        "fingerprint_algorithm": "java-token-v2-literal-sensitive",
        "normalized_sha256": hashlib.sha256(f"norm:{token}".encode()).hexdigest(),
        "body_sha256": hashlib.sha256(f"body:{token}".encode()).hexdigest(),
    }


def record(
    *,
    candidate_id: str,
    identity: str,
    token: str,
    group_id: str,
    focus: str,
    hazards: list[str],
) -> dict[str, object]:
    excerpt = f"SECRET_SOURCE_{token}\n"
    return {
        "candidate_id": candidate_id,
        "source_identity": identity,
        "source": source(identity, token),
        "source_location": {
            "path": f"src/net/minecraft/{token}.java",
            "start_line": 10,
            "end_line": 20,
        },
        "atlas_observed_hazards": hazards,
        "atlas_classifications": ["CLIENT_OBSERVABLE"],
        "calls": {
            "call_sites": 1,
            "resolved_targets": [],
            "unresolved_call_sites": 1,
            "top_unresolved_callees": [{"callee": "helper/0", "sites": 1}],
        },
        "group_ids": [group_id],
        "review_focus": [focus],
        "source_excerpt": excerpt,
        "source_excerpt_sha256": hashlib.sha256(excerpt.encode()).hexdigest(),
    }


def fixture_payloads() -> dict[str, bytes]:
    plan = closure._load_plan()
    biome = record(
        candidate_id="DISC-NET-R2C-WORLD-DELEGATE-0001",
        identity="net.minecraft.world.level.chunk.PalettedContainer$Data#write()",
        token="biome",
        group_id=plan.groups[0].group_id,
        focus=plan.groups[0].review_focus,
        hazards=["CODEC"],
    )
    light = record(
        candidate_id="DISC-NET-R2C-WORLD-DELEGATE-0002",
        identity="net.minecraft.world.level.chunk.DataLayer#getData()",
        token="light",
        group_id=plan.groups[1].group_id,
        focus=plan.groups[1].review_focus,
        hazards=["CLIENT_OBSERVABLE"],
    )
    payloads = closure._payloads(
        plan=plan,
        plan_sha256=finalize._sha256_file(closure.DEFAULT_PLAN),
        parent_plan_sha256=finalize._sha256_file(closure.DEFAULT_PARENT_PLAN),
        frontier_sha256=finalize._sha256_file(closure.DEFAULT_FRONTIER),
        source_sha256=closure.EXPECTED_SOURCE_SHA256,
        records=[biome, light],
    )
    worksheet = json.loads(payloads["worksheet.json"])
    for group in worksheet["groups"]:
        identities = [candidate["source_identity"] for candidate in group["candidates"]]
        group["source_inspected"] = True
        group["selected_source_identities"] = identities
        group["rejected_source_identities"] = []
        group["hazards_reviewed"] = sorted(
            {
                hazard
                for candidate in group["candidates"]
                for hazard in candidate["atlas_observed_hazards"]
            }
        )
        group["followup_dependencies"] = []
        group["semantic_observations"] = [f"Reviewed exact {group['group_id']} delegate law."]
        group["review_complete"] = True
    payloads["worksheet.json"] = pretty(worksheet)
    return payloads


def manifest_file(value: dict[str, object], path: str) -> dict[str, object]:
    for raw in value["files"]:  # type: ignore[index]
        item = raw  # type: ignore[assignment]
        if item["path"] == path:  # type: ignore[index]
            return item  # type: ignore[return-value]
    raise AssertionError(f"missing manifest file {path}")


class R2cWorldStateDelegateFinalizeTests(unittest.TestCase):
    def write_fixture(self, root: Path) -> tuple[Path, Path, Path, Path]:
        payloads = fixture_payloads()
        pack = root / "review-pack.json"
        worksheet = root / "worksheet.json"
        manifest = root / "manifest.json"
        output = root / "delegate-review-result.json"
        pack.write_bytes(payloads["review-pack.json"])
        worksheet.write_bytes(payloads["worksheet.json"])
        manifest.write_bytes(payloads["manifest.json"])
        return pack, worksheet, manifest, output

    def test_complete_delegate_review_emits_source_free_result(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack, worksheet, manifest, output = self.write_fixture(Path(temporary))
            original_manifest = json.loads(manifest.read_text())
            original_worksheet = manifest_file(original_manifest, "worksheet.json")
            completed_worksheet_sha = hashlib.sha256(worksheet.read_bytes()).hexdigest()

            summary = finalize.finalize(pack, worksheet, manifest, output)
            result = json.loads(output.read_text())
            text = output.read_text()

            self.assertEqual(summary["groups"], 2)
            self.assertEqual(summary["selected_sources"], 2)
            self.assertFalse(summary["contains_official_source_text"])
            self.assertEqual(result["kind"], finalize.RESULT_KIND)
            self.assertEqual(result["id"], finalize.RESULT_ID)
            self.assertTrue(result["all_groups_review_complete"])
            self.assertFalse(result["production_admitted"])
            self.assertFalse(result["contains_official_source_text"])
            self.assertNotIn("source_excerpt", text)
            self.assertNotIn("SECRET_SOURCE", text)
            self.assertEqual(result["generated_worksheet_sha256"], original_worksheet["sha256"])
            self.assertEqual(result["generated_worksheet_size"], original_worksheet["size"])
            self.assertEqual(result["worksheet_sha256"], completed_worksheet_sha)
            self.assertNotEqual(result["worksheet_sha256"], result["generated_worksheet_sha256"])
            self.assertEqual(
                [(group["group_id"], group["parent_group_id"]) for group in result["groups"]],
                list(closure.EXPECTED_GROUPS),
            )

    def test_manifest_tamper_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack, worksheet, manifest, output = self.write_fixture(Path(temporary))
            value = json.loads(manifest.read_text())
            value["files"][0]["sha256"] = "0" * 64
            manifest.write_bytes(pretty(value))
            with self.assertRaisesRegex(finalize.FinalizeError, "manifest metadata mismatch"):
                finalize.finalize(pack, worksheet, manifest, output)

    def test_generated_worksheet_manifest_tamper_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack, worksheet, manifest, output = self.write_fixture(Path(temporary))
            value = json.loads(manifest.read_text())
            item = manifest_file(value, "worksheet.json")
            item["sha256"] = "0" * 64
            manifest.write_bytes(pretty(value))
            with self.assertRaisesRegex(finalize.FinalizeError, "manifest metadata mismatch for worksheet.json"):
                finalize.finalize(pack, worksheet, manifest, output)

    def test_current_plan_provenance_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack, worksheet, manifest, output = self.write_fixture(Path(temporary))
            pack_value = json.loads(pack.read_text())
            pack_value["plan_sha256"] = "0" * 64
            pack.write_bytes(pretty(pack_value))
            with self.assertRaisesRegex(finalize.FinalizeError, "identity/provenance mismatch"):
                finalize.finalize(pack, worksheet, manifest, output)

    def test_unresolved_followup_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack, worksheet, manifest, output = self.write_fixture(Path(temporary))
            value = json.loads(worksheet.read_text())
            value["groups"][0]["followup_dependencies"] = ["third-order delegate"]
            worksheet.write_bytes(pretty(value))
            with self.assertRaisesRegex(finalize.FinalizeError, "unresolved followup"):
                finalize.finalize(pack, worksheet, manifest, output)

    def test_selected_hazards_must_be_reviewed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack, worksheet, manifest, output = self.write_fixture(Path(temporary))
            value = json.loads(worksheet.read_text())
            value["groups"][1]["hazards_reviewed"] = []
            worksheet.write_bytes(pretty(value))
            with self.assertRaisesRegex(finalize.FinalizeError, "hazards are not fully reviewed"):
                finalize.finalize(pack, worksheet, manifest, output)

    def test_selection_must_exactly_partition_candidates(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack, worksheet, manifest, output = self.write_fixture(Path(temporary))
            value = json.loads(worksheet.read_text())
            value["groups"][0]["selected_source_identities"] = []
            worksheet.write_bytes(pretty(value))
            with self.assertRaisesRegex(finalize.FinalizeError, "must not be empty"):
                finalize.finalize(pack, worksheet, manifest, output)

    def test_candidate_metadata_drift_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack, worksheet, manifest, output = self.write_fixture(Path(temporary))
            value = json.loads(worksheet.read_text())
            value["groups"][0]["candidates"][0]["calls"]["call_sites"] = 2
            worksheet.write_bytes(pretty(value))
            with self.assertRaisesRegex(finalize.FinalizeError, "candidate metadata drift"):
                finalize.finalize(pack, worksheet, manifest, output)

    def test_output_refuses_overwrite(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            pack, worksheet, manifest, output = self.write_fixture(Path(temporary))
            output.write_text("existing")
            with self.assertRaisesRegex(finalize.FinalizeError, "must not already exist"):
                finalize.finalize(pack, worksheet, manifest, output)


if __name__ == "__main__":
    unittest.main()
