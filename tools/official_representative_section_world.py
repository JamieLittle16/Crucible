#!/usr/bin/env python3
"""Generate one representative world and finalize its persisted region containers.

The full generation implementation lives in `official_representative_section_world_impl`. This
wrapper changes only process/container finalization after the implementation has completed its
normal final synchronous `save-all flush` barrier.

Normal vanilla `stop` performs another large world save and exhausted the hosted runner heap. A
controlled SIGKILL after a quiescent `save-off` + `save-all flush` prevents that redundant semantic
save, but it also bypasses `RegionFile.close()`. Minecraft 26.2 still has a close-time
`padToFullSector()` boundary, so the wrapper reproduces only that physical append-only finalization:
selected region files receive zero bytes up to the next 4096-byte sector boundary before the
unchanged corpus extractor reads them.
"""

from __future__ import annotations

import json
import os
import queue
import subprocess
import sys
import threading
import time
from pathlib import Path

import official_representative_section_world_impl as _impl

SECTOR_BYTES = 4096
REGION_HEADER_BYTES = SECTOR_BYTES * 2
FINALIZATION_POLICY = "final-flush-save-off-flush-sigkill-pad-region-tail-v3"
SAVE_OFF_MARKER = "CRUCIBLE_REPRESENTATIVE_SAVE_OFF"
QUIESCENT_SAVE_MARKER = "CRUCIBLE_REPRESENTATIVE_QUIESCENT_SAVE"

_REGION_FINALIZATION_RECORDS: list[dict[str, object]] = []


def __getattr__(name: str):
    """Forward existing helper/test API to the preserved implementation module."""

    return getattr(_impl, name)


def _quiesce_persistence(console: _impl.ServerConsole, deadline: float) -> None:
    """Disable later autosaves and establish a final synchronous persistence barrier."""

    # The implementation has already completed its normal final save-all flush. Disable automatic
    # saving before asking for the final quiescent flush so no subsequent server tick can begin a
    # new semantic save between the admitted barrier and controlled termination.
    console.send(["save-off", f"say {SAVE_OFF_MARKER}"])
    console.wait_for(SAVE_OFF_MARKER, deadline, "automatic-save disable barrier")
    console.barrier(QUIESCENT_SAVE_MARKER, deadline)


def _terminate_after_final_flush(process: subprocess.Popen[str]) -> None:
    """End the oracle after the admitted quiescent synchronous persistence barrier."""

    if process.poll() is not None:
        raise _impl.RepresentativeWorldError(
            "official server exited after final save barrier before controlled finalization"
        )
    # SIGTERM may execute JVM shutdown hooks. SIGKILL is deliberate here: automatic saving has been
    # disabled and the final synchronous save barrier completed, so shutdown-time semantic saving is
    # outside the evidence path and must not be allowed to restart persistence work.
    process.kill()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired as error:
        raise _impl.RepresentativeWorldError(
            "official server did not terminate after final save barrier"
        ) from error


def _region_relative_path(world: Path, path: Path) -> str:
    """Return one canonical world-relative path and reject any path escape."""

    world_root = world.resolve()
    try:
        resolved = path.resolve(strict=True)
        relative = resolved.relative_to(world_root)
    except (FileNotFoundError, ValueError) as error:
        raise _impl.RepresentativeWorldError(
            f"selected region path escapes or is missing from generated world: {path}"
        ) from error
    if path.is_symlink():
        raise _impl.RepresentativeWorldError(
            f"selected region file must not be a symlink: {path}"
        )
    return relative.as_posix()


def _pad_region_file_tail(world: Path, path: Path) -> dict[str, object]:
    """Append only zero padding required to reproduce close-time full-sector finalization."""

    relative = _region_relative_path(world, path)
    if not path.is_file():
        raise _impl.RepresentativeWorldError(
            f"selected region path is not a regular file: {path}"
        )

    original_size = path.stat().st_size
    if original_size < REGION_HEADER_BYTES:
        raise _impl.RepresentativeWorldError(
            f"selected region file is smaller than the two-sector Anvil header: {path}"
        )

    padding_bytes = (-original_size) % SECTOR_BYTES
    if padding_bytes:
        # Binary append mode is intentional: this phase must be physically incapable of rewriting
        # an existing header, timestamp, location entry, compressed payload, or NBT byte.
        with path.open("ab") as handle:
            handle.write(b"\x00" * padding_bytes)
            handle.flush()
            os.fsync(handle.fileno())

    final_size = path.stat().st_size
    expected_size = original_size + padding_bytes
    if final_size != expected_size or final_size % SECTOR_BYTES != 0:
        raise _impl.RepresentativeWorldError(
            "region physical finalization did not produce the exact expected sector-aligned size: "
            f"{path}: before={original_size}, padding={padding_bytes}, after={final_size}"
        )

    return {
        "path": relative,
        "original_size": original_size,
        "padding_bytes": padding_bytes,
        "final_size": final_size,
    }


def _finalize_selected_region_files(
    world: Path, plan: dict[str, object]
) -> list[dict[str, object]]:
    """Reproduce only selected `RegionFile` close-time sector-tail padding."""

    selected = sorted(
        set(_impl.expected_region_paths(world, plan)),
        key=lambda path: path.as_posix(),
    )
    if not selected:
        raise _impl.RepresentativeWorldError(
            "representative plan resolved to no selected region files"
        )
    return [_pad_region_file_tail(world, path) for path in selected]


def run_server(
    *,
    server: Path,
    work_dir: Path,
    seed: int,
    plan: dict[str, object],
    timeout_seconds: int,
    batch_size: int,
    batch_settle_seconds: int,
) -> tuple[Path, list[_impl.BatchTiming]]:
    """Generate/persist one world, quiesce persistence, and finalize region containers."""

    global _REGION_FINALIZATION_RECORDS
    _REGION_FINALIZATION_RECORDS = []

    world = work_dir / "world"
    if world.exists():
        raise _impl.RepresentativeWorldError(
            f"representative world already exists: {world}"
        )
    work_dir.mkdir(parents=True, exist_ok=True)
    (work_dir / "eula.txt").write_text("eula=true\n", encoding="utf-8")
    (work_dir / "server.properties").write_text(
        _impl.server_properties(seed), encoding="utf-8"
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
        target=_impl._reader,
        args=(process.stdout, lines, work_dir / "server.log"),
        daemon=True,
    ).start()
    console = _impl.ServerConsole(process, lines)

    deadline = time.monotonic() + timeout_seconds
    try:
        console.wait_for_start(deadline)
        timings = _impl.execute_batches(
            console,
            plan,
            batch_size=batch_size,
            batch_settle_seconds=batch_settle_seconds,
            deadline=deadline,
        )
        _quiesce_persistence(console, deadline)
        _terminate_after_final_flush(process)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)

    level_dat = world / "level.dat"
    if not level_dat.is_file():
        raise _impl.RepresentativeWorldError("official server produced no level.dat")
    missing_regions = [
        path for path in _impl.expected_region_paths(world, plan) if not path.is_file()
    ]
    if missing_regions:
        raise _impl.RepresentativeWorldError(
            "official server did not materialize all selected region files: "
            + ", ".join(str(path) for path in missing_regions[:12])
        )

    _REGION_FINALIZATION_RECORDS = _finalize_selected_region_files(world, plan)
    return world, timings


def _evidence_path(argv: list[str]) -> Path | None:
    try:
        index = argv.index("--evidence")
    except ValueError:
        return None
    if index + 1 >= len(argv):
        return None
    return Path(argv[index + 1])


def main() -> int:
    # `generate` resolves `run_server` through the implementation module's globals, so replace only
    # that boundary. All plan, batching, command, extraction and semantic evidence logic remains
    # unchanged.
    _impl.run_server = run_server
    status = _impl.main()
    if status != 0:
        return status

    evidence_path = _evidence_path(sys.argv[1:])
    if evidence_path is not None and evidence_path.is_file():
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        if not isinstance(evidence, dict):
            raise _impl.RepresentativeWorldError(
                "representative world evidence must be a JSON object"
            )
        evidence["finalization_policy"] = FINALIZATION_POLICY
        evidence["region_container_finalization"] = list(_REGION_FINALIZATION_RECORDS)
        evidence_path.write_text(
            json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
