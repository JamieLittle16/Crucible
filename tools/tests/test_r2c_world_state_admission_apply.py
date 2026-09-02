from __future__ import annotations

import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r2c_world_state_admission_apply as apply_semantics
from tools import r2c_world_state_admission_prepare as prepare


class SemanticAdmissionApplyTests(unittest.TestCase):
    @staticmethod
    def _bytes(value: object) -> bytes:
        return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")

    @classmethod
    def _write(cls, path: Path, value: object) -> str:
        raw = cls._bytes(value)
        path.write_bytes(raw)
        return hashlib.sha256(raw).hexdigest()

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

    def _worksheet(self) -> dict[str, object]:
        selected_sources: list[dict[str, object]] = []
        groups: list[dict[str, object]] = []
        for group_index, group_id in enumerate(prepare.review.FOCUS_GROUPS, start=1):
            identities: list[str] = []
            for source_index in range(2):
                if group_id == "R2C-LIGHT" and source_index == 1:
                    candidate_id = "DISC-NET-R2C-WORLD-DELEGATE-0007"
                else:
                    candidate_id = f"DISC-NET-R2C-WORLD-{group_index * 10 + source_index:04d}"
                candidate = self._candidate(
                    candidate_id,
                    f"example.{group_id.removeprefix('R2C-').title()}{source_index}",
                    "selected()",
                )
                identity = str(candidate["source_identity"])
                identities.append(identity)
                selected_sources.append(
                    {
                        "var_id": prepare._var_id(candidate_id),
                        "source_identity": identity,
                        "candidate": candidate,
                    }
                )
            groups.append(
                {
                    "group_id": group_id,
                    "selected_source_identities": identities,
                    "semantic_rules": [],
                    "admission_complete": False,
                }
            )
        return {
            "schema": 1,
            "kind": prepare.KIND,
            "id": prepare.ID,
            "commit_policy": prepare.COMMIT_POLICY,
            "review_result_sha256": "a" * 64,
            "source_archive_sha256": prepare.review.EXPECTED_SOURCE_SHA256,
            "contains_official_source_text": False,
            "semantic_rule_contract": {
                "id_prefix": prepare.SEM_PREFIX,
                "required_fields": ["id", "statement", "source_identities"],
                "source_support_must_be_selected": True,
                "automatic_semantic_inference_forbidden": True,
            },
            "selected_sources": selected_sources,
            "groups": groups,
            "all_groups_admission_complete": False,
            "production_admitted": False,
        }

    def _decisions(self, worksheet: dict[str, object], worksheet_sha: str) -> dict[str, object]:
        by_identity = {
            entry["source_identity"]: entry["candidate"]["candidate_id"]  # type: ignore[index]
            for entry in worksheet["selected_sources"]  # type: ignore[index]
        }
        groups = []
        for index, group in enumerate(worksheet["groups"], start=1):  # type: ignore[index]
            group_id = str(group["group_id"])
            candidate_ids = [
                str(by_identity[identity]) for identity in group["selected_source_identities"]
            ]
            groups.append(
                {
                    "group_id": group_id,
                    "admission_complete": True,
                    "semantic_rules": [
                        {
                            "id": f"SEM-NET-R2C-WORLD-{index:03d}",
                            "statement": f"Reviewed semantic rule for {group_id}.",
                            "source_candidate_ids": candidate_ids,
                        }
                    ],
                }
            )
        return {
            "schema": 1,
            "kind": apply_semantics.DECISION_KIND,
            "id": apply_semantics.DECISION_ID,
            "commit_policy": apply_semantics.DECISION_COMMIT_POLICY,
            "contains_official_source_text": False,
            "production_admitted": False,
            "source_archive_sha256": prepare.review.EXPECTED_SOURCE_SHA256,
            "prepared_worksheet_sha256": worksheet_sha,
            "review_result_sha256": worksheet["review_result_sha256"],
            "automatic_semantic_inference_forbidden": True,
            "groups": groups,
        }

    def _stage(
        self,
        root: Path,
        *,
        mutate_worksheet=None,
        mutate_decisions=None,
    ) -> tuple[Path, Path, Path]:
        worksheet = self._worksheet()
        if mutate_worksheet is not None:
            mutate_worksheet(worksheet)
        worksheet_path = root / "worksheet.json"
        worksheet_sha = self._write(worksheet_path, worksheet)
        decisions = self._decisions(worksheet, worksheet_sha)
        if mutate_decisions is not None:
            mutate_decisions(decisions)
        decisions_path = root / "decisions.json"
        self._write(decisions_path, decisions)
        return worksheet_path, decisions_path, root / "completed.json"

    def test_maps_candidate_ids_to_exact_selected_source_identities(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions, output = self._stage(Path(temporary))
            summary = apply_semantics.apply(
                worksheet=worksheet,
                decisions=decisions,
                output=output,
            )
            completed = json.loads(output.read_text(encoding="utf-8"))
            groups = completed["groups"]

            self.assertTrue(completed["all_groups_admission_complete"])
            self.assertEqual(summary["groups"], 3)
            self.assertEqual(summary["semantic_rules"], 3)
            self.assertEqual(summary["selected_sources"], 6)
            self.assertTrue(all(group["admission_complete"] for group in groups))
            self.assertNotIn("source_candidate_ids", json.dumps(completed))
            self.assertIn(
                "example.Light1#selected()",
                groups[2]["semantic_rules"][0]["source_identities"],
            )

    def test_rejects_stale_prepared_worksheet_digest(self) -> None:
        def mutate(decisions: dict[str, object]) -> None:
            decisions["prepared_worksheet_sha256"] = "f" * 64

        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions, output = self._stage(
                Path(temporary), mutate_decisions=mutate
            )
            with self.assertRaisesRegex(apply_semantics.ApplyError, "semantic decision provenance mismatch"):
                apply_semantics.apply(worksheet=worksheet, decisions=decisions, output=output)

    def test_rejects_unknown_selected_candidate_id(self) -> None:
        def mutate(decisions: dict[str, object]) -> None:
            decisions["groups"][0]["semantic_rules"][0]["source_candidate_ids"][0] = (  # type: ignore[index]
                "DISC-NET-R2C-WORLD-9999"
            )

        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions, output = self._stage(
                Path(temporary), mutate_decisions=mutate
            )
            with self.assertRaisesRegex(apply_semantics.ApplyError, "cites unknown selected candidate id"):
                apply_semantics.apply(worksheet=worksheet, decisions=decisions, output=output)

    def test_rejects_cross_group_candidate_support(self) -> None:
        worksheet = self._worksheet()
        foreign = worksheet["selected_sources"][2]["candidate"]["candidate_id"]  # type: ignore[index]

        def mutate(decisions: dict[str, object]) -> None:
            decisions["groups"][0]["semantic_rules"][0]["source_candidate_ids"][0] = foreign  # type: ignore[index]

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            worksheet_path = root / "worksheet.json"
            worksheet_sha = self._write(worksheet_path, worksheet)
            decisions = self._decisions(worksheet, worksheet_sha)
            mutate(decisions)
            decisions_path = root / "decisions.json"
            self._write(decisions_path, decisions)
            with self.assertRaisesRegex(apply_semantics.ApplyError, "cites candidate outside R2C-BIOMES"):
                apply_semantics.apply(
                    worksheet=worksheet_path,
                    decisions=decisions_path,
                    output=root / "completed.json",
                )

    def test_rejects_uncovered_selected_source(self) -> None:
        def mutate(decisions: dict[str, object]) -> None:
            support = decisions["groups"][1]["semantic_rules"][0]["source_candidate_ids"]  # type: ignore[index]
            del support[-1]

        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions, output = self._stage(
                Path(temporary), mutate_decisions=mutate
            )
            with self.assertRaisesRegex(apply_semantics.ApplyError, "must support at least one semantic rule"):
                apply_semantics.apply(worksheet=worksheet, decisions=decisions, output=output)

    def test_rejects_duplicate_rule_ids_across_groups(self) -> None:
        def mutate(decisions: dict[str, object]) -> None:
            decisions["groups"][1]["semantic_rules"][0]["id"] = (  # type: ignore[index]
                decisions["groups"][0]["semantic_rules"][0]["id"]  # type: ignore[index]
            )

        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions, output = self._stage(
                Path(temporary), mutate_decisions=mutate
            )
            with self.assertRaisesRegex(apply_semantics.ApplyError, "duplicate semantic rule id"):
                apply_semantics.apply(worksheet=worksheet, decisions=decisions, output=output)

    def test_rejects_decisions_that_enable_automatic_inference(self) -> None:
        def mutate(decisions: dict[str, object]) -> None:
            decisions["automatic_semantic_inference_forbidden"] = False

        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions, output = self._stage(
                Path(temporary), mutate_decisions=mutate
            )
            with self.assertRaisesRegex(apply_semantics.ApplyError, "semantic decision provenance mismatch"):
                apply_semantics.apply(worksheet=worksheet, decisions=decisions, output=output)

    def test_rejects_non_pristine_prepared_worksheet(self) -> None:
        def mutate(worksheet: dict[str, object]) -> None:
            worksheet["groups"][0]["semantic_rules"] = [  # type: ignore[index]
                {"id": "SEM-NET-R2C-WORLD-OLD", "statement": "old", "source_identities": ["x"]}
            ]

        with tempfile.TemporaryDirectory() as temporary:
            worksheet, decisions, output = self._stage(
                Path(temporary), mutate_worksheet=mutate
            )
            with self.assertRaisesRegex(apply_semantics.ApplyError, "already contains semantic rules"):
                apply_semantics.apply(worksheet=worksheet, decisions=decisions, output=output)


if __name__ == "__main__":
    unittest.main()
