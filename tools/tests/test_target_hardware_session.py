from __future__ import annotations

import copy
import hashlib
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from typing import Callable

from tools import target_hardware_session as session

COMMIT = "a" * 40


def hardware() -> dict[str, object]:
    return {
        "commit_sha": COMMIT,
        "rustc_verbose": "rustc 1.97.1\nhost: x86_64-unknown-linux-gnu",
        "target_triple": "x86_64-unknown-linux-gnu",
        "cpu_model": "Synthetic CPU",
        "cpu_vendor": "GenuineSynthetic",
        "cpu_family": "6",
        "cpu_model_id": "999",
        "cpu_stepping": "1",
        "cpu_microcode": "0x1234",
        "kernel": "Linux synthetic",
        "cpu_governor": "performance",
        "cpu_current_khz": "4000000",
        "cpu_min_khz": "800000",
        "cpu_max_khz": "5000000",
        "cpus_allowed_list": "0-7",
        "mems_allowed_list": "0",
        "online_cpus": "0-7",
        "smt_active": "1",
        "cache_topology": "L1:Data:size=48K:line=64:shared=0-1",
        "perf_event_paranoid": "1",
        "transparent_hugepage": "always [madvise] never",
        "memory_total_kib": "33554432 kB",
        "load_average": "0.10 0.20 0.30 1/100 1",
        "no_turbo": "0",
        "rustflags": "",
        "cargo_encoded_rustflags": "",
    }


def artifact_for(spec: session.BenchmarkSpec) -> dict[str, object]:
    base: dict[str, object] = {
        "schema": 1,
        "benchmark": spec.benchmark,
        "mode": "full",
        "hosted_ci_is_diagnostic_only": True,
        "hardware": hardware(),
    }
    if spec.key == "composition_hot":
        base.update(
            {
                "structural": {"exact_type_identity": True},
                "paired_rounds": [{"round": 0}],
                "semantic_checksum": 17,
            }
        )
    elif spec.key == "world_access":
        base["cases"] = [
            {
                "semantic_checksum": 19,
                "paired_rounds": [{"round": 0}],
                "setup_samples_ns": [101],
                "whole_cost_samples": [{"round": 0}],
            }
        ]
    elif spec.key == "executor_baseline":
        base.update(
            {
                "semantic_reference": {
                    "stage_count": 16,
                    "useful_operations": 4096,
                    "work_checksum": 23,
                },
                "candidates": [{"workers": 1}, {"workers": 2}, {"workers": 4}],
                "rounds": [{"round": 0}],
            }
        )
    elif spec.key == "fused_outbound":
        base.update(
            {
                "production_path_unchanged": True,
                "cases": [
                    {
                        "byte_equivalent": True,
                        "semantic_checksum": 29,
                        "paired_rounds": [{"round": 0}],
                    }
                ],
            }
        )
    else:  # pragma: no cover - session.BENCHMARKS is closed.
        raise AssertionError(spec.key)
    return base


class FakeRunner:
    def __init__(
        self,
        repo: Path,
        *,
        dirty: bool = False,
        fail_binary: str | None = None,
        mutate: Callable[[session.BenchmarkSpec, dict[str, object]], None] | None = None,
    ) -> None:
        self.repo = repo
        self.dirty = dirty
        self.fail_binary = fail_binary
        self.mutate = mutate
        self.calls: list[list[str]] = []

    def __call__(self, argv: list[str] | tuple[str, ...], cwd: Path) -> subprocess.CompletedProcess[str]:
        args = list(argv)
        self.calls.append(args)
        if cwd.resolve() != self.repo.resolve():
            return subprocess.CompletedProcess(args, 1, "", "wrong cwd")

        if args == ["git", "rev-parse", "--show-toplevel"]:
            return subprocess.CompletedProcess(args, 0, f"{self.repo}\n", "")
        if args == ["git", "status", "--porcelain", "--untracked-files=all"]:
            status = "?? dirty.txt\n" if self.dirty else ""
            return subprocess.CompletedProcess(args, 0, status, "")
        if args == ["git", "rev-parse", "HEAD"]:
            return subprocess.CompletedProcess(args, 0, f"{COMMIT}\n", "")
        if args and args[0] == "cargo":
            binary = args[args.index("--bin") + 1]
            if binary == self.fail_binary:
                return subprocess.CompletedProcess(args, 7, "", "synthetic benchmark failure")
            spec = next(spec for spec in session.BENCHMARKS if spec.binary == binary)
            payload = copy.deepcopy(artifact_for(spec))
            if self.mutate is not None:
                self.mutate(spec, payload)
            output = self.repo / args[args.index("--output") + 1]
            output.write_text(
                json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n",
                encoding="utf-8",
            )
            return subprocess.CompletedProcess(args, 0, "", "")
        return subprocess.CompletedProcess(args, 1, "", "unexpected command")


class TargetHardwareSessionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name) / "repo"
        self.repo.mkdir()

    def test_valid_session_is_content_addressed_and_deterministic(self) -> None:
        output = Path("target/qualification/session")
        first_manifest, first_digest = session.run_session(
            repo_root=self.repo,
            output_dir=output,
            runner=FakeRunner(self.repo),
            environment={},
        )
        first_bytes = first_manifest.read_bytes()
        record = json.loads(first_bytes)
        self.assertEqual(record["schema"], 1)
        self.assertEqual(record["commit_sha"], COMMIT)
        self.assertFalse(record["decision_made"])
        self.assertEqual(
            [artifact["key"] for artifact in record["artifacts"]],
            [spec.key for spec in session.BENCHMARKS],
        )
        self.assertEqual(first_digest, hashlib.sha256(first_bytes).hexdigest())
        sidecar = first_manifest.with_name(session.SIDECAR_NAME).read_text(encoding="ascii")
        self.assertEqual(sidecar, f"{first_digest}  {session.MANIFEST_NAME}\n")

        shutil.rmtree(self.repo / output)
        second_manifest, second_digest = session.run_session(
            repo_root=self.repo,
            output_dir=output,
            runner=FakeRunner(self.repo),
            environment={},
        )
        self.assertEqual(second_digest, first_digest)
        self.assertEqual(second_manifest.read_bytes(), first_bytes)

    def test_dirty_worktree_is_rejected_before_output_creation(self) -> None:
        output = Path("target/session")
        with self.assertRaisesRegex(session.SessionError, "clean worktree"):
            session.run_session(
                repo_root=self.repo,
                output_dir=output,
                runner=FakeRunner(self.repo, dirty=True),
                environment={},
            )
        self.assertFalse((self.repo / output).exists())

    def test_command_failure_cannot_create_sealed_manifest(self) -> None:
        output = Path("target/session")
        with self.assertRaisesRegex(session.SessionError, "benchmark command failed"):
            session.run_session(
                repo_root=self.repo,
                output_dir=output,
                runner=FakeRunner(self.repo, fail_binary="world_access_bench"),
                environment={},
            )
        self.assertFalse((self.repo / output / session.MANIFEST_NAME).exists())
        self.assertFalse((self.repo / output / session.SIDECAR_NAME).exists())

    def test_common_and_semantic_guards_fail_closed(self) -> None:
        spec = session.BENCHMARKS[0]
        mutations = (
            ("schema", lambda payload: payload.__setitem__("schema", 2)),
            ("benchmark", lambda payload: payload.__setitem__("benchmark", "wrong")),
            ("full mode", lambda payload: payload.__setitem__("mode", "smoke")),
            (
                "type identity",
                lambda payload: payload["structural"].__setitem__("exact_type_identity", False),
            ),
            ("semantic checksum", lambda payload: payload.__setitem__("semantic_checksum", 0)),
        )
        for label, mutate in mutations:
            with self.subTest(label=label):
                payload = artifact_for(spec)
                mutate(payload)
                with self.assertRaises(session.SessionError):
                    session.validate_artifact(spec, payload, COMMIT)

    def test_embedded_commit_mismatch_is_rejected(self) -> None:
        payload = artifact_for(session.BENCHMARKS[0])
        payload["hardware"]["commit_sha"] = "b" * 40
        with self.assertRaisesRegex(session.SessionError, "embedded commit"):
            session.validate_artifact(session.BENCHMARKS[0], payload, COMMIT)

    def test_stable_machine_identity_mismatch_aborts_session(self) -> None:
        def mutate(spec: session.BenchmarkSpec, payload: dict[str, object]) -> None:
            if spec.key == "executor_baseline":
                payload["hardware"]["cpu_model"] = "Different CPU"

        output = Path("target/session")
        with self.assertRaisesRegex(session.SessionError, "stable machine/toolchain identity"):
            session.run_session(
                repo_root=self.repo,
                output_dir=output,
                runner=FakeRunner(self.repo, mutate=mutate),
                environment={},
            )
        self.assertFalse((self.repo / output / session.MANIFEST_NAME).exists())

    def test_dynamic_frequency_load_and_affinity_drift_are_retained_not_rejected(self) -> None:
        ordinal = {spec.key: index for index, spec in enumerate(session.BENCHMARKS)}

        def mutate(spec: session.BenchmarkSpec, payload: dict[str, object]) -> None:
            index = ordinal[spec.key]
            payload["hardware"]["cpu_current_khz"] = str(3_000_000 + index)
            payload["hardware"]["load_average"] = f"0.{index} 0.0 0.0 1/1 1"
            payload["hardware"]["cpus_allowed_list"] = "0" if index < 2 else "0-3"

        output = Path("target/session")
        manifest, _ = session.run_session(
            repo_root=self.repo,
            output_dir=output,
            runner=FakeRunner(self.repo, mutate=mutate),
            environment={},
        )
        stable = json.loads(manifest.read_text(encoding="utf-8"))["stable_hardware_identity"]
        self.assertNotIn("cpu_current_khz", stable)
        self.assertNotIn("load_average", stable)
        self.assertNotIn("cpus_allowed_list", stable)

    def test_existing_output_is_never_overwritten(self) -> None:
        output = Path("target/session")
        (self.repo / output).mkdir(parents=True)
        with self.assertRaisesRegex(session.SessionError, "already exists"):
            session.run_session(
                repo_root=self.repo,
                output_dir=output,
                runner=FakeRunner(self.repo),
                environment={},
            )

    def test_github_actions_cannot_produce_authoritative_session(self) -> None:
        with self.assertRaisesRegex(session.SessionError, "forbidden in GitHub Actions"):
            session.run_session(
                repo_root=self.repo,
                output_dir=Path("target/session"),
                runner=FakeRunner(self.repo),
                environment={"GITHUB_ACTIONS": "true"},
            )


if __name__ == "__main__":
    unittest.main()
