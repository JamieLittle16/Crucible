from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_admission_materialize as materialize
from tools import r2c_world_state_admission_promote as promote
from tools import r2c_world_state_source_review_finalize as review


def pretty(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


class PromotionFixture:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.staging = root / "staging"
        self.repo = root / "repo"
        self.report = root / "gate-report.json"
        self.var_id = "VAR-NET-R2C-WORLD-BIOME-001"
        self.source_identity = "net.minecraft.test.WorldState#sample(final int value)"
        self.record_path = f"records/{self.var_id}.json"
        self.record = {
            "schema": 1,
            "id": self.var_id,
            "status": "VAR_REVIEWED",
            "source": {
                "type": "net.minecraft.test.WorldState",
                "signature": "sample(final int value)",
                "fingerprint_algorithm": promote.EXPECTED_FINGERPRINT,
                "normalized_sha256": "1" * 64,
                "body_sha256": "2" * 64,
            },
            "classifications": ["CLIENT_OBSERVABLE"],
            "hazards_reviewed": ["CLIENT_OBSERVABLE"],
            "semantic_rules": ["SEM-NET-R2C-WORLD-001"],
            "evidence": ["REVIEW", "PREPARE"],
            "notes": ["source-free test record"],
        }
        self.record_raw = pretty(self.record)
        self.gate = {
            "schema": 1,
            "id": materialize.GATE_ID,
            "frontier": materialize.FRONTIER,
            "minimum_status": "VAR_REVIEWED",
            "require_semantic_rules": True,
            "require_hazards_reviewed": True,
            "methods": [{"query": self.source_identity, "var_id": self.var_id}],
        }
        self.gate_raw = pretty(self.gate)
        self.semantics_raw = (
            "# R2C World-State Semantics — Minecraft Java 26.2\n\n"
            "### SEM-NET-R2C-WORLD-001\n\nSource-free test semantic rule.\n"
        ).encode()
        self._write_staging()
        self.report_value = self._report_value()
        self.report.write_bytes(pretty(self.report_value))
        (self.repo / "vanilla").mkdir(parents=True)
        (self.repo / "tools").mkdir(parents=True)

    def _write_staging(self) -> None:
        (self.staging / "records").mkdir(parents=True)
        (self.staging / "semantics").mkdir()
        (self.staging / self.record_path).write_bytes(self.record_raw)
        (self.staging / "gate.json").write_bytes(self.gate_raw)
        (self.staging / "semantics" / materialize.SEMANTICS_FILE).write_bytes(
            self.semantics_raw
        )
        files = []
        for relative, raw in (
            (self.record_path, self.record_raw),
            (f"semantics/{materialize.SEMANTICS_FILE}", self.semantics_raw),
            ("gate.json", self.gate_raw),
        ):
            files.append({"path": relative, "size": len(raw), "sha256": digest(raw)})
        manifest = {
            "schema": 1,
            "kind": materialize.KIND,
            "id": materialize.ID,
            "commit_policy": materialize.COMMIT_POLICY,
            "review_result_sha256": "3" * 64,
            "admission_worksheet_sha256": "4" * 64,
            "source_archive_sha256": review.EXPECTED_SOURCE_SHA256,
            "contains_official_source_text": False,
            "var_records": 1,
            "semantic_rules": 1,
            "gate_id": materialize.GATE_ID,
            "independent_gate_required": True,
            "production_admitted": False,
            "files": files,
            "next_step": "independent source gate",
        }
        self.manifest_raw = pretty(manifest)
        (self.staging / "manifest.json").write_bytes(self.manifest_raw)

    def _report_value(self) -> dict[str, object]:
        return {
            "schema": 1,
            "gate_id": materialize.GATE_ID,
            "admitted": True,
            "gate_path": str(self.staging / "gate.json"),
            "gate_sha256": digest(self.gate_raw),
            "minimum_status": "VAR_REVIEWED",
            "materialization_id": materialize.ID,
            "materialization_manifest_sha256": digest(self.manifest_raw),
            "source_free_bundle_bound": True,
            "source": {
                "minecraft_version": "26.2",
                "protocol_version": "776",
                "world_version": "4903",
                "archive_sha256": review.EXPECTED_SOURCE_SHA256,
                "fingerprint_algorithm": promote.EXPECTED_FINGERPRINT,
                "atlas_version": "0.1.2",
                "schema_version": "1",
            },
            "frontier": {
                "name": materialize.FRONTIER,
                "config_path": "vanilla/frontiers/r2c-world-projection.json",
                "config_sha256": "5" * 64,
                "root_methods": 1,
                "reachable_methods": 1,
            },
            "required_methods": [
                {
                    "var_id": self.var_id,
                    "record_path": str(self.staging / self.record_path),
                    "record_sha256": digest(self.record_raw),
                    "source": self.source_identity,
                    "status": "VAR_REVIEWED",
                    "normalized_sha256": "1" * 64,
                    "body_sha256": "2" * 64,
                    "semantic_rules": ["SEM-NET-R2C-WORLD-001"],
                    "observed_hazards": ["CLIENT_OBSERVABLE"],
                    "reviewed_hazards": ["CLIENT_OBSERVABLE"],
                }
            ],
            "closure_diagnostics": {
                "review_status": {"VAR_REVIEWED": 1},
                "unresolved_call_sites": 0,
                "note": "diagnostic only",
            },
            "failures": [],
        }

    def rewrite_report(self) -> None:
        self.report.write_bytes(pretty(self.report_value))


class WorldStateAdmissionPromotionTests(unittest.TestCase):
    def test_admitted_bundle_promotes_exact_staged_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            summary = promote.promote(fixture.staging, fixture.report, fixture.repo)

            record_dest = fixture.repo / promote.RECORD_ROOT / f"{fixture.var_id}.json"
            semantics_dest = fixture.repo / promote.SEMANTICS_PATH
            gate_dest = fixture.repo / promote.GATE_PATH
            report_dest = fixture.repo / promote.REPORT_PATH
            manifest_dest = fixture.repo / promote.MANIFEST_PATH

            self.assertEqual(record_dest.read_bytes(), fixture.record_raw)
            self.assertEqual(semantics_dest.read_bytes(), fixture.semantics_raw)
            self.assertEqual(gate_dest.read_bytes(), fixture.gate_raw)
            self.assertEqual(report_dest.read_bytes(), fixture.report.read_bytes())
            manifest = json.loads(manifest_dest.read_text())
            self.assertTrue(manifest["source_admitted"])
            self.assertTrue(manifest["production_implementation_authorized"])
            self.assertFalse(manifest["runtime_behavior_implemented"])
            self.assertFalse(manifest["contains_official_source_text"])
            self.assertEqual(manifest["var_records"], 1)
            self.assertEqual(
                manifest["materialization_manifest_sha256"], digest(fixture.manifest_raw)
            )
            self.assertEqual(summary["manifest_sha256"], digest(manifest_dest.read_bytes()))

    def test_non_admitted_gate_report_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            fixture.report_value["admitted"] = False
            fixture.report_value["failures"] = ["evidence failed"]
            fixture.rewrite_report()
            with self.assertRaisesRegex(promote.PromoteError, "not admitted cleanly"):
                promote.promote(fixture.staging, fixture.report, fixture.repo)

    def test_gate_digest_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            fixture.report_value["gate_sha256"] = "a" * 64
            fixture.rewrite_report()
            with self.assertRaisesRegex(promote.PromoteError, "exact staged gate"):
                promote.promote(fixture.staging, fixture.report, fixture.repo)

    def test_materialization_manifest_digest_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            fixture.report_value["materialization_manifest_sha256"] = "a" * 64
            fixture.rewrite_report()
            with self.assertRaisesRegex(promote.PromoteError, "exact materialization manifest"):
                promote.promote(fixture.staging, fixture.report, fixture.repo)

    def test_unbound_source_free_bundle_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            fixture.report_value["source_free_bundle_bound"] = False
            fixture.rewrite_report()
            with self.assertRaisesRegex(promote.PromoteError, "source-free staging bundle"):
                promote.promote(fixture.staging, fixture.report, fixture.repo)

    def test_record_digest_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            required = fixture.report_value["required_methods"]
            assert isinstance(required, list) and isinstance(required[0], dict)
            required[0]["record_sha256"] = "b" * 64
            fixture.rewrite_report()
            with self.assertRaisesRegex(promote.PromoteError, "record digest mismatch"):
                promote.promote(fixture.staging, fixture.report, fixture.repo)

    def test_source_pin_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            source = fixture.report_value["source"]
            assert isinstance(source, dict)
            source["archive_sha256"] = "c" * 64
            fixture.rewrite_report()
            with self.assertRaisesRegex(promote.PromoteError, "source.archive_sha256 mismatch"):
                promote.promote(fixture.staging, fixture.report, fixture.repo)

    def test_unmanifested_staging_file_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            (fixture.staging / "unexpected.txt").write_text("not admitted")
            with self.assertRaisesRegex(promote.PromoteError, "file inventory mismatch"):
                promote.promote(fixture.staging, fixture.report, fixture.repo)

    def test_destination_collision_is_preflighted_before_any_write(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            gate = fixture.repo / promote.GATE_PATH
            gate.parent.mkdir(parents=True)
            gate.write_text("existing")
            with self.assertRaisesRegex(promote.PromoteError, "refuses to overwrite"):
                promote.promote(fixture.staging, fixture.report, fixture.repo)
            record = fixture.repo / promote.RECORD_ROOT / f"{fixture.var_id}.json"
            self.assertFalse(record.exists())
            self.assertFalse((fixture.repo / promote.REPORT_PATH).exists())
            self.assertEqual(gate.read_text(), "existing")

    def test_report_must_admit_exact_staged_var_set(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            fixture.report_value["required_methods"] = []
            fixture.rewrite_report()
            with self.assertRaisesRegex(promote.PromoteError, "exactly the staged VAR set"):
                promote.promote(fixture.staging, fixture.report, fixture.repo)

    def test_staged_var_filename_must_match_record_id(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            fixture = PromotionFixture(Path(tmp))
            old_path = fixture.staging / fixture.record_path
            wrong_relative = "records/VAR-NET-R2C-WORLD-WRONG-001.json"
            wrong_path = fixture.staging / wrong_relative
            old_path.rename(wrong_path)

            manifest = json.loads(fixture.manifest_raw)
            for entry in manifest["files"]:
                if entry["path"] == fixture.record_path:
                    entry["path"] = wrong_relative
            fixture.manifest_raw = pretty(manifest)
            (fixture.staging / "manifest.json").write_bytes(fixture.manifest_raw)
            fixture.report_value["materialization_manifest_sha256"] = digest(
                fixture.manifest_raw
            )
            fixture.rewrite_report()

            with self.assertRaisesRegex(promote.PromoteError, "filename does not match record id"):
                promote.promote(fixture.staging, fixture.report, fixture.repo)


if __name__ == "__main__":
    unittest.main()
