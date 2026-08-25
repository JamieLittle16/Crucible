from __future__ import annotations

import hashlib
import json
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
RECORD_DIR = REPO_ROOT / "vanilla/records/network/r1/configuration"
GATE = REPO_ROOT / "vanilla/gates/network/GATE-NET-CONFIG-26_2-001.json"
CHECKPOINT = REPO_ROOT / "vanilla/reports/r1b-direct-configuration-source-admission-26.2.json"

EXPECTED_GATE_SHA256 = "7a93465fd2d481ecf2c24ebb9ffdbeede90f6874bc44f61d635f7db684cbe8ae"
EXPECTED_RECORD_SHA256 = {
    "VAR-NET-R1B-BRAND-PAYLOAD-CODEC-001.json": "508062dce0f33975632ae0d4f14e42edde9d016173b2d4e006cc16adb740cad8",
    "VAR-NET-R1B-CB-CUSTOM-PAYLOAD-CODEC-001.json": "2c68248f5b38d1f5c084affdef57ef18f4741c030aeb7cb99063725620fae24d",
    "VAR-NET-R1B-CLIENT-INFO-HANDLER-001.json": "31e8fdd76d6907504f30a83f663993ebf9c0d2bd252869a13ab1f094a092ba6e",
    "VAR-NET-R1B-CLIENT-INFO-PACKET-CODEC-001.json": "2582388945a7938aba50659e05f40a1871b78a53d829b5cb3d8efb4f5a2a2195",
    "VAR-NET-R1B-CONFIG-FINISH-HANDLER-001.json": "916bff4606a7e2d2f48d28fb52b70eea464e94c6e09c23dd381e866ec2c25c20",
    "VAR-NET-R1B-CONFIG-REGISTRATION-001.json": "e79c44bc092a4b525dd74cedf678c01861d30c98708d924754490063b67a27a6",
    "VAR-NET-R1B-CONFIG-START-001.json": "ffa82ee627f455a6ab858f7613a7e33460805b00273ce665854969e2789bc56c",
    "VAR-NET-R1B-CUSTOM-PAYLOAD-HANDLER-001.json": "0ffe5b85020b4840895dfd4f18d19d8978a4850badc75032ba02fd8665794bc2",
    "VAR-NET-R1B-ENABLED-FEATURES-CODEC-001.json": "4652ca246bcfa194bed49f2bc56b3c382f122407deb64d5ffccbc185aeb10486",
    "VAR-NET-R1B-FINISH-CLIENTBOUND-CODEC-001.json": "87f85da975b6d8f1c8cf5c3388ac96f37d61fd048e41c593daafa71b3570d682",
    "VAR-NET-R1B-FINISH-SERVERBOUND-CODEC-001.json": "b472596f24798959770b05935d7b1a7596da8b8b4e6e4f5468fec17afea12b4b",
    "VAR-NET-R1B-JOIN-WORLD-START-001.json": "0b6258b9cfce9901ad66adb98321d2d41eecf76fe823aa81b2f1d89e1a5432b1",
    "VAR-NET-R1B-KNOWN-PACK-CODEC-001.json": "21a4d193da7b81aef2ec459a5bef9c7e44295c7fe81e7b1fdfd0c77b8171fb98",
    "VAR-NET-R1B-KNOWN-PACK-HANDLER-001.json": "29f1a7c0f09a4086f50a131073360d4f6521cd57eb7e0cb4d0f35993275dffdf",
    "VAR-NET-R1B-PLACE-NEW-PLAYER-001.json": "d9cb96a41a102c34210cf3e7c9400d770ece88e3de4170729b3769f08d21fa55",
    "VAR-NET-R1B-PREPARE-SPAWN-START-001.json": "beab1786d468f895272f0d9e41cbc10e10d083fb9459ce884ed4b078ec8acbe3",
    "VAR-NET-R1B-PREPARE-SPAWN-TICK-001.json": "bd698bfa02e45410e3ca330c51f24b9f04002e4cd214aa61096ab44634ba865d",
    "VAR-NET-R1B-REGISTRY-DATA-CODEC-001.json": "2f9097d2c713447baeb77ee4a8b73a7f2f7a954c8f090521df9a6088b62ee9f7",
    "VAR-NET-R1B-REGISTRY-SYNC-RESPONSE-001.json": "bbb8d9f62080facfb641e671420c59d3c93511eee2dcaf95d83e2b220bf02b04",
    "VAR-NET-R1B-REGISTRY-SYNC-SEND-001.json": "0ea6e3adfb91ad66656ad4079dd476d428c70599012a4a99e5589bd823152419",
    "VAR-NET-R1B-REGISTRY-SYNC-START-001.json": "04424dd0b7fea5276ea6ae36fa360f8df385fd7a71b81356c3cf3a2d84863040",
    "VAR-NET-R1B-SB-CUSTOM-PAYLOAD-CODEC-001.json": "188e1f214aa2b447dae4040e332cbca5bec0a60383ff42dc63ac1727eaa21201",
    "VAR-NET-R1B-SELECT-KNOWN-CB-CODEC-001.json": "c7f3d763a1f32bf9415610376cd1d68194902c8ef74fdce1bd7ddf0cf16bd13b",
    "VAR-NET-R1B-SELECT-KNOWN-SB-CODEC-001.json": "5d4cef0f1a08bd726545f0b79fa28e93fc0c445b12bb0efcc6e73eda1402c062",
    "VAR-NET-R1B-UPDATE-TAGS-CODEC-001.json": "e8d3f7ed47ace2650485d403f59c00547e44e03b5acf2f2cacae4da70de62eb8",
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class R1BDirectConfigurationAdmissionTests(unittest.TestCase):
    def test_exact_admitted_record_and_gate_bytes_are_preserved(self) -> None:
        self.assertEqual(sha256(GATE), EXPECTED_GATE_SHA256)
        actual = {path.name: sha256(path) for path in RECORD_DIR.glob("*.json")}
        self.assertEqual(actual, EXPECTED_RECORD_SHA256)

    def test_all_committed_records_are_reviewed_and_source_free(self) -> None:
        for name in EXPECTED_RECORD_SHA256:
            raw = (RECORD_DIR / name).read_text(encoding="utf-8")
            self.assertNotIn("source_excerpt", raw)
            record = json.loads(raw)
            self.assertEqual(record["status"], "VAR_REVIEWED")
            self.assertTrue(record["semantic_rules"])
            observed = set(record.get("hazards_reviewed", []))
            self.assertEqual(len(observed), len(record.get("hazards_reviewed", [])))

    def test_checkpoint_matches_pinned_26_2_admission(self) -> None:
        checkpoint = json.loads(CHECKPOINT.read_text(encoding="utf-8"))
        self.assertEqual(checkpoint["status"], "direct-configuration-gate-admitted")
        self.assertTrue(checkpoint["gate"]["admitted"])
        self.assertEqual(checkpoint["gate"]["failures"], [])
        self.assertEqual(checkpoint["gate"]["required_methods"], 25)
        self.assertEqual(checkpoint["gate"]["sha256"], EXPECTED_GATE_SHA256)
        self.assertEqual(checkpoint["source"]["minecraft"], "26.2")
        self.assertEqual(checkpoint["source"]["protocol"], 776)
        self.assertEqual(checkpoint["source"]["data_version"], 4903)
        self.assertEqual(checkpoint["source"]["archive_sha256"], "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750")

    def test_direct_admission_does_not_claim_r1b_completion(self) -> None:
        checkpoint = json.loads(CHECKPOINT.read_text(encoding="utf-8"))
        self.assertEqual(checkpoint["remaining_required_gates"], [
            "GATE-NET-CONFIG-CLOSURE-26_2-001",
            "GATE-NET-PLAY-ENTRY-26_2-001",
        ])


if __name__ == "__main__":
    unittest.main()
