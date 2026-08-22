from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))

import official_representative_section_world as world
import section_representative_plan as plan
import vanilla_dimensions


class FakeConsole:
    def __init__(self) -> None:
        self.events: list[tuple[str, object]] = []

    def send(self, commands: list[str] | tuple[str, ...]) -> None:
        self.events.append(("send", tuple(commands)))

    def wait_for(self, marker: str, deadline: float, label: str) -> None:
        del deadline
        self.events.append(("wait", (marker, label)))

    def settle(self, seconds: int, deadline: float) -> None:
        del deadline
        self.events.append(("settle", seconds))

    def barrier(self, marker: str, deadline: float) -> None:
        del deadline
        self.events.append(("barrier", marker))


class OfficialRepresentativeSectionWorldTests(unittest.TestCase):
    def test_server_properties_bind_seed_and_enable_all_dimensions(self) -> None:
        properties = world.server_properties(123).splitlines()
        self.assertIn("level-seed=123", properties)
        self.assertIn("allow-nether=true", properties)
        self.assertIn("sync-chunk-writes=true", properties)
        self.assertIn("max-tick-time=-1", properties)
        self.assertNotIn("allow-nether=false", properties)

    def test_dimension_descriptors_are_the_only_world_topology_source(self) -> None:
        self.assertEqual(
            tuple(descriptor.key for descriptor in vanilla_dimensions.STANDARD_DIMENSIONS),
            plan.DIMENSIONS,
        )
        for representative in plan.REPRESENTATIVE_DIMENSIONS:
            self.assertIs(
                representative.vanilla,
                vanilla_dimensions.require_standard_dimension(representative.key),
            )
            self.assertTrue(representative.vanilla.region_path.parts)

    def test_forceload_commands_are_exact_and_plan_bound(self) -> None:
        built = plan.build_plan()
        commands = world.commands_for_plan(built)
        self.assertEqual(len(commands), 192)
        self.assertEqual(
            world.command_digest(commands),
            "cb97b7490c28e38293251561749a87dbda2d0f78d78c7cf98471e5eff825a354",
        )
        self.assertEqual(
            commands[0],
            "execute in minecraft:overworld run forceload add 0 0",
        )
        self.assertEqual(
            commands[64],
            "execute in minecraft:the_nether run forceload add 0 0",
        )
        self.assertEqual(
            commands[128],
            "execute in minecraft:the_end run forceload add 0 0",
        )
        self.assertTrue(
            all(command.startswith("execute in minecraft:") for command in commands)
        )

    def test_chunk_tickets_have_exact_inverse_commands(self) -> None:
        tickets = world.tickets_for_plan(plan.build_plan())
        self.assertEqual(len(tickets), 192)
        identities = {(ticket.dimension, ticket.chunk_x, ticket.chunk_z) for ticket in tickets}
        self.assertEqual(len(identities), 192)
        for ticket in tickets:
            self.assertEqual(
                ticket.remove_command(),
                ticket.add_command().replace("forceload add", "forceload remove", 1),
            )

    def test_generation_batches_are_bounded_dimension_local_and_complete(self) -> None:
        built = plan.build_plan()
        batches = world.generation_batches(built, 8)
        self.assertEqual(len(batches), 24)
        self.assertEqual([batch.index for batch in batches], list(range(24)))
        flattened = []
        for batch in batches:
            self.assertGreater(len(batch.tickets), 0)
            self.assertLessEqual(len(batch.tickets), 8)
            self.assertTrue(all(ticket.dimension == batch.dimension for ticket in batch.tickets))
            flattened.extend(batch.tickets)
        self.assertEqual(flattened, world.tickets_for_plan(built))
        self.assertEqual([batch.dimension for batch in batches[:8]], ["minecraft:overworld"] * 8)
        self.assertEqual(
            [batch.dimension for batch in batches[8:16]],
            ["minecraft:the_nether"] * 8,
        )
        self.assertEqual([batch.dimension for batch in batches[16:]], ["minecraft:the_end"] * 8)

    def test_invalid_generation_batch_size_fails_closed(self) -> None:
        for invalid in (0, -1, -128):
            with self.assertRaises(world.RepresentativeWorldError):
                world.generation_batches(plan.build_plan(), invalid)

    def test_batch_orchestrator_orders_every_phase_and_final_barrier(self) -> None:
        built = plan.build_plan()
        console = FakeConsole()
        timings = world.execute_batches(
            console,  # type: ignore[arg-type]
            built,
            batch_size=64,
            batch_settle_seconds=0,
            deadline=10**12,
        )
        self.assertEqual(len(timings), 3)
        self.assertEqual([timing.dimension for timing in timings], list(plan.DIMENSIONS))
        self.assertTrue(all(timing.ticket_count == 64 for timing in timings))

        events = console.events
        cursor = 0
        for batch in world.generation_batches(built, 64):
            kind, add_commands = events[cursor]
            self.assertEqual(kind, "send")
            self.assertEqual(
                add_commands,
                tuple(ticket.add_command() for ticket in batch.tickets),
            )
            cursor += 1

            self.assertEqual(events[cursor][0], "send")
            added_command = events[cursor][1][0]
            self.assertTrue(added_command.startswith("say "))
            added_marker = added_command.removeprefix("say ")
            self.assertIn("_ADDED", added_marker)
            cursor += 1
            self.assertEqual(events[cursor][0], "wait")
            self.assertEqual(events[cursor][1][0], added_marker)
            cursor += 1
            self.assertEqual(events[cursor], ("settle", 0))
            cursor += 1

            self.assertEqual(events[cursor][0], "barrier")
            self.assertIn("_SAVED", events[cursor][1])
            cursor += 1

            self.assertEqual(events[cursor][0], "send")
            self.assertEqual(
                events[cursor][1],
                tuple(ticket.remove_command() for ticket in batch.tickets),
            )
            cursor += 1
            self.assertEqual(events[cursor][0], "send")
            removed_command = events[cursor][1][0]
            self.assertTrue(removed_command.startswith("say "))
            removed_marker = removed_command.removeprefix("say ")
            self.assertIn("_REMOVED", removed_marker)
            cursor += 1
            self.assertEqual(events[cursor][0], "wait")
            self.assertEqual(events[cursor][1][0], removed_marker)
            cursor += 1

        self.assertEqual(
            events[cursor],
            ("barrier", "CRUCIBLE_REPRESENTATIVE_FINAL_SAVE"),
        )
        self.assertEqual(cursor + 1, len(events))

    def test_region_postcondition_covers_every_planned_chunk_region(self) -> None:
        built = plan.build_plan()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            paths = world.expected_region_paths(root, built)
            self.assertGreater(len(paths), 0)
            relative = {path.relative_to(root).as_posix() for path in paths}
            for descriptor in vanilla_dimensions.STANDARD_DIMENSIONS:
                self.assertTrue(
                    any(
                        item.startswith(descriptor.region_path.as_posix() + "/r.")
                        for item in relative
                    ),
                    descriptor.key,
                )

            expected = set()
            dimensions = built["dimensions"]
            for descriptor in vanilla_dimensions.STANDARD_DIMENSIONS:
                for chunk_x, chunk_z in dimensions[descriptor.key]["chunks"]:
                    expected.add(
                        (
                            root
                            / descriptor.region_path
                            / f"r.{chunk_x // 32}.{chunk_z // 32}.mca"
                        ).relative_to(root).as_posix()
                    )
            self.assertEqual(relative, expected)

    def test_command_schedule_has_no_duplicate_dimension_chunk(self) -> None:
        commands = world.commands_for_plan(plan.build_plan())
        self.assertEqual(len(commands), len(set(commands)))


if __name__ == "__main__":
    unittest.main()
