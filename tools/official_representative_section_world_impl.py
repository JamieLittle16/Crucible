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
from dataclasses import dataclass
from pathlib import Path

import official_state_data
import section_representative_plan

DEFAULT_TIMEOUT_SECONDS = 900
DEFAULT_BATCH_SIZE = 8
DEFAULT_BATCH_SETTLE_SECONDS = 2
GENERATOR_ID = "official-server-representative-section-world-v2-batched"


class RepresentativeWorldError(RuntimeError):
    """Raised when the official representative-world generation probe fails."""


@dataclass(frozen=True, slots=True)
class ChunkTicket:
    """One dimension-scoped chunk requested from the official server."""

    dimension: str
    chunk_x: int
    chunk_z: int

    @property
    def block_x(self) -> int:
        return self.chunk_x * 16

    @property
    def block_z(self) -> int:
        return self.chunk_z * 16

    def add_command(self) -> str:
        return (
            f"execute in {self.dimension} run forceload add "
            f"{self.block_x} {self.block_z}"
        )

    def remove_command(self) -> str:
        return (
            f"execute in {self.dimension} run forceload remove "
            f"{self.block_x} {self.block_z}"
        )


@dataclass(frozen=True, slots=True)
class GenerationBatch:
    """Bounded same-dimension generation unit."""

    index: int
    dimension: str
    tickets: tuple[ChunkTicket, ...]


@dataclass(frozen=True, slots=True)
class BatchTiming:
    index: int
    dimension: str
    ticket_count: int
    elapsed_ms: int


class ServerConsole:
    """Small line-oriented command/barrier adapter for the official server process."""

    def __init__(
        self,
        process: subprocess.Popen[str],
        lines: queue.Queue[str],
    ) -> None:
        self.process = process
        self.lines = lines
        self.tail: list[str] = []
        assert process.stdin is not None
        self.stdin = process.stdin

    def _observe(self, line: str) -> None:
        self.tail.append(line.rstrip())
        self.tail = self.tail[-80:]

    def send(self, commands: list[str] | tuple[str, ...]) -> None:
        if not commands:
            return
        if self.process.poll() is not None:
            raise RepresentativeWorldError(
                f"official server exited before command dispatch: {self.process.returncode}"
            )
        self.stdin.write("\n".join(commands) + "\n")
        self.stdin.flush()

    def wait_for(self, marker: str, deadline: float, label: str) -> None:
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RepresentativeWorldError(
                    f"official server exited while waiting for {label}: {self.process.returncode}"
                    + ("\n" + "\n".join(self.tail) if self.tail else "")
                )
            try:
                line = self.lines.get(timeout=0.25)
            except queue.Empty:
                continue
            if not line:
                continue
            self._observe(line)
            if marker in line:
                return
        raise RepresentativeWorldError(
            f"timed out waiting for official server {label}: marker={marker}"
            + ("\n" + "\n".join(self.tail) if self.tail else "")
        )

    def wait_for_start(self, deadline: float) -> None:
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                break
            try:
                line = self.lines.get(timeout=0.25)
            except queue.Empty:
                continue
            if not line:
                continue
            self._observe(line)
            if "Done (" in line and 'For help, type "help"' in line:
                return
        raise RepresentativeWorldError(
            "official server did not reach completed startup"
            + ("\n" + "\n".join(self.tail) if self.tail else "")
        )

    def settle(self, seconds: int, deadline: float) -> None:
        settle_deadline = min(deadline, time.monotonic() + seconds)
        while time.monotonic() < settle_deadline:
            if self.process.poll() is not None:
                raise RepresentativeWorldError(
                    f"official server exited during representative chunk generation: {self.process.returncode}"
                )
            time.sleep(min(0.25, max(0.0, settle_deadline - time.monotonic())))
        if time.monotonic() >= deadline:
            raise RepresentativeWorldError("representative generation exhausted global timeout")

    def barrier(self, marker: str, deadline: float) -> None:
        # Commands execute serially on the server command path.  Seeing the marker means
        # the preceding synchronous `save-all flush` command has returned.
        self.send(["save-all flush", f"say {marker}"])
        self.wait_for(marker, deadline, f"save barrier {marker}")


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


def tickets_for_plan(plan: dict[str, object]) -> list[ChunkTicket]:
    dimensions = plan["dimensions"]
    assert isinstance(dimensions, dict)
    tickets: list[ChunkTicket] = []
    for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS:
        entry = dimensions[descriptor.key]
        assert isinstance(entry, dict)
        chunks = entry["chunks"]
        assert isinstance(chunks, list)
        tickets.extend(
            ChunkTicket(descriptor.key, int(chunk[0]), int(chunk[1]))
            for chunk in chunks
        )
    return tickets


def commands_for_plan(plan: dict[str, object]) -> list[str]:
    """Stable selection-command identity retained across generator mechanisms."""

    return [ticket.add_command() for ticket in tickets_for_plan(plan)]


def generation_batches(
    plan: dict[str, object],
    batch_size: int,
) -> list[GenerationBatch]:
    if batch_size <= 0:
        raise RepresentativeWorldError("generation batch size must be positive")
    dimensions = plan["dimensions"]
    assert isinstance(dimensions, dict)
    batches: list[GenerationBatch] = []
    batch_index = 0
    for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS:
        entry = dimensions[descriptor.key]
        assert isinstance(entry, dict)
        chunks = entry["chunks"]
        assert isinstance(chunks, list)
        tickets = [
            ChunkTicket(descriptor.key, int(chunk[0]), int(chunk[1]))
            for chunk in chunks
        ]
        for start in range(0, len(tickets), batch_size):
            batch_tickets = tuple(tickets[start : start + batch_size])
            batches.append(GenerationBatch(batch_index, descriptor.key, batch_tickets))
            batch_index += 1
    return batches


def command_digest(commands: list[str]) -> str:
    return hashlib.sha256(("\n".join(commands) + "\n").encode("utf-8")).hexdigest()


def expected_region_paths(world: Path, plan: dict[str, object]) -> list[Path]:
    dimensions = plan["dimensions"]
    assert isinstance(dimensions, dict)
    paths: set[Path] = set()
    for descriptor in section_representative_plan.REPRESENTATIVE_DIMENSIONS:
        entry = dimensions[descriptor.key]
        assert isinstance(entry, dict)
        chunks = entry["chunks"]
        assert isinstance(chunks, list)
        directory = world / descriptor.vanilla.region_path
        for chunk in chunks:
            chunk_x = int(chunk[0])
            chunk_z = int(chunk[1])
            paths.add(directory / f"r.{chunk_x // 32}.{chunk_z // 32}.mca")
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


def _batch_marker(batch: GenerationBatch, phase: str) -> str:
    safe_dimension = batch.dimension.replace(":", "_")
    return f"CRUCIBLE_REP_{batch.index:03d}_{safe_dimension}_{phase}"


def execute_batches(
    console: ServerConsole,
    plan: dict[str, object],
    *,
    batch_size: int,
    batch_settle_seconds: int,
    deadline: float,
) -> list[BatchTiming]:
    timings: list[BatchTiming] = []
    for batch in generation_batches(plan, batch_size):
        started = time.monotonic()
        console.send([ticket.add_command() for ticket in batch.tickets])
        added_marker = _batch_marker(batch, "ADDED")
        console.send([f"say {added_marker}"])
        console.wait_for(added_marker, deadline, f"batch {batch.index} ticket installation")

        console.settle(batch_settle_seconds, deadline)

        saved_marker = _batch_marker(batch, "SAVED")
        console.barrier(saved_marker, deadline)

        console.send([ticket.remove_command() for ticket in batch.tickets])
        removed_marker = _batch_marker(batch, "REMOVED")
        console.send([f"say {removed_marker}"])
        console.wait_for(removed_marker, deadline, f"batch {batch.index} ticket removal")

        timings.append(
            BatchTiming(
                index=batch.index,
                dimension=batch.dimension,
                ticket_count=len(batch.tickets),
                elapsed_ms=round((time.monotonic() - started) * 1000),
            )
        )

    final_marker = "CRUCIBLE_REPRESENTATIVE_FINAL_SAVE"
    console.barrier(final_marker, deadline)
    return timings


def run_server(
    *,
    server: Path,
    work_dir: Path,
    seed: int,
    plan: dict[str, object],
    timeout_seconds: int,
    batch_size: int,
    batch_settle_seconds: int,
) -> tuple[Path, list[BatchTiming]]:
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
    assert process.stdout is not None

    lines: queue.Queue[str] = queue.Queue()
    threading.Thread(
        target=_reader,
        args=(process.stdout, lines, work_dir / "server.log"),
        daemon=True,
    ).start()
    console = ServerConsole(process, lines)

    deadline = time.monotonic() + timeout_seconds
    try:
        console.wait_for_start(deadline)
        timings = execute_batches(
            console,
            plan,
            batch_size=batch_size,
            batch_settle_seconds=batch_settle_seconds,
            deadline=deadline,
        )
        console.send(["stop"])
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
    return world, timings


def generate(
    *,
    version: str,
    work_dir: Path,
    cache: Path,
    plan_path: Path,
    seed_index: int,
    timeout_seconds: int,
    batch_size: int,
    batch_settle_seconds: int,
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
    if batch_size <= 0:
        raise RepresentativeWorldError("generation batch size must be positive")
    if batch_settle_seconds < 0:
        raise RepresentativeWorldError("batch settle seconds must be non-negative")
    seed = int(seeds[seed_index])

    resolved, _ = official_state_data.resolve(version, cache)
    server, _ = resolved["server"]
    server_sha256 = official_state_data.sha256_file(server)
    world, timings = run_server(
        server=server,
        work_dir=work_dir,
        seed=seed,
        plan=plan,
        timeout_seconds=timeout_seconds,
        batch_size=batch_size,
        batch_settle_seconds=batch_settle_seconds,
    )

    if evidence_output is not None:
        commands = commands_for_plan(plan)
        batches = generation_batches(plan, batch_size)
        evidence_output.parent.mkdir(parents=True, exist_ok=True)
        evidence_output.write_text(
            json.dumps(
                {
                    "schema": 2,
                    "generator": GENERATOR_ID,
                    "minecraft_version": version,
                    "server_sha256": server_sha256,
                    "representative_policy": plan["policy"],
                    "plan_sha256": plan["plan_sha256"],
                    "seed_index": seed_index,
                    "seed": seed,
                    "selection_command_count": len(commands),
                    "selection_command_sha256": command_digest(commands),
                    "batch_size": batch_size,
                    "batch_count": len(batches),
                    "batch_settle_seconds": batch_settle_seconds,
                    "batch_timings": [
                        {
                            "index": timing.index,
                            "dimension": timing.dimension,
                            "ticket_count": timing.ticket_count,
                            "elapsed_ms": timing.elapsed_ms,
                        }
                        for timing in timings
                    ],
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
    parser.add_argument("--batch-size", type=int, default=DEFAULT_BATCH_SIZE)
    parser.add_argument(
        "--batch-settle-seconds", type=int, default=DEFAULT_BATCH_SETTLE_SECONDS
    )
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    if args.timeout_seconds <= 0:
        parser.error("--timeout-seconds must be positive")
    if args.batch_size <= 0:
        parser.error("--batch-size must be positive")
    if args.batch_settle_seconds < 0:
        parser.error("--batch-settle-seconds must be non-negative")

    try:
        world = generate(
            version=args.version,
            work_dir=args.work_dir,
            cache=args.cache,
            plan_path=args.plan,
            seed_index=args.seed_index,
            timeout_seconds=args.timeout_seconds,
            batch_size=args.batch_size,
            batch_settle_seconds=args.batch_settle_seconds,
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
