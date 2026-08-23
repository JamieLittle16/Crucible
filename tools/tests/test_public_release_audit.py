from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import public_release_audit


class PublicReleaseAuditTests(unittest.TestCase):
    def init_repo(self, root: Path) -> None:
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        subprocess.run(["git", "-C", str(root), "config", "user.email", "audit@example.invalid"], check=True)
        subprocess.run(["git", "-C", str(root), "config", "user.name", "Audit Test"], check=True)

    def commit_all(self, root: Path, message: str) -> None:
        # Test fixtures intentionally include paths such as `.env` that are often
        # ignored by a developer's global Git configuration. Force-add makes the
        # temporary repositories hermetic instead of inheriting workstation policy.
        subprocess.run(["git", "-C", str(root), "add", "--force", "--all"], check=True)
        subprocess.run(["git", "-C", str(root), "commit", "-q", "-m", message], check=True)

    def test_clean_history_passes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.init_repo(root)
            (root / "README.md").write_text("clean\n", encoding="utf-8")
            self.commit_all(root, "clean")
            self.assertEqual(public_release_audit.audit(root), [])

    def test_scanner_source_does_not_match_its_own_signatures(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.init_repo(root)
            scanner = root / "public_release_audit.py"
            scanner.write_bytes(Path(public_release_audit.__file__).read_bytes())
            self.commit_all(root, "commit scanner source")
            findings = public_release_audit.audit(root)
            self.assertFalse(
                any(finding.kind == "credential-pattern" for finding in findings),
                findings,
            )

    def test_legacy_scanner_pgp_literal_is_not_key_material(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.init_repo(root)
            scanner = root / "tools" / "public_release_audit.py"
            scanner.parent.mkdir(parents=True)
            scanner.write_bytes(
                b'("PGP private key", re.compile(rb"'
                + public_release_audit.PGP_PRIVATE_KEY_HEADER
                + b'")),\n'
            )
            self.commit_all(root, "legacy scanner literal")
            findings = public_release_audit.audit(root)
            self.assertFalse(
                any(finding.kind == "credential-pattern" for finding in findings),
                findings,
            )

    def test_real_pgp_header_in_scanner_still_blocks(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.init_repo(root)
            scanner = root / "tools" / "public_release_audit.py"
            scanner.parent.mkdir(parents=True)
            scanner.write_bytes(
                b'("PGP private key", re.compile(rb"'
                + public_release_audit.PGP_PRIVATE_KEY_HEADER
                + b'")),\n'
                + public_release_audit.PGP_PRIVATE_KEY_HEADER
                + b"\nVersion: test\n\nsecret-payload\n"
            )
            self.commit_all(root, "scanner with real pgp header")
            findings = public_release_audit.audit(root)
            self.assertTrue(
                any(
                    finding.blocking
                    and finding.kind == "credential-pattern"
                    and finding.path == "tools/public_release_audit.py"
                    and "PGP private key" in finding.detail
                    for finding in findings
                ),
                findings,
            )

    def test_deleted_historical_secret_is_still_detected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.init_repo(root)
            token = "ghp_" + "A" * 36
            (root / "temporary.txt").write_text(f"token={token}\n", encoding="utf-8")
            self.commit_all(root, "add accidental token")
            (root / "temporary.txt").unlink()
            self.commit_all(root, "delete accidental token")

            findings = public_release_audit.audit(root)
            self.assertTrue(
                any(
                    finding.blocking
                    and finding.kind == "credential-pattern"
                    and finding.path == "temporary.txt"
                    for finding in findings
                )
            )

    def test_large_historical_blob_is_scanned_for_secrets(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.init_repo(root)
            token = b"ghp_" + b"B" * 36
            payload = root / "large.bin"
            payload.write_bytes(b"x" * (6 * 1024 * 1024) + b"\n" + token + b"\n")
            self.commit_all(root, "add large blob with accidental token")

            findings = public_release_audit.audit(root)
            self.assertTrue(
                any(
                    finding.blocking
                    and finding.kind == "credential-pattern"
                    and finding.path == "large.bin"
                    and "GitHub classic token" in finding.detail
                    for finding in findings
                ),
                findings,
            )

    def test_deleted_historical_jar_is_still_detected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.init_repo(root)
            (root / "server.jar").write_bytes(b"not really a jar")
            self.commit_all(root, "add forbidden artifact")
            (root / "server.jar").unlink()
            self.commit_all(root, "delete forbidden artifact")

            findings = public_release_audit.audit(root)
            self.assertTrue(
                any(
                    finding.blocking
                    and finding.kind == "forbidden-path"
                    and finding.path == "server.jar"
                    for finding in findings
                )
            )

    def test_forbidden_historical_name_survives_blob_reuse(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.init_repo(root)
            payload = b"same bytes under two names\n"
            (root / "harmless.txt").write_bytes(payload)
            self.commit_all(root, "add harmless blob")
            (root / "server.jar").write_bytes(payload)
            self.commit_all(root, "reuse blob under forbidden name")
            (root / "server.jar").unlink()
            self.commit_all(root, "delete forbidden name")

            findings = public_release_audit.audit(root)
            self.assertTrue(
                any(
                    finding.blocking
                    and finding.kind == "forbidden-path"
                    and finding.path == "server.jar"
                    for finding in findings
                )
            )

    def test_bootstrap_transport_is_review_only(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.init_repo(root)
            payload = root / ".bootstrap" / "docs" / "part-000.b64"
            payload.parent.mkdir(parents=True)
            payload.write_text("UEsDBAoAAAAA\n", encoding="utf-8")
            self.commit_all(root, "temporary transport")

            findings = public_release_audit.audit(root)
            self.assertEqual(len(findings), 1)
            self.assertFalse(findings[0].blocking)
            self.assertEqual(findings[0].kind, "transport-history")
            self.assertEqual(findings[0].path, ".bootstrap/docs/part-000.b64")

    def test_env_and_private_key_paths_are_blocking(self) -> None:
        for path in [".env", "config/.env.production", "keys/deploy.pem"]:
            with self.subTest(path=path), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.init_repo(root)
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text("placeholder\n", encoding="utf-8")
                self.commit_all(root, "add forbidden secret path")
                findings = public_release_audit.audit(root)
                self.assertTrue(any(finding.blocking and finding.path == path for finding in findings))


if __name__ == "__main__":
    unittest.main()
