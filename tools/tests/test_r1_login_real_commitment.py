import hashlib
import json
import unittest
from pathlib import Path

from tools import r1_login_contract as login


ROOT = Path(__file__).resolve().parents[2]
COMMITMENT = ROOT / "vanilla/fixtures/protocol/26.2-login-capture-commitment.json"
CONTRACT = ROOT / "vanilla/protocol/PROTO-NET-LOGIN-26-2-001.json"
WITNESS = ROOT / "vanilla/reports/r1a2-login-witness-26.2.json"


class R1LoginRealCommitmentTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.commitment = json.loads(COMMITMENT.read_text(encoding="utf-8"))
        cls.contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
        cls.witness = json.loads(WITNESS.read_text(encoding="utf-8"))

    def test_prefix_frames_are_self_hashing_and_match_contract(self) -> None:
        prefix = self.commitment["login_prefix"]
        client = prefix["client-to-server"]
        server = prefix["server-to-client"]

        expected = {
            "client-intention-login": client[0],
            "login-hello": client[1],
            "login-acknowledged": client[2],
            "login-finished": server[0],
        }

        for packet in self.contract["packets"]:
            frame = expected[packet["name"]]
            raw = bytes.fromhex(frame["frame_hex"])
            self.assertEqual(hashlib.sha256(raw).hexdigest(), frame["frame_sha256"])
            self.assertEqual(packet["golden"]["frame_hex"], frame["frame_hex"])
            self.assertEqual(packet["golden"]["body_hex"], frame["body_hex"])

    def test_real_prefix_replays_source_admitted_login_law(self) -> None:
        prefix = self.commitment["login_prefix"]
        handshake = bytes.fromhex(prefix["client-to-server"][0]["body_hex"])
        hello = bytes.fromhex(prefix["client-to-server"][1]["body_hex"])
        ack = bytes.fromhex(prefix["client-to-server"][2]["body_hex"])
        finished_body = bytes.fromhex(prefix["server-to-client"][0]["body_hex"])

        login._check_handshake(handshake)  # noqa: SLF001
        player_name, client_uuid = login._check_hello(hello)  # noqa: SLF001
        finished = login._check_login_finished(finished_body)  # noqa: SLF001
        login._check_login_ack(ack)  # noqa: SLF001

        self.assertEqual(player_name, self.witness["player_name"])
        self.assertEqual(login._uuid_text(client_uuid), self.witness["client_hello_uuid"])  # noqa: SLF001

        offline_uuid = login.offline_player_uuid(player_name)
        self.assertEqual(login._uuid_text(offline_uuid), self.witness["offline_profile_uuid"])  # noqa: SLF001
        self.assertEqual(finished["profile_uuid"], offline_uuid)
        self.assertEqual(finished["profile_name"], player_name)
        self.assertEqual(len(finished["properties"]), self.witness["profile_property_count"])
        self.assertEqual(
            login._uuid_text(finished["session_uuid"]),  # noqa: SLF001
            self.witness["session_uuid"],
        )

    def test_full_capture_commitment_and_tail_counts_are_bound(self) -> None:
        self.assertEqual(
            self.commitment["capture_sha256"], self.witness["capture_sha256"]
        )
        self.assertEqual(self.contract["id"], self.witness["contract_id"])
        self.assertEqual(self.contract["target"], self.commitment["target"])

        streams = {item["direction"]: item for item in self.commitment["streams"]}
        tail = self.witness["uninterpreted_post_login_frames"]
        self.assertEqual(
            streams["client-to-server"]["frame_count"] - 3,
            tail["client_to_server_after_login"],
        )
        self.assertEqual(
            streams["server-to-client"]["frame_count"] - 1,
            tail["server_to_client_after_login_finished"],
        )

        archive = self.commitment["archive"]
        self.assertEqual(archive["format"], "zip")
        self.assertEqual(archive["member"], "crucible-r1a2-login-capture.json")
        self.assertEqual(archive["archive_bytes"], 2_174_431)
        self.assertEqual(archive["member_bytes"], 25_174_268)
        self.assertEqual(
            archive["sha256"],
            "1af7943df7b97763654294cf9f7b97dcdf1b051b1aa70ff7c52f1105e2dc8d8f",
        )
        self.assertEqual(
            archive["member_sha256"],
            "4ce3858d39e2501f92842e7c7cddd39c5a53037c12d47bed6e4f3000153d9c5f",
        )


if __name__ == "__main__":
    unittest.main()
