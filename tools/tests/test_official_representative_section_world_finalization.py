import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

import official_representative_section_world as worldgen


class FakeProcess:
    def __init__(self, *, returncode=None):
        self.returncode = returncode
        self.killed = False
        self.wait_timeouts = []

    def poll(self):
        return self.returncode

    def kill(self):
        self.killed = True
        self.returncode = -9

    def wait(self, timeout=None):
        self.wait_timeouts.append(timeout)
        return self.returncode


class FakeConsole:
    def __init__(self):
        self.events = []

    def send(self, commands):
        self.events.append(("send", tuple(commands)))

    def wait_for(self, marker, deadline, label):
        self.events.append(("wait", marker, deadline, label))

    def barrier(self, marker, deadline):
        self.events.append(("barrier", marker, deadline))


class FinalFlushFinalizationTests(unittest.TestCase):
    def test_quiesce_disables_autosave_then_flushes_again(self):
        console = FakeConsole()
        worldgen._quiesce_persistence(console, 123.0)
        self.assertEqual(
            console.events,
            [
                (
                    "send",
                    ("save-off", f"say {worldgen.SAVE_OFF_MARKER}"),
                ),
                (
                    "wait",
                    worldgen.SAVE_OFF_MARKER,
                    123.0,
                    "automatic-save disable barrier",
                ),
                (
                    "barrier",
                    worldgen.QUIESCENT_SAVE_MARKER,
                    123.0,
                ),
            ],
        )

    def test_finalizer_uses_kill_after_quiescent_flush_boundary(self):
        process = FakeProcess()
        worldgen._terminate_after_final_flush(process)
        self.assertTrue(process.killed)
        self.assertEqual(process.returncode, -9)
        self.assertEqual(process.wait_timeouts, [10])

    def test_finalizer_rejects_server_that_died_before_controlled_exit(self):
        process = FakeProcess(returncode=1)
        with self.assertRaisesRegex(
            worldgen.RepresentativeWorldError,
            "exited after final save barrier",
        ):
            worldgen._terminate_after_final_flush(process)
        self.assertFalse(process.killed)

    def test_aligned_region_file_is_left_byte_identical(self):
        with tempfile.TemporaryDirectory() as temporary:
            world = Path(temporary) / "world"
            region = world / "region" / "r.0.0.mca"
            region.parent.mkdir(parents=True)
            before = bytes((index * 17) % 251 for index in range(worldgen.SECTOR_BYTES * 3))
            region.write_bytes(before)

            record = worldgen._pad_region_file_tail(world, region)

            self.assertEqual(region.read_bytes(), before)
            self.assertEqual(
                record,
                {
                    "path": "region/r.0.0.mca",
                    "original_size": len(before),
                    "padding_bytes": 0,
                    "final_size": len(before),
                },
            )

    def test_unaligned_region_file_preserves_prefix_and_appends_only_zero_padding(self):
        with tempfile.TemporaryDirectory() as temporary:
            world = Path(temporary) / "world"
            region = world / "dimensions" / "minecraft" / "overworld" / "region" / "r.-1.0.mca"
            region.parent.mkdir(parents=True)
            before = bytes(
                (index * 29 + 7) % 251
                for index in range(worldgen.REGION_HEADER_BYTES + 137)
            )
            region.write_bytes(before)

            record = worldgen._pad_region_file_tail(world, region)
            after = region.read_bytes()
            padding = (-len(before)) % worldgen.SECTOR_BYTES

            self.assertEqual(after[: len(before)], before)
            self.assertEqual(after[len(before) :], b"\x00" * padding)
            self.assertEqual(len(after) % worldgen.SECTOR_BYTES, 0)
            self.assertEqual(record["original_size"], len(before))
            self.assertEqual(record["padding_bytes"], padding)
            self.assertEqual(record["final_size"], len(before) + padding)
            self.assertEqual(
                record["path"],
                "dimensions/minecraft/overworld/region/r.-1.0.mca",
            )

    def test_region_padding_rejects_too_small_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            world = Path(temporary) / "world"
            region = world / "region" / "r.0.0.mca"
            region.parent.mkdir(parents=True)
            region.write_bytes(b"\x00" * (worldgen.REGION_HEADER_BYTES - 1))

            with self.assertRaisesRegex(
                worldgen.RepresentativeWorldError,
                "smaller than the two-sector Anvil header",
            ):
                worldgen._pad_region_file_tail(world, region)

    def test_region_padding_rejects_symlink(self):
        with tempfile.TemporaryDirectory() as temporary:
            world = Path(temporary) / "world"
            region_dir = world / "region"
            region_dir.mkdir(parents=True)
            target = region_dir / "real.mca"
            target.write_bytes(b"\x00" * worldgen.REGION_HEADER_BYTES)
            link = region_dir / "r.0.0.mca"
            link.symlink_to(target.name)

            with self.assertRaisesRegex(
                worldgen.RepresentativeWorldError,
                "must not be a symlink",
            ):
                worldgen._pad_region_file_tail(world, link)

    def test_finalization_policy_is_explicit(self):
        self.assertEqual(
            worldgen.FINALIZATION_POLICY,
            "final-flush-save-off-flush-sigkill-pad-region-tail-v3",
        )
        self.assertEqual(worldgen.SECTOR_BYTES, 4096)
        self.assertEqual(worldgen.REGION_HEADER_BYTES, 8192)
        self.assertEqual(
            worldgen.SAVE_OFF_MARKER,
            "CRUCIBLE_REPRESENTATIVE_SAVE_OFF",
        )
        self.assertEqual(
            worldgen.QUIESCENT_SAVE_MARKER,
            "CRUCIBLE_REPRESENTATIVE_QUIESCENT_SAVE",
        )


if __name__ == "__main__":
    unittest.main()
