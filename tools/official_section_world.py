#!/usr/bin/env python3
"""Generate a small deterministic world with the pinned official Minecraft server."""

from __future__ import annotations

import argparse
import json
import queue
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

import official_state_data

DEFAULT_SEED = "6842363988700132471"
DEFAULT_TIMEOUT_SECONDS = 240


class WorldProbeError(RuntimeError):
    """Raised when the official world-generation probe fails."""


def server_properties(seed: str) -> str:
    return "\n".join(
        [
            "allow-nether=false",
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
            "motd=Crucible section corpus probe",
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


def _reader(stream, output: queue.Queue[str], log_path: Path) -> None:
    with log_path.open("w", encoding="utf-8") as log:
        for line in iter(stream.readline, ""):
            log.write(line)
            log.flush()
            output.put(line)
    output.put("")


def run_server(server: Path, work_dir: Path, timeout_seconds: int) -> Path:
    world = work_dir / "world"
    if world.exists():
        raise WorldProbeError(f"probe world already exists: {world}")
    work_dir.mkdir(parents=True, exist_ok=True)
    (work_dir / "eula.txt").write_text("eula=true\n", encoding="utf-8")
    (work_dir / "server.properties").write_text(
        server_properties(DEFAULT_SEED), encoding="utf-8"
    )

    command = [
        "java",
        "-Xms512M",
        "-Xmx2048M",
        "-jar",
        str(server.resolve()),
        "nogui",
    ]
    process = subprocess.Popen(
        command,
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
    reader = threading.Thread(
        target=_reader,
        args=(process.stdout, lines, work_dir / "server.log"),
        daemon=True,
    )
    reader.start()

    deadline = time.monotonic() + timeout_seconds
    started = False
    output_tail: list[str] = []
    try:
        while time.monotonic() < deadline:
            if process.poll() is not None:
                break
            try:
                line = lines.get(timeout=0.25)
            except queue.Empty:
                continue
            if line:
                output_tail.append(line.rstrip())
                output_tail = output_tail[-40:]
                if "Done (" in line and 'For help, type "help"' in line:
                    started = True
                    process.stdin.write("save-all flush\n")
                    process.stdin.write("stop\n")
                    process.stdin.flush()
                    break

        if not started:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)
            tail = "\n".join(output_tail)
            raise WorldProbeError(
                "official server did not reach completed startup before exit/timeout"
                + (f":\n{tail}" if tail else "")
            )

        remaining = max(1.0, deadline - time.monotonic())
        try:
            return_code = process.wait(timeout=remaining)
        except subprocess.TimeoutExpired as error:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
            raise WorldProbeError("official server did not stop cleanly") from error
        if return_code != 0:
            raise WorldProbeError(f"official server exited with code {return_code}")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)

    level_dat = world / "level.dat"
    region_dir = world / "region"
    if not level_dat.is_file() or not region_dir.is_dir():
        raise WorldProbeError("official server exited without a complete overworld save")
    if not any(region_dir.glob("r.*.*.mca")):
        raise WorldProbeError("official server save contains no overworld region files")
    return world


def generate(
    version: str,
    work_dir: Path,
    cache: Path,
    timeout_seconds: int,
    evidence_output: Path | None,
) -> Path:
    if work_dir.exists():
        if any(work_dir.iterdir()):
            raise WorldProbeError(f"work directory must be absent or empty: {work_dir}")
        shutil.rmtree(work_dir)

    resolved, _ = official_state_data.resolve(version, cache)
    server, _ = resolved["server"]
    server_sha256 = official_state_data.sha256_file(server)
    world = run_server(server, work_dir, timeout_seconds)

    if evidence_output is not None:
        evidence_output.parent.mkdir(parents=True, exist_ok=True)
        evidence_output.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "minecraft_version": version,
                    "server_sha256": server_sha256,
                    "seed": DEFAULT_SEED,
                    "generator": "official-server-spawn-world-v1",
                    "server_properties": server_properties(DEFAULT_SEED).splitlines(),
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
    parser.add_argument("--cache", type=Path, default=Path(".crucible/vanilla/downloads"))
    parser.add_argument("--timeout-seconds", type=int, default=DEFAULT_TIMEOUT_SECONDS)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    if args.timeout_seconds <= 0:
        parser.error("--timeout-seconds must be positive")
    try:
        world = generate(
            version=args.version,
            work_dir=args.work_dir,
            cache=args.cache,
            timeout_seconds=args.timeout_seconds,
            evidence_output=args.evidence,
        )
    except (WorldProbeError, ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"official section world error: {error}", file=sys.stderr)
        return 1
    print(f"official section world: {world} PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
