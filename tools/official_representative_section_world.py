#!/usr/bin/env python3
"""Generate one deterministic representative-section world with the pinned official server."""

from __future__ import annotations

import argparse
import hashlib
import json
import queue
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

import official_state_data
import section_representative_plan
import vanilla_section_extractor

DEFAULT_TIMEOUT_SECONDS = 600
DEFAULT_SETTLE_SECONDS = 90


class RepresentativeWorldError(RuntimeError):
    """Raised when the official representative-world generation probe fails."""


def server_properties(seed: int) -> str:
    return "\n".join(
        [
            "allow-nether=true",
            "difficulty=peaceful",
            "enable-command-block=false",
            "enable-query=false",
            "enable-rcon=false",
            "gamemode=survival",
            "generate-structures=true",
            "level-name=world",
            f"level-seed={seed}",
            "max-players=1",
            "max-tick-time=-1",
            "motd=Crucible representative section corpus",
            "online-mode=false",
            "server-ip=127.0.0.1",
            "server-port=25565",
            "simulation-distance=2",
            "spawn-protection=0",
            "sync-chunk-writes=true",
            "view-distance=2",
            "white-list=true",
        ]
    ) + "\n"


def commands_for_plan(plan: dict[str, object]) -> list[str]:
    dimensions = plan["dimensions"]
    assert isinstance(dimensions, dict)
    commands: list[str] = []
    for dimension in section_representative_plan.DIMENSIONS:
        entry = dimensions[dimension]
        assert isinstance(entry, dict)
        chunks = entry["chunks"]
        assert isinstance(chunks, list)
        for chunk in chunks:
            chunk_x = int(chunk[0])
            chunk_z = int(chunk[1])
            block_x = chunk_x * 16
            block_z = chunk_z * 16
            commands.append(
                f"execute in {dimension} run forceload add {block_x} {block_z}"
            )
    return commands


def command_digest(commands: list[str]) -> str:
    return hashlib.sha256(("\n".join(commands) + "\n").encode("utf-8")).hexdigest()


def expected_region_paths(world: Path, plan: dict[str, object]) -> list[Path]:
    dimension_dirs = {
        name: world / relative
        for name, relative in vanilla_section_extractor.STANDARD_DIMENSIONS
    }
    dimensions = plan["dimensions"]
    assert isinstance(dimensions, dict)
    paths: set[Path] = set()
    for dimension in section_representative_plan.DIMENSIONS:
        entry = dimensions[dimension]
        assert isinstance(entry, dict)
        chunks = entry["chunks"]
        assert isinstance(chunks, list)
        for chunk in chunks:
            chunk_x = int(chunk[0])
            chunk_z = int(chunk[1])
            paths.add(
                dimension_dirs[dimension]
                / f"r.{chunk_x // 32}.{chunk_z // 32}.mca"
            )
    return sorted(paths)


def _reader(stream, output: queue.Queue[str], log_path: Path) -> None:
    with log_path.open("w", encoding="utf-8") as log:
        for line in iter(stream.readline, ""):
            log.write(line)
            log.flush()
            output.put(line)
    output.put("")


def _stop_process(process: subprocess.Popen[str]) -> None:
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)


def run_server(
    *,
    server: Path,
    work_dir: Path,
    seed: int,
    plan: dict[str, object],
    timeout_seconds: int,
    settle_seconds: int,
) -> Path:
    world = work_dir / "world"
    if world.exists():
        raise RepresentativeWorldError(f"representative world already exists: {world}")
    work_dir.mkdir(parents=True, exist_ok=True)
    (work_dir / "eula.txt").write_text("eula=true\n", encoding="utf-8")
    (work_dir / "server.properties").write_text(
        server_properties(seed), encoding="utf-8"
    )

    process = subprocess.Popen(
        [
            "java",
            "-Xms512M",
            "-Xmx3072M",
            "-jar",
            str(server.resolve()),
            "nogui",
        ],
        cwd=work_dir,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )
    assert process.stdin is not None
    assert process.stdout is not None

    lines: queue.Queue[str] = queue.Queue()
    threading.Thread(
        target=_reader,
        args=(process.stdout, lines, work_dir / "server.log"),
        daemon=True,
    ).start()

    deadline = time.monotonic() + timeout_seconds
    started = False
    tail: list[str] = []
    try:
        while time.monotonic() < deadline:
            if process.poll() is not None:
                break
            try:
                line = lines.get(timeout=0.25)
            except queue.Empty:
                continue
            if not line:
                continue
            tail.append(line.rstrip())
            tail = tail[-60:]
            if "Done (" in line and 'For help, type "help"' in line:
                started = True
                break

        if not started:
            _stop_process(process)
            raise RepresentativeWorldError(
                "official server did not reach completed startup"
                + ("\n" + "\n".join(tail) if tail else "")
            )

        commands = commands_for_plan(plan)
        process.stdin.write("\n".join(commands) + "\n")
        process.stdin.flush()

        settle_deadline = min(deadline, time.monotonic() + settle_seconds)
        while time.monotonic() < settle_deadline:
            if process.poll() is not None:
                raise RepresentativeWorldError(
                    f"official server exited during representative chunk generation: {process.returncode}"
                )
            time.sleep(min(0.5, max(0.0, settle_deadline - time.monotonic())))

        process.stdin.write("save-all flush\n")
        process.stdin.flush()
        time.sleep(5)
        process.stdin.write("stop\n")
        process.stdin.flush()

        try:
            return_code = process.wait(timeout=max(1.0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired as error:
            _stop_process(process)
            raise RepresentativeWorldError("official server did not stop cleanly") from error
        if return_code != 0:
            raise RepresentativeWorldError(f"official server exited with code {return_code}")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)

    level_dat = world / "level.dat"
    if not level_dat.is_file():
        raise RepresentativeWorldError("official server produced no level.dat")
    missing_regions = [path for path in expected_region_paths(world, plan) if not path.is_file()]
    if missing_regions:
        raise RepresentativeWorldError(
            "official server did not materialize all selected region files: "
            + ", ".join(str(path) for path in missing_regions[:12])
        )
    return world


def generate(
    *,
    version: str,
    work_dir: Path,
    cache: Path,
    plan_path: Path,
    seed_index: int,
    timeout_seconds: int,
    settle_seconds: int,
    evidence_output: Path | None,
) -> Path:
    if work_dir.exists():
        if any(work_dir.iterdir()):
            raise RepresentativeWorldError(
                f"work directory must be absent or empty: {work_dir}"
            )
        shutil.rmtree(work_dir)

    plan = section_representative_plan.load_plan(plan_path)
    target = plan["target"]
    assert isinstance(target, dict)
    if version != target["minecraft_version"]:
        raise RepresentativeWorldError(
            f"requested version {version} does not match plan target {target['minecraft_version']}"
        )
    seeds = plan["seeds"]
    assert isinstance(seeds, list)
    if seed_index < 0 or seed_index >= len(seeds):
        raise RepresentativeWorldError(
            f"seed-index must be in 0..{len(seeds) - 1}; got {seed_index}"
        )
    seed = int(seeds[seed_index])

    resolved, _ = official_state_data.resolve(version, cache)
    server, _ = resolved["server"]
    server_sha256 = official_state_data.sha256_file(server)
    world = run_server(
        server=server,
        work_dir=work_dir,
        seed=seed,
        plan=plan,
        timeout_seconds=timeout_seconds,
        settle_seconds=settle_seconds,
    )

    if evidence_output is not None:
        commands = commands_for_plan(plan)
        evidence_output.parent.mkdir(parents=True, exist_ok=True)
        evidence_output.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "generator": "official-server-representative-section-world-v1",
                    "minecraft_version": version,
                    "server_sha256": server_sha256,
                    "representative_policy": plan["policy"],
                    "plan_sha256": plan["plan_sha256"],
                    "seed_index": seed_index,
                    "seed": seed,
                    "command_count": len(commands),
                    "command_sha256": command_digest(commands),
                    "settle_seconds": settle_seconds,
                    "server_properties": server_properties(seed).splitlines(),
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
    return world


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", default="26.2")
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--seed-index", type=int, required=True)
    parser.add_argument(
        "--cache", type=Path, default=Path(".crucible/vanilla/downloads")
    )
    parser.add_argument("--timeout-seconds", type=int, default=DEFAULT_TIMEOUT_SECONDS)
    parser.add_argument("--settle-seconds", type=int, default=DEFAULT_SETTLE_SECONDS)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    if args.timeout_seconds <= 0:
        parser.error("--timeout-seconds must be positive")
    if args.settle_seconds < 0:
        parser.error("--settle-seconds must be non-negative")

    try:
        world = generate(
            version=args.version,
            work_dir=args.work_dir,
            cache=args.cache,
            plan_path=args.plan,
            seed_index=args.seed_index,
            timeout_seconds=args.timeout_seconds,
            settle_seconds=args.settle_seconds,
            evidence_output=args.evidence,
        )
    except (
        RepresentativeWorldError,
        section_representative_plan.PlanError,
        ValueError,
        OSError,
        subprocess.SubprocessError,
    ) as error:
        print(f"official representative section world error: {error}", file=sys.stderr)
        return 1

    print(f"official representative section world: {world} PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
