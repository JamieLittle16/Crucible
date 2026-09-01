from __future__ import annotations

import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_source_review_finalize as finalize
from tools import r2c_world_state_source_review_pack as packer
from tools import r2c_world_state_source_review_parent_apply as apply_review


class ParentReviewApplyTests(unittest.TestCase):
    @staticmethod
    def _bytes(value: object) -> bytes:
        return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")

    @classmethod
    def _write(cls, path: Path, value: object) -> str:
        raw = cls._bytes(value)
        path.write_bytes(raw)
        return hashlib.sha256(raw).hexdigest()

    @staticmethod
    def _candidate(
        candidate_id: str,
        type_name: str,
        signature: str,
        *,
        hazards: list[str] | None = None,
    ) -> dict[str, object]:
        return {
            "candidate_id": candidate_id,
            "source_identity": f"{type_name}#{signature}",
            "source": {
                "type": type_name,
                "signature": signature,
                "fingerprint_algorithm": "java-token-v2-literal-sensitive",
                "normalized_sha256": "1" * 64,
                "body_sha256": "2" * 64,
            },
            "source_location": {
                "path": f"src/{type_name.replace('.', '/')}.java",
                "start_line": 10,
                "end_line": 12,
            },
            "atlas_observed_hazards": hazards or [],
            "atlas_classifications": [],
            "calls": {
                "call_sites": 0,
                "resolved_targets": [],
                "unresolved_call_sites": 0,
                "top_unresolved_callees": [],
            },
        }

    def _worksheet(self) -> dict[str, object]:
        shared = self._candidate(
            "DISC-NET-R2C-WORLD-0099",
            "example.Shared",
            "selected()",
        )
        groups = []
        for index, group_id in enumerate(finalize.FOCUS_GROUPS, start=1):
            selected = self._candidate(
                f"DISC-NET-R2C-WORLD-{index:04d}",
                f"example.Selected{index}",
                "selected()",
                hazards=["CLIENT_OBSERVABLE"] if group_id == "R2C-LIGHT" else [],
            )
            rejected = self._candidate(
                f"DISC-NET-R2C-WORLD-{index + 10:04d}",
                f"example.Rejected{index}",
                "rejected()",
            )
            candidates = [selected, rejected]
            if group_id in {"R2C-BIOMES", "R2C-HEIGHTMAPS"}:
                candidates.append(copy.deepcopy(shared))
            groups.append({
                "group_id": group_id,
                "review_focus": f"Review {group_id}.",
                "candidates": candidates,
                "source_inspected": False,
                "selected_source_identities": [],
                "rejected_source_identities": [],
                "hazards_reviewed": [],
                "followup_dependencies": [],
                "semantic_observations": [],
                "review_complete": False,
            })
        return {
            "schema": 1,
            "kind": packer.WORKSHEET_KIND,
            "review_id": packer.DISCOVERY_REVIEW_ID,
            "commit_policy": packer.WORKSHEET_COMMIT_POLICY,
            "contains_official_source_text": False,
            "source_archive_sha256": packer.EXPECTED_SOURCE_SHA256,
            "discovery_sha256": "3" * 64,
            "review_pack_sha256": "4" * 64,
            "groups": groups,
            "production_admitted": False,
        }

    def _manifest(self, worksheet_sha: str) -> dict[str, object]:
        provenance = apply_review._current_provenance()
        return {
            "schema": 1,
            "kind": apply_review.bundle.BUNDLE_MANIFEST_KIND,
            "commit_policy": apply_review.bundle.BUNDLE_MANIFEST_COMMIT_POLICY,
            "contains_official_source_text": False,
            "production_admitted": False,
            "source_archive_sha256": packer.EXPECTED_SOURCE_SHA256,
            **provenance,
            "discovery_sha256": "3" * 64,
            "review_pack_sha256": "4" * 64,
            "worksheet_sha256": worksheet_sha,
            "unique_candidate_methods": 7,
            "unique_source_records": 7,
            "source_excerpt_bytes": 1,
        }

    def _decisions(
        self,
        *,
        worksheet_sha: str,
        manifest_sha: str,
        manifest: dict[str, object],
    ) -> dict[str, object]:
        groups = []
        for index, group_id in enumerate(finalize.FOCUS_GROUPS, start=1):
            selected = [f"DISC-NET-R2C-WORLD-{index:04d}"]
            if group_id == "R2C-BIOMES":
                selected.append("DISC-NET-R2C-WORLD-0099")
            selected.sort()
            groups.append({
                "group_id": group_id,
                "source_inspected": True,
                "selected_candidate_ids": selected,
                "hazards_reviewed": ["CLIENT_OBSERVABLE"] if group_id == "R2C-LIGHT" else [],
                "followup_dependencies": [],
                "semantic_observations": [f"Reviewed semantic observation for {group_id}."],
            })
        return {
            "schema": 1,
            "kind": apply_review.DECISION_KIND,
            "review_id": packer.DISCOVERY_REVIEW_ID,
            "commit_policy": apply_review.DECISION_COMMIT_POLICY,
            "contains_official_source_text": False,
            "production_admitted": False,
            "source_archive_sha256": packer.EXPECTED_SOURCE_SHA256,
            "plan_sha256": manifest["plan_sha256"],
            "frontier_sha256": manifest["frontier_sha256"],
            "bundle_manifest_sha256": manifest_sha,
            "generated_worksheet_sha256": worksheet_sha,
            "discovery_sha256": manifest["discovery_sha256"],
            "review_pack_sha256": manifest["review_pack_sha256"],
            "automatic_source_selection_forbidden": True,
            "unselected_candidate_policy": apply_review.UNSELECTED_POLICY,
            "groups": groups,
        }

    def _stage(
        self,
        root: Path,
        *,
        worksheet: dict[str, object] | None = None,
        mutate_manifest=None,
        mutate_decisions=None,
    ) -> tuple[Path, Path, Path, Path]:
        worksheet_value = worksheet or self._worksheet()
        worksheet_path = root / "worksheet.json"
        worksheet_sha = self._write(worksheet_path, worksheet_value)
        manifest = self._manifest(worksheet_sha)
        if mutate_manifest is not None:
            mutate_manifest(manifest)
        manifest_path = root / "bundle-manifest.json"
        manifest_sha = self._write(manifest_path, manifest)
        decisions = self._decisions(
            worksheet_sha=worksheet_sha,
            manifest_sha=manifest_sha,
            manifest=manifest,
        )
        if mutate_decisions is not None:
            mutate_decisions(decisions)
        decisions_path = root / "decisions.json"
        self._write(decisions_path, decisions)
        return worksheet_path, manifest_path, decisions_path, root / "completed.json"

    def test_applies_explicit_decisions_and_rejects_every_omitted_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            worksheet, manifest, decisions, output = self._stage(root)
            summary = apply_review.apply(
                worksheet=worksheet,
                bundle_manifest=manifest,
                decisions=decisions,
                output=output,
            )
            completed = json.loads(output.read_text(encoding="utf-8"))
            groups = {group["group_id"]: group for group in completed["groups"]}

            self.assertEqual(summary["selected_sources"], 4)
            self.assertTrue(all(group["source_inspected"] for group in groups.values()))
            self.assertTrue(all(group["review_complete"] for group in groups.values()))
            self.assertIn(
                "example.Shared#selected()",
                groups["R2C-BIOMES"]["selected_source_identities"],
            )
            self.assertIn(
                "example.Shared#selected()",
                groups["R2C-HEIGHTMAPS"]["rejected_source_identities"],
            )
            self.assertEqual(groups["R2C-LIGHT"]["hazards_reviewed"], ["CLIENT_OBSERVABLE"])

    def test_rejects_stale_frontier_manifest(self) -> None:
        def mutate(manifest: dict[str, object]) -> None:
            manifest["frontier_sha256"] = "f" * 64

        with tempfile.TemporaryDirectory() as temporary:
            worksheet, manifest, decisions, output = self._stage(
                Path(temporary), mutate_manifest=mutate
            )
            with self.assertRaisesRegex(apply_review.ApplyError, "parent bundle manifest provenance mismatch"):
                apply_review.apply(
                    worksheet=worksheet,
                    bundle_manifest=manifest,
                    decisions=decisions,
                    output=output,
                )

    def test_rejects_decisions_bound_to_wrong_worksheet(self) -> None:
        def mutate(decisions: dict[str, object]) -> None:
            decisions["generated_worksheet_sha256"] = "e" * 64

        with tempfile.TemporaryDirectory() as temporary:
            worksheet, manifest, decisions, output = self._stage(
                Path(temporary), mutate_decisions=mutate
            )
            with self.assertRaisesRegex(apply_review.ApplyError, "parent review decision provenance mismatch"):
                apply_review.apply(
                    worksheet=worksheet,
                    bundle_manifest=manifest,
                    decisions=decisions,
                    output=output,
                )

    def test_rejects_unknown_selected_candidate(self) -> None:
        def mutate(decisions: dict[str, object]) -> None:
            decisions["groups"][0]["selected_candidate_ids"] = ["DISC-NET-R2C-WORLD-9999"]  # type: ignore[index]

        with tempfile.TemporaryDirectory() as temporary:
            worksheet, manifest, decisions, output = self._stage(
                Path(temporary), mutate_decisions=mutate
            )
            with self.assertRaisesRegex(apply_review.ApplyError, "selected unknown candidate ids"):
                apply_review.apply(
                    worksheet=worksheet,
                    bundle_manifest=manifest,
                    decisions=decisions,
                    output=output,
                )

    def test_rejects_incomplete_hazard_review(self) -> None:
        def mutate(decisions: dict[str, object]) -> None:
            decisions["groups"][2]["hazards_reviewed"] = []  # type: ignore[index]

        with tempfile.TemporaryDirectory() as temporary:
            worksheet, manifest, decisions, output = self._stage(
                Path(temporary), mutate_decisions=mutate
            )
            with self.assertRaisesRegex(apply_review.ApplyError, "hazards_reviewed must exactly match"):
                apply_review.apply(
                    worksheet=worksheet,
                    bundle_manifest=manifest,
                    decisions=decisions,
                    output=output,
                )

    def test_rejects_already_reviewed_worksheet(self) -> None:
        worksheet = self._worksheet()
        worksheet["groups"][0]["source_inspected"] = True  # type: ignore[index]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            worksheet_path = root / "worksheet.json"
            worksheet_sha = self._write(worksheet_path, worksheet)
            manifest = self._manifest(worksheet_sha)
            manifest_path = root / "bundle-manifest.json"
            manifest_sha = self._write(manifest_path, manifest)
            decisions = self._decisions(
                worksheet_sha=worksheet_sha,
                manifest_sha=manifest_sha,
                manifest=manifest,
            )
            decisions_path = root / "decisions.json"
            self._write(decisions_path, decisions)
            with self.assertRaisesRegex(apply_review.ApplyError, "worksheet has already been reviewed"):
                apply_review.apply(
                    worksheet=worksheet_path,
                    bundle_manifest=manifest_path,
                    decisions=decisions_path,
                    output=root / "completed.json",
                )

    def test_rejects_inconsistent_shared_candidate_metadata(self) -> None:
        worksheet = self._worksheet()
        shared = worksheet["groups"][1]["candidates"][2]  # type: ignore[index]
        shared["source"]["body_sha256"] = "a" * 64  # type: ignore[index]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            worksheet_path = root / "worksheet.json"
            worksheet_sha = self._write(worksheet_path, worksheet)
            manifest = self._manifest(worksheet_sha)
            manifest_path = root / "bundle-manifest.json"
            manifest_sha = self._write(manifest_path, manifest)
            decisions = self._decisions(
                worksheet_sha=worksheet_sha,
                manifest_sha=manifest_sha,
                manifest=manifest,
            )
            decisions_path = root / "decisions.json"
            self._write(decisions_path, decisions)
            with self.assertRaisesRegex(apply_review.ApplyError, "shared parent candidate metadata differs"):
                apply_review.apply(
                    worksheet=worksheet_path,
                    bundle_manifest=manifest_path,
                    decisions=decisions_path,
                    output=root / "completed.json",
                )


if __name__ == "__main__":
    unittest.main()
