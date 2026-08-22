from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import ci_policy


class CiPolicyTests(unittest.TestCase):
    def test_actions_must_be_pinned_to_full_commit_sha(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            github_config = Path(temporary)
            (github_config / "good.yml").write_text(
                "steps:\n"
                "  - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6\n"
                "  - uses: ./local-action\n",
                encoding="utf-8",
            )
            self.assertEqual(ci_policy.workflow_action_errors(github_config), [])

            nested = github_config / "actions" / "nested"
            nested.mkdir(parents=True)
            (nested / "action.yaml").write_text(
                "runs:\n"
                "  steps:\n"
                "    - uses: actions/checkout@v6\n",
                encoding="utf-8",
            )
            errors = ci_policy.workflow_action_errors(github_config)
            self.assertTrue(any("full 40-hex commit SHA" in error for error in errors))
            self.assertTrue(any("action.yaml" in error for error in errors))

    def test_internal_only_lockfile_needs_no_allowlist(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = root / "Cargo.lock"
            allowlist = root / "allowlist.txt"
            lock.write_text(
                "version = 4\n\n[[package]]\nname = \"internal\"\nversion = \"0.0.0\"\n",
                encoding="utf-8",
            )
            allowlist.write_text("# no external dependencies\n", encoding="utf-8")
            self.assertEqual(ci_policy.dependency_errors(lock, allowlist), [])

    def test_registry_dependency_requires_exact_allowlist_entry(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = root / "Cargo.lock"
            allowlist = root / "allowlist.txt"
            lock.write_text(
                "version = 4\n\n"
                "[[package]]\n"
                "name = \"example\"\n"
                "version = \"1.2.3\"\n"
                f"source = \"{ci_policy.CRATES_IO_SOURCE}\"\n",
                encoding="utf-8",
            )
            allowlist.write_text("", encoding="utf-8")
            errors = ci_policy.dependency_errors(lock, allowlist)
            self.assertTrue(any("unreviewed crates.io dependency" in error for error in errors))

            allowlist.write_text("example@1.2.3\n", encoding="utf-8")
            self.assertEqual(ci_policy.dependency_errors(lock, allowlist), [])

    def test_git_and_unknown_sources_are_forbidden(self) -> None:
        cases = [
            "git+https://github.com/example/repo?rev=deadbeef",
            "registry+https://example.invalid/index",
        ]
        for source in cases:
            with self.subTest(source=source), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                lock = root / "Cargo.lock"
                allowlist = root / "allowlist.txt"
                lock.write_text(
                    "version = 4\n\n"
                    "[[package]]\n"
                    "name = \"example\"\n"
                    "version = \"1.0.0\"\n"
                    f"source = \"{source}\"\n",
                    encoding="utf-8",
                )
                allowlist.write_text("example@1.0.0\n", encoding="utf-8")
                self.assertTrue(ci_policy.dependency_errors(lock, allowlist))

    def test_stale_and_duplicate_allowlist_entries_fail(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = root / "Cargo.lock"
            allowlist = root / "allowlist.txt"
            lock.write_text(
                "version = 4\n\n[[package]]\nname = \"internal\"\nversion = \"0.0.0\"\n",
                encoding="utf-8",
            )
            allowlist.write_text("example@1.0.0\nexample@1.0.0\n", encoding="utf-8")
            errors = ci_policy.dependency_errors(lock, allowlist)
            self.assertTrue(any("duplicate" in error for error in errors))
            self.assertTrue(any("stale" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
