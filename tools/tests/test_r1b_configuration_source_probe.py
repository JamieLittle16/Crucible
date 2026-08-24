from __future__ import annotations

import hashlib
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from tools import r1b_configuration_source_probe as probe


REPO_ROOT = Path(__file__).resolve().parents[2]
FRONTIER_PATH = REPO_ROOT / "vanilla/frontiers/r1b-configuration-selected.json"


def provenance_connection(
    source_sha: str,
    *,
    fingerprint: str = "java-token-v2-literal-sensitive",
) -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.execute("CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)")
    conn.executemany(
        "INSERT INTO meta(key,value) VALUES(?,?)",
        (
            ("source_archive_sha256", source_sha),
            ("minecraft_version", "26.2"),
            ("protocol_version", "776"),
            ("atlas_version", "0.1.1"),
            ("fingerprint_algorithm", fingerprint),
        ),
    )
    return conn


def write_lock(path: Path, source_sha: str) -> None:
    path.write_text(
        f'''schema = 1
minecraft = "26.2"
protocol = 776

[source]
archive_sha256 = "{source_sha}"

[atlas]
version = "0.1.1"
fingerprint_algorithm = "java-token-v2-literal-sensitive"
''',
        encoding="utf-8",
    )


class R1BConfigurationSourceProbeTests(unittest.TestCase):
    def test_source_provenance_requires_archive_and_atlas_to_match_lock(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "mc-src.zip"
            source.write_bytes(b"pinned official source fixture")
            source_sha = hashlib.sha256(source.read_bytes()).hexdigest()
            lock = root / "vanilla.lock.toml"
            write_lock(lock, source_sha)
            conn = provenance_connection(source_sha)
            self.addCleanup(conn.close)

            self.assertEqual(probe.require_pinned_source(conn, source, lock), source_sha)

    def test_source_provenance_rejects_wrong_archive_before_evidence_emission(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "mc-src.zip"
            source.write_bytes(b"wrong source")
            expected_sha = hashlib.sha256(b"expected source").hexdigest()
            lock = root / "vanilla.lock.toml"
            write_lock(lock, expected_sha)
            conn = provenance_connection(expected_sha)
            self.addCleanup(conn.close)

            with self.assertRaisesRegex(probe.ProbeError, "source archive SHA-256"):
                probe.require_pinned_source(conn, source, lock)

    def test_source_provenance_rejects_stale_atlas_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "mc-src.zip"
            source.write_bytes(b"pinned source")
            source_sha = hashlib.sha256(source.read_bytes()).hexdigest()
            lock = root / "vanilla.lock.toml"
            write_lock(lock, source_sha)
            conn = provenance_connection(source_sha, fingerprint="obsolete-fingerprint")
            self.addCleanup(conn.close)

            with self.assertRaisesRegex(probe.ProbeError, "Atlas/source lock mismatch"):
                probe.require_pinned_source(conn, source, lock)

    def test_explicit_root_resolution_fails_closed_on_one_missing_root(self) -> None:
        mapping = {"one": [1], "two": [], "three": [3]}

        def resolver(_conn: object, query: str) -> list[int]:
            return mapping[query]

        with self.assertRaisesRegex(probe.ProbeError, "two"):
            probe.require_frontier_roots(object(), ["one", "two", "three"], resolver)

    def test_explicit_root_resolution_preserves_every_successful_root(self) -> None:
        mapping = {"one": [1, 2], "two": [3]}

        def resolver(_conn: object, query: str) -> list[int]:
            return mapping[query]

        resolved = probe.require_frontier_roots(object(), ["one", "two"], resolver)
        self.assertEqual(resolved, [("one", [1, 2]), ("two", [3])])

    def test_method_extraction_ignores_braces_in_literals_and_comments(self) -> None:
        source = '''
class Example {
    void before() {}
    public void placeNewPlayer(int value) {
        String text = "literal { not structure }";
        // } line comment
        if (value > 0) {
            /* { block comment } */
            value++;
        }
    }
    void after() {}
}
'''
        extracted = probe.extract_java_method(source, "placeNewPlayer")
        self.assertIn("public void placeNewPlayer", extracted)
        self.assertIn("value++;", extracted)
        self.assertNotIn("void after", extracted)
        self.assertTrue(extracted.rstrip().endswith("}"))

    def test_method_extraction_never_confuses_preceding_call_site_for_declaration(self) -> None:
        source = '''
class Example {
    void helper() {
        placeNewPlayer(7);
        if (true) { System.out.println("call-site block"); }
    }

    public void placeNewPlayer(int value) {
        value++;
    }
}
'''
        extracted = probe.extract_java_method(source, "placeNewPlayer")
        self.assertIn("public void placeNewPlayer", extracted)
        self.assertIn("value++;", extracted)
        self.assertNotIn("placeNewPlayer(7)", extracted)
        self.assertNotIn("call-site block", extracted)

    def test_method_extraction_owner_filter_rejects_nested_same_name(self) -> None:
        source = '''
class PlayerList {
    static class Helper {
        void placeNewPlayer(String wrong) {}
    }

    public void placeNewPlayer(int right) {
        right++;
    }
}
'''
        extracted = probe.extract_java_method(
            source,
            "placeNewPlayer",
            owner_simple_name="PlayerList",
        )
        self.assertIn("int right", extracted)
        self.assertNotIn("String wrong", extracted)

    def test_method_extraction_fails_closed_on_overload_ambiguity(self) -> None:
        source = '''
class PlayerList {
    void placeNewPlayer(int one) {}
    void placeNewPlayer(String two) {}
}
'''
        with self.assertRaisesRegex(probe.ProbeError, "ambiguous"):
            probe.extract_java_method(
                source,
                "placeNewPlayer",
                owner_simple_name="PlayerList",
            )

    def test_candidate_ids_and_queries_are_unique(self) -> None:
        ids = [var_id for var_id, _query in probe.CANDIDATES]
        queries = [query for _var_id, query in probe.CANDIDATES]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(len(queries), len(set(queries)))

    def test_bundle_writer_is_deterministic_and_preserves_ephemeral_source_policy(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "nested" / "r1b-bundle.json"
            bundle = {
                "schema": 1,
                "kind": probe.BUNDLE_KIND,
                "commit_policy": "EPHEMERAL_DO_NOT_COMMIT",
                "contains_official_source_text": True,
                "var_candidates": [
                    {
                        "var_id": "VAR-UNIQUE",
                        "match_count": 1,
                        "record_template": {"id": "VAR-UNIQUE"},
                    },
                    {
                        "var_id": "VAR-AMBIGUOUS",
                        "match_count": 2,
                        "candidates": ["A#f()", "B#f()"],
                    },
                ],
                "play_bootstrap_source": {
                    "path": probe.PLAYER_LIST_PATH,
                    "source": "void placeNewPlayer() {}",
                },
            }

            probe.write_admission_bundle(output, bundle)
            first = output.read_bytes()
            probe.write_admission_bundle(output, bundle)
            second = output.read_bytes()

            self.assertEqual(first, second)
            decoded = json.loads(first)
            self.assertEqual(decoded["schema"], 1)
            self.assertEqual(decoded["kind"], probe.BUNDLE_KIND)
            self.assertEqual(decoded["commit_policy"], "EPHEMERAL_DO_NOT_COMMIT")
            self.assertIs(decoded["contains_official_source_text"], True)
            self.assertNotIn("record_template", decoded["var_candidates"][1])
            self.assertEqual(
                decoded["var_candidates"][1]["candidates"],
                ["A#f()", "B#f()"],
            )

    def test_bundle_output_is_explicit_not_a_default_repository_artifact(self) -> None:
        parser = probe.build_parser()
        default_args = parser.parse_args([])
        explicit = parser.parse_args(["--bundle-output", "/tmp/r1b.json"])

        self.assertIsNone(default_args.bundle_output)
        self.assertEqual(explicit.bundle_output, Path("/tmp/r1b.json"))

    def test_selected_frontier_is_narrow_and_contains_required_handoff_anchors(self) -> None:
        frontier = json.loads(FRONTIER_PATH.read_text(encoding="utf-8"))
        roots = "\n".join(frontier["root_queries"])
        self.assertEqual(frontier["schema"], 1)
        self.assertEqual(len(frontier["root_queries"]), len(set(frontier["root_queries"])))
        for required in (
            "ConfigurationProtocols",
            "SynchronizeRegistriesTask",
            "PrepareSpawnTask",
            "JoinWorldTask",
            "PlayerList#placeNewPlayer",
            "ClientboundUpdateTagsPacket",
            "ServerboundClientInformationPacket",
        ):
            self.assertIn(required, roots)
        for forbidden in (
            "CodeOfConduct",
            "ResourcePack",
            "ServerLinks",
            "protocol.status",
        ):
            self.assertNotIn(forbidden, roots)


if __name__ == "__main__":
    unittest.main()
