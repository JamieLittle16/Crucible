from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from tools import r1b_configuration_bundle_review as review
from tools import r1b_configuration_source_probe as source_probe


OFFICIAL_SOURCE_MARKER = "OFFICIAL_SOURCE_TEXT_MUST_NOT_ESCAPE"


def digest(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def write_lock(path: Path, source_sha: str) -> None:
    path.write_text(
        f'''schema = 1
minecraft = "26.2"
protocol = 776
data_version = 4903

[source]
archive_sha256 = "{source_sha}"

[atlas]
fingerprint_algorithm = "java-token-v2-literal-sensitive"
''',
        encoding="utf-8",
    )


def write_frontier(path: Path, roots: list[str]) -> None:
    path.write_text(
        json.dumps({"schema": 1, "name": "r1b-configuration-selected", "root_queries": roots}),
        encoding="utf-8",
    )


def synthetic_bundle(frontier: Path, source_sha: str, roots: list[str]) -> dict[str, object]:
    candidates: list[dict[str, object]] = []
    for index, (var_id, query) in enumerate(source_probe.CANDIDATES):
        template = {
            "schema": 1,
            "id": var_id,
            "status": "INDEXED",
            "source": {
                "type": f"net.minecraft.synthetic.Type{index}",
                "signature": f"method{index}()",
                "fingerprint_algorithm": "java-token-v2-literal-sensitive",
                "normalized_sha256": digest(f"normalized-{index}"),
                "body_sha256": digest(f"body-{index}"),
            },
            "classifications": ["CLIENT_OBSERVABLE", "PROTOCOL"],
            "hazards_reviewed": [],
            "semantic_rules": [],
            "evidence": [],
            "notes": [],
            "atlas_observed_hazards": ["CODEC"] if index % 2 == 0 else [],
        }
        candidates.append(
            {
                "var_id": var_id,
                "query": query,
                "match_count": 1,
                "record_template": template,
            }
        )
    return {
        "schema": 1,
        "kind": source_probe.BUNDLE_KIND,
        "commit_policy": "EPHEMERAL_DO_NOT_COMMIT",
        "contains_official_source_text": True,
        "source_archive_sha256": source_sha,
        "frontier": str(frontier),
        "frontier_roots": [
            {"query": root, "matches": [f"net.minecraft.synthetic.Root{index}#method()"]}
            for index, root in enumerate(roots)
        ],
        "var_candidates": candidates,
        "play_bootstrap_source": {
            "path": source_probe.PLAYER_LIST_PATH,
            "owner": "PlayerList",
            "method": "placeNewPlayer",
            "source": f"public void placeNewPlayer() {{ /* {OFFICIAL_SOURCE_MARKER} */ }}",
        },
        "summary": {
            "roots_ok": len(roots),
            "record_templates_emitted": len(source_probe.CANDIDATES),
            "record_templates_needing_refinement": 0,
        },
    }


def write_bundle(path: Path, bundle: dict[str, object]) -> None:
    path.write_text(json.dumps(bundle, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def file_map(root: Path) -> dict[str, bytes]:
    return {
        str(path.relative_to(root)): path.read_bytes()
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }


class R1BConfigurationBundleReviewTests(unittest.TestCase):
    def fixture(self, root: Path) -> tuple[Path, Path, Path, dict[str, object]]:
        source_sha = digest("pinned-source")
        lock = root / "vanilla.lock.toml"
        write_lock(lock, source_sha)
        frontier = root / "r1b-configuration-selected.json"
        roots = ["Root#configuration()", "Root#spawn()"]
        write_frontier(frontier, roots)
        bundle = synthetic_bundle(frontier, source_sha, roots)
        bundle_path = root / "bundle.json"
        write_bundle(bundle_path, bundle)
        return lock, frontier, bundle_path, bundle

    def test_materialized_pack_is_source_text_free_and_review_blocked(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock, frontier, bundle_path, _bundle = self.fixture(root)
            output = root / "review-pack"

            manifest = review.materialize_review_pack(
                bundle_path=bundle_path,
                output_dir=output,
                lock_path=lock,
                frontier_path=frontier,
            )

            self.assertFalse(manifest["contains_official_source_text"])
            self.assertEqual(manifest["commit_policy"], "REVIEW_REQUIRED_BEFORE_COMMIT")
            self.assertEqual(len(manifest["review_candidates"]), len(source_probe.CANDIDATES))
            self.assertTrue(manifest["review_requirements"]["manual_var_review_required"])
            self.assertEqual(
                manifest["play_bootstrap"]["source_excerpt_sha256"],
                digest(f"public void placeNewPlayer() {{ /* {OFFICIAL_SOURCE_MARKER} */ }}"),
            )

            outputs = file_map(output)
            self.assertEqual(
                len([name for name in outputs if name.startswith("records/")]),
                len(source_probe.CANDIDATES),
            )
            for raw in outputs.values():
                self.assertNotIn(OFFICIAL_SOURCE_MARKER.encode("utf-8"), raw)

            first_id = source_probe.CANDIDATES[0][0]
            first_record = json.loads(outputs[f"records/{first_id}.json"])
            self.assertEqual(first_record["status"], "INDEXED")
            self.assertEqual(first_record["hazards_reviewed"], [])
            self.assertEqual(first_record["semantic_rules"], [])
            self.assertNotIn("atlas_observed_hazards", first_record)
            self.assertEqual(
                manifest["review_candidates"][0]["atlas_observed_hazards"], ["CODEC"]
            )

            gate = json.loads(outputs[f"gate/{review.GATE_ID}.json"])
            self.assertEqual(gate["minimum_status"], "VAR_REVIEWED")
            self.assertTrue(gate["require_semantic_rules"])
            self.assertTrue(gate["require_hazards_reviewed"])
            self.assertEqual(len(gate["methods"]), len(source_probe.CANDIDATES))

    def test_review_pack_is_deterministic_across_output_directories(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock, frontier, bundle_path, _bundle = self.fixture(root)
            first = root / "first"
            second = root / "second"
            review.materialize_review_pack(
                bundle_path=bundle_path,
                output_dir=first,
                lock_path=lock,
                frontier_path=frontier,
            )
            review.materialize_review_pack(
                bundle_path=bundle_path,
                output_dir=second,
                lock_path=lock,
                frontier_path=frontier,
            )
            self.assertEqual(file_map(first), file_map(second))

    def test_ambiguous_candidate_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock, frontier, bundle_path, bundle = self.fixture(root)
            candidate = bundle["var_candidates"][0]
            candidate["match_count"] = 2
            candidate.pop("record_template")
            candidate["candidates"] = ["Type#one()", "Type#two()"]
            write_bundle(bundle_path, bundle)

            with self.assertRaisesRegex(review.ReviewPackError, "resolve exactly once"):
                review.validate_bundle(
                    bundle_path=bundle_path,
                    lock_path=lock,
                    frontier_path=frontier,
                )

    def test_source_lock_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock, frontier, bundle_path, bundle = self.fixture(root)
            bundle["source_archive_sha256"] = digest("different-source")
            write_bundle(bundle_path, bundle)

            with self.assertRaisesRegex(review.ReviewPackError, "source archive SHA-256"):
                review.validate_bundle(
                    bundle_path=bundle_path,
                    lock_path=lock,
                    frontier_path=frontier,
                )

    def test_probe_template_cannot_preclaim_manual_review(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock, frontier, bundle_path, bundle = self.fixture(root)
            template = bundle["var_candidates"][0]["record_template"]
            template["status"] = "VAR_REVIEWED"
            write_bundle(bundle_path, bundle)

            with self.assertRaisesRegex(review.ReviewPackError, "must remain INDEXED"):
                review.validate_bundle(
                    bundle_path=bundle_path,
                    lock_path=lock,
                    frontier_path=frontier,
                )

    def test_frontier_root_drift_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock, frontier, bundle_path, bundle = self.fixture(root)
            bundle["frontier_roots"] = list(reversed(bundle["frontier_roots"]))
            write_bundle(bundle_path, bundle)

            with self.assertRaisesRegex(review.ReviewPackError, "frontier roots"):
                review.validate_bundle(
                    bundle_path=bundle_path,
                    lock_path=lock,
                    frontier_path=frontier,
                )

    def test_existing_output_directory_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock, frontier, bundle_path, _bundle = self.fixture(root)
            output = root / "review-pack"
            output.mkdir()

            with self.assertRaisesRegex(review.ReviewPackError, "must not already exist"):
                review.materialize_review_pack(
                    bundle_path=bundle_path,
                    output_dir=output,
                    lock_path=lock,
                    frontier_path=frontier,
                )


if __name__ == "__main__":
    unittest.main()
