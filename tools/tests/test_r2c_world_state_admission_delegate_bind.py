from __future__ import annotations

import copy
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_admission_prepare as prepare
from tools import r2c_world_state_delegate_closure_source_review as closure
from tools import r2c_world_state_source_review_delegate_bind as bind_review
from tools import r2c_world_state_source_review_delegate_finalize as delegate_review
from tools import r2c_world_state_source_review_finalize as parent_review
from tools import r2c_world_state_source_review_pack as parent_packer


class ParentDelegateBindingTests(unittest.TestCase):
    @staticmethod
    def _write(path: Path, value: object) -> None:
        path.write_text(
            json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    @staticmethod
    def _candidate(candidate_id: str, type_name: str, signature: str) -> dict[str, object]:
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
            "atlas_observed_hazards": [],
            "atlas_classifications": [],
            "calls": {
                "call_sites": 0,
                "resolved_targets": [],
                "unresolved_call_sites": 0,
                "top_unresolved_callees": [],
            },
        }

    def _parent_manifest(self) -> dict[str, object]:
        provenance = bind_review._current_parent_provenance()
        return {
            "schema": 1,
            "kind": bind_review.parent_bundle.BUNDLE_MANIFEST_KIND,
            "commit_policy": bind_review.parent_bundle.BUNDLE_MANIFEST_COMMIT_POLICY,
            "contains_official_source_text": False,
            "production_admitted": False,
            "source_archive_sha256": parent_packer.EXPECTED_SOURCE_SHA256,
            **provenance,
            "discovery_sha256": "3" * 64,
            "review_pack_sha256": "4" * 64,
            "worksheet_sha256": "5" * 64,
            "unique_candidate_methods": 3,
            "unique_source_records": 3,
            "source_excerpt_bytes": 1,
        }

    def _parent_result(self) -> dict[str, object]:
        groups = []
        for index, group_id in enumerate(parent_review.FOCUS_GROUPS, start=1):
            candidate = self._candidate(
                f"DISC-NET-R2C-WORLD-{index:04d}",
                f"example.Parent{index}",
                "selected()",
            )
            groups.append({
                "group_id": group_id,
                "selected_sources": [candidate],
                "rejected_source_identities": [],
                "hazards_reviewed": [],
                "semantic_observations": [f"Reviewed parent observation for {group_id}."],
            })
        return {
            "schema": 1,
            "kind": parent_review.RESULT_KIND,
            "id": parent_review.RESULT_ID,
            "commit_policy": parent_review.RESULT_COMMIT_POLICY,
            "source_archive_sha256": parent_review.EXPECTED_SOURCE_SHA256,
            "discovery_sha256": "3" * 64,
            "review_pack_sha256": "4" * 64,
            "worksheet_sha256": "5" * 64,
            "contains_official_source_text": False,
            "all_groups_review_complete": True,
            "groups": groups,
            "production_admitted": False,
            "next_step": "Prepare semantic admission.",
        }

    def _delegate_candidate(
        self,
        candidate_id: str,
        type_name: str,
        signature: str,
        group_id: str,
        focus: str,
    ) -> dict[str, object]:
        candidate = self._candidate(candidate_id, type_name, signature)
        candidate["group_ids"] = [group_id]
        candidate["review_focus"] = [focus]
        return candidate

    def _delegate_result(self) -> dict[str, object]:
        groups = []
        for index, (group_id, parent_group_id) in enumerate(closure.EXPECTED_GROUPS, start=1):
            focus = f"Reviewed delegate focus for {group_id}."
            candidate = self._delegate_candidate(
                f"DISC-NET-R2C-WORLD-DELEGATE-{index:04d}",
                f"example.Delegate{index}",
                "selected()",
                group_id,
                focus,
            )
            groups.append({
                "group_id": group_id,
                "parent_group_id": parent_group_id,
                "review_focus": focus,
                "selected_sources": [candidate],
                "rejected_source_identities": [],
                "hazards_reviewed": [],
                "semantic_observations": [f"Reviewed delegate observation for {group_id}."],
            })
        provenance = bind_review._current_parent_provenance()
        return {
            "schema": 1,
            "kind": delegate_review.RESULT_KIND,
            "id": delegate_review.RESULT_ID,
            "commit_policy": delegate_review.RESULT_COMMIT_POLICY,
            "source_archive_sha256": parent_packer.EXPECTED_SOURCE_SHA256,
            "parent_review_id": parent_packer.DISCOVERY_REVIEW_ID,
            "contains_official_source_text": False,
            "all_groups_review_complete": True,
            "production_admitted": False,
            "plan_sha256": bind_review._sha256_file(closure.DEFAULT_PLAN),
            "parent_discovery_plan_sha256": provenance["plan_sha256"],
            "frontier_sha256": provenance["frontier_sha256"],
            "review_pack_sha256": "6" * 64,
            "worksheet_sha256": "7" * 64,
            "generated_worksheet_sha256": "8" * 64,
            "generated_worksheet_size": 123,
            "manifest_sha256": "9" * 64,
            "groups": groups,
            "next_step": "Bind into parent.",
        }

    def _bind(
        self,
        root: Path,
        *,
        parent: dict[str, object] | None = None,
        manifest: dict[str, object] | None = None,
        delegate: dict[str, object] | None = None,
    ) -> Path:
        parent_path = root / "parent-review-result.json"
        manifest_path = root / "bundle-manifest.json"
        delegate_path = root / "delegate-review-result.json"
        output = root / "bound-review-result.json"
        self._write(parent_path, parent or self._parent_result())
        self._write(manifest_path, manifest or self._parent_manifest())
        self._write(delegate_path, delegate or self._delegate_result())
        bind_review.bind(
            parent_review_result=parent_path,
            parent_bundle_manifest=manifest_path,
            delegate_review_result=delegate_path,
            output=output,
        )
        return output

    def test_binding_feeds_existing_admission_preparer_and_delegate_var_ids(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bound_path = self._bind(root)
            bound = json.loads(bound_path.read_text(encoding="utf-8"))

            groups = {group["group_id"]: group for group in bound["groups"]}
            biome_identities = {item["source_identity"] for item in groups["R2C-BIOMES"]["selected_sources"]}
            light_identities = {item["source_identity"] for item in groups["R2C-LIGHT"]["selected_sources"]}
            heightmap_ids = {item["candidate_id"] for item in groups["R2C-HEIGHTMAPS"]["selected_sources"]}
            self.assertIn("example.Delegate1#selected()", biome_identities)
            self.assertIn("example.Delegate2#selected()", light_identities)
            self.assertEqual(heightmap_ids, {"DISC-NET-R2C-WORLD-0002"})
            self.assertEqual(bound["delegate_binding"]["selected_delegate_sources"], 2)

            admission_path = root / "admission.json"
            prepare.prepare(bound_path, admission_path)
            admission = json.loads(admission_path.read_text(encoding="utf-8"))
            var_ids = {entry["var_id"] for entry in admission["selected_sources"]}
            self.assertIn("VAR-NET-R2C-WORLD-DELEGATE-0001", var_ids)
            self.assertIn("VAR-NET-R2C-WORLD-DELEGATE-0002", var_ids)

    def test_binding_rejects_stale_parent_frontier(self) -> None:
        manifest = self._parent_manifest()
        manifest["frontier_sha256"] = "f" * 64
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(bind_review.BindError, "parent bundle manifest provenance mismatch"):
                self._bind(Path(temporary), manifest=manifest)

    def test_binding_rejects_delegate_parent_plan_drift(self) -> None:
        delegate = self._delegate_result()
        delegate["parent_discovery_plan_sha256"] = "e" * 64
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(bind_review.BindError, "delegate review provenance mismatch"):
                self._bind(Path(temporary), delegate=delegate)

    def test_binding_rejects_delegate_plan_drift(self) -> None:
        delegate = self._delegate_result()
        delegate["plan_sha256"] = "d" * 64
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(bind_review.BindError, "delegate review provenance mismatch"):
                self._bind(Path(temporary), delegate=delegate)

    def test_binding_rejects_misparented_delegate_group(self) -> None:
        delegate = self._delegate_result()
        delegate["groups"][0]["parent_group_id"] = "R2C-HEIGHTMAPS"  # type: ignore[index]
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(bind_review.BindError, "mis-parented"):
                self._bind(Path(temporary), delegate=delegate)

    def test_binding_rejects_delegate_identity_collision_with_parent(self) -> None:
        parent = self._parent_result()
        delegate = self._delegate_result()
        parent_candidate = parent["groups"][0]["selected_sources"][0]  # type: ignore[index]
        delegate_candidate = delegate["groups"][0]["selected_sources"][0]  # type: ignore[index]
        delegate_candidate["source_identity"] = parent_candidate["source_identity"]
        delegate_candidate["source"] = copy.deepcopy(parent_candidate["source"])
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(bind_review.BindError, "collides with parent-selected source"):
                self._bind(Path(temporary), parent=parent, delegate=delegate)

    def test_binding_rejects_incomplete_delegate_review(self) -> None:
        delegate = self._delegate_result()
        delegate["all_groups_review_complete"] = False
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(bind_review.BindError, "delegate review provenance mismatch"):
                self._bind(Path(temporary), delegate=delegate)


if __name__ == "__main__":
    unittest.main()
