from __future__ import annotations

import importlib.util
import sqlite3
import sys
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1]


def _load(name: str, path: Path):  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


review = _load("r2b_play_entry_source_review", TOOLS / "r2b_play_entry_source_review.py")


class R2BPlayEntrySourceReviewTests(unittest.TestCase):
    def _inventory_db(self, signatures: list[tuple[str, int]]) -> sqlite3.Connection:
        conn = sqlite3.connect(":memory:")
        conn.execute("CREATE TABLE types(id INTEGER PRIMARY KEY, qualified_name TEXT NOT NULL)")
        conn.execute(
            """CREATE TABLE methods(
                   id INTEGER PRIMARY KEY,
                   type_id INTEGER NOT NULL,
                   signature TEXT NOT NULL,
                   param_count INTEGER NOT NULL,
                   is_constructor INTEGER NOT NULL
               )"""
        )
        conn.execute(
            "INSERT INTO types(id,qualified_name) VALUES(1,?)", (review.INVENTORY_MENU_TYPE,)
        )
        for index, (signature, param_count) in enumerate(signatures, start=1):
            conn.execute(
                "INSERT INTO methods(id,type_id,signature,param_count,is_constructor) VALUES(?,?,?,?,1)",
                (index, 1, signature, param_count),
            )
        return conn

    def test_final_frontier_reuses_both_historical_exact_reviews(self) -> None:
        self.assertEqual(len(review.first_pass.CANDIDATES), 27)
        self.assertEqual(len(review.followup.CANDIDATES), 35)
        conn = self._inventory_db(
            [("InventoryMenu(final Inventory inventory , final boolean active , final Player owner)", 3)]
        )
        selected = review.candidates(conn)
        conn.close()
        self.assertEqual(len(selected), 27 + 35 + 4 + 1)
        self.assertEqual(len({candidate.var_id for candidate in selected}), len(selected))
        self.assertEqual(
            len({review.selector_key(candidate) for candidate in selected}), len(selected)
        )

    def test_inventory_closure_matches_hardened_probe_exact_signatures(self) -> None:
        fixed = {candidate.var_id: candidate for candidate in review._fixed_closure_candidates()}
        self.assertEqual(
            fixed["DISC-NET-R2B-PLAY-INVENTORY-SET-SYNCHRONIZER-001"].exact_signature,
            "setSynchronizer(final ContainerSynchronizer synchronizer)",
        )
        self.assertEqual(
            fixed["DISC-NET-R2B-PLAY-INVENTORY-SEND-ALL-001"].exact_signature,
            "sendAllDataToRemote()",
        )
        self.assertEqual(
            fixed["DISC-NET-R2B-PLAY-INVENTORY-SYNCHRONIZER-FIELD-001"].exact_signature,
            "<fieldinit:containerSynchronizer>()",
        )
        self.assertEqual(
            fixed["DISC-NET-R2B-PLAY-INVENTORY-SYNCHRONIZER-CONTRACT-001"].exact_signature,
            "sendInitialData(AbstractContainerMenu container , List < ItemStack > slotItems , ItemStack carried , int [ ] dataSlots)",
        )

    def test_every_inventory_menu_constructor_is_selected_without_guessing_signature(self) -> None:
        signatures = [
            ("InventoryMenu(final Inventory inventory , final boolean active , final Player owner)", 3),
            ("InventoryMenu(final Inventory inventory , final Player owner)", 2),
        ]
        conn = self._inventory_db(signatures)
        dynamic = review._inventory_menu_candidates(conn)
        conn.close()
        self.assertEqual([candidate.exact_signature for candidate in dynamic], sorted(item[0] for item in signatures))
        self.assertTrue(all(candidate.type_name == review.INVENTORY_MENU_TYPE for candidate in dynamic))

    def test_missing_inventory_constructor_fails_closed(self) -> None:
        conn = self._inventory_db([])
        with self.assertRaises(review.R2BPlayEntryReviewError):
            review._inventory_menu_candidates(conn)
        conn.close()

    def test_final_frontier_stays_out_of_r2c_and_r3(self) -> None:
        conn = self._inventory_db(
            [("InventoryMenu(final Inventory inventory , final boolean active , final Player owner)", 3)]
        )
        selected = review.candidates(conn)
        conn.close()
        joined = "\n".join(
            f"{candidate.type_name}#{candidate.exact_signature or candidate.method_name}"
            for candidate in selected
        )
        for forbidden in (
            "ClientboundLevelChunkWithLightPacket",
            "ServerboundMovePlayerPacket",
            "LevelChunkSection",
            "ChunkMap",
        ):
            self.assertNotIn(forbidden, joined)

    def test_stale_partial_review_disposition_cannot_claim_final_closure(self) -> None:
        self.assertIn("DELEGATED_REVIEW_REQUIRED", review.ROUTE_DISPOSITIONS)
        self.assertEqual(review.REVIEW_ID, "REVIEW-NET-R2B-PLAY-ENTRY-FINAL-26_2-001")
        self.assertEqual(review.COMMIT_POLICY, "EPHEMERAL_DO_NOT_COMMIT")


if __name__ == "__main__":
    unittest.main()
