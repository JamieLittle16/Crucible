import copy
import json
import tempfile
import unittest
from pathlib import Path

from tools.protocol_contract import ContractError, validate_contract


LOCK_TEXT = """schema = 1
minecraft = "test-version"
protocol = 42
data_version = 7

[source]
kind = "test-source"
archive_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
java_files = 1

[atlas]
schema = 1
version = "test"
fingerprint_algorithm = "test-fingerprint-v1"
database = ".test/atlas.sqlite"
"""


def encode_var_int(value: int) -> bytes:
    result = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            result.append(byte | 0x80)
        else:
            result.append(byte)
            return bytes(result)


class ProtocolContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.lock = self.root / "vanilla.lock.toml"
        self.lock.write_text(LOCK_TEXT, encoding="utf-8")
        self.records = self.root / "records"
        self.records.mkdir()
        self.record_path = self.records / "VAR-PROTOCOL-TEST-001.json"
        self.record = {
            "schema": 1,
            "id": "VAR-PROTOCOL-TEST-001",
            "status": "VAR_REVIEWED",
            "source": {
                "type": "net.minecraft.test.Packet",
                "signature": "test()",
                "fingerprint_algorithm": "test-fingerprint-v1",
                "normalized_sha256": "b" * 64,
                "body_sha256": "c" * 64,
            },
            "classifications": ["SEMANTIC_NETWORK"],
            "hazards_reviewed": [],
            "semantic_rules": ["SEM-PROTOCOL-TEST-001"],
            "evidence": [],
            "notes": [],
        }
        self._write_record()

        packet_id = 300
        body = encode_var_int(packet_id) + b"\x01\x02\xfe"
        frame = encode_var_int(len(body)) + body
        self.contract = {
            "schema": 1,
            "id": "PROTO-TEST-STATUS-001",
            "target": {
                "minecraft": "test-version",
                "protocol": 42,
                "source_archive_sha256": "a" * 64,
                "fingerprint_algorithm": "test-fingerprint-v1",
            },
            "packets": [
                {
                    "name": "status-request",
                    "phase": "status",
                    "direction": "serverbound",
                    "id": packet_id,
                    "semantic_rules": ["SEM-PROTOCOL-TEST-001"],
                    "source_records": ["VAR-PROTOCOL-TEST-001"],
                    "golden": {
                        "body_hex": body.hex(),
                        "frame_hex": frame.hex(),
                    },
                }
            ],
        }
        self.contract_path = self.root / "contract.json"

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _write_record(self) -> None:
        self.record_path.write_text(
            json.dumps(self.record, separators=(",", ":")), encoding="utf-8"
        )

    def _write_contract(self, contract: dict | None = None) -> None:
        value = self.contract if contract is None else contract
        self.contract_path.write_text(
            json.dumps(value, separators=(",", ":")), encoding="utf-8"
        )

    def _validate(self, contract: dict | None = None) -> dict[str, object]:
        self._write_contract(contract)
        return validate_contract(
            self.contract_path,
            lock_path=self.lock,
            records_root=self.records,
        )

    def test_valid_contract_is_admitted_with_compact_summary(self) -> None:
        self.assertEqual(
            self._validate(),
            {
                "schema": 1,
                "id": "PROTO-TEST-STATUS-001",
                "minecraft": "test-version",
                "protocol": 42,
                "packets": 1,
            },
        )

    def test_unknown_fields_fail_closed_at_each_artifact_layer(self) -> None:
        mutations = [
            (("extra",), True),
            (("target", "extra"), True),
            (("packets", 0, "extra"), True),
            (("packets", 0, "golden", "extra"), True),
        ]
        for path, value in mutations:
            with self.subTest(path=path):
                contract = copy.deepcopy(self.contract)
                cursor = contract
                for key in path[:-1]:
                    cursor = cursor[key]
                cursor[path[-1]] = value
                with self.assertRaises(ContractError):
                    self._validate(contract)

    def test_target_identity_must_match_lock_exactly(self) -> None:
        fields = {
            "minecraft": "other-version",
            "protocol": 43,
            "source_archive_sha256": "d" * 64,
            "fingerprint_algorithm": "other-fingerprint",
        }
        for field, value in fields.items():
            with self.subTest(field=field):
                contract = copy.deepcopy(self.contract)
                contract["target"][field] = value
                with self.assertRaisesRegex(ContractError, "does not match vanilla lock"):
                    self._validate(contract)

    def test_boolean_cannot_masquerade_as_integer_schema_or_protocol(self) -> None:
        for field_path in [("schema",), ("target", "protocol")]:
            with self.subTest(field_path=field_path):
                contract = copy.deepcopy(self.contract)
                cursor = contract
                for key in field_path[:-1]:
                    cursor = cursor[key]
                cursor[field_path[-1]] = True
                with self.assertRaises(ContractError):
                    self._validate(contract)

    def test_source_records_must_exist_be_reviewed_and_match_fingerprint_policy(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["packets"][0]["source_records"] = ["VAR-PROTOCOL-MISSING-001"]
        with self.assertRaisesRegex(ContractError, "missing source record"):
            self._validate(contract)

        for mutation, expected in [
            (("status", "NEEDS_REVIEW"), "not VAR_REVIEWED"),
            (("fingerprint_algorithm", "wrong"), "fingerprint algorithm"),
            (("normalized_sha256", "bad"), "normalized_sha256"),
            (("body_sha256", "bad"), "body_sha256"),
        ]:
            with self.subTest(mutation=mutation):
                self.record = copy.deepcopy(self.record)
                field, value = mutation
                if field == "status":
                    self.record[field] = value
                else:
                    self.record["source"][field] = value
                self._write_record()
                with self.assertRaisesRegex(ContractError, expected):
                    self._validate()
                self.setUp_record_defaults()

    def setUp_record_defaults(self) -> None:
        self.record = {
            "schema": 1,
            "id": "VAR-PROTOCOL-TEST-001",
            "status": "VAR_REVIEWED",
            "source": {
                "type": "net.minecraft.test.Packet",
                "signature": "test()",
                "fingerprint_algorithm": "test-fingerprint-v1",
                "normalized_sha256": "b" * 64,
                "body_sha256": "c" * 64,
            },
            "classifications": ["SEMANTIC_NETWORK"],
            "hazards_reviewed": [],
            "semantic_rules": ["SEM-PROTOCOL-TEST-001"],
            "evidence": [],
            "notes": [],
        }
        self._write_record()

    def test_packet_semantic_rules_must_be_linked_by_cited_var_records(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["packets"][0]["semantic_rules"] = ["SEM-PROTOCOL-OTHER-001"]
        with self.assertRaisesRegex(ContractError, "not linked by cited VAR records"):
            self._validate(contract)

    def test_duplicate_names_and_phase_direction_identities_are_rejected(self) -> None:
        for change_name in (False, True):
            with self.subTest(change_name=change_name):
                contract = copy.deepcopy(self.contract)
                duplicate = copy.deepcopy(contract["packets"][0])
                if change_name:
                    duplicate["name"] = "status-request-alias"
                else:
                    duplicate["id"] = 301
                    body = encode_var_int(301) + b"\x01\x02\xfe"
                    duplicate["golden"] = {
                        "body_hex": body.hex(),
                        "frame_hex": (encode_var_int(len(body)) + body).hex(),
                    }
                contract["packets"].append(duplicate)
                expected = "duplicate packet identity" if change_name else "duplicate packet name"
                with self.assertRaisesRegex(ContractError, expected):
                    self._validate(contract)

    def test_golden_hex_is_canonical_and_packet_id_must_match(self) -> None:
        mutations = [
            ("body_hex", "0A"),
            ("body_hex", "0a 01"),
            ("body_hex", "0"),
            ("frame_hex", "zz"),
        ]
        for field, value in mutations:
            with self.subTest(field=field, value=value):
                contract = copy.deepcopy(self.contract)
                contract["packets"][0]["golden"][field] = value
                with self.assertRaises(ContractError):
                    self._validate(contract)

        contract = copy.deepcopy(self.contract)
        contract["packets"][0]["id"] = 301
        with self.assertRaisesRegex(ContractError, "does not match golden body id"):
            self._validate(contract)

    def test_noncanonical_varints_and_frame_disagreement_are_rejected(self) -> None:
        original = self.contract["packets"][0]
        body = bytes.fromhex(original["golden"]["body_hex"])

        contract = copy.deepcopy(self.contract)
        noncanonical_body = b"\xac\x82\x00" + body[2:]
        contract["packets"][0]["golden"] = {
            "body_hex": noncanonical_body.hex(),
            "frame_hex": (encode_var_int(len(noncanonical_body)) + noncanonical_body).hex(),
        }
        with self.assertRaisesRegex(ContractError, "noncanonical VarInt"):
            self._validate(contract)

        contract = copy.deepcopy(self.contract)
        frame = bytes.fromhex(original["golden"]["frame_hex"])
        contract["packets"][0]["golden"]["frame_hex"] = (b"\x86\x00" + frame[1:]).hex()
        with self.assertRaisesRegex(ContractError, "noncanonical VarInt"):
            self._validate(contract)

        contract = copy.deepcopy(self.contract)
        contract["packets"][0]["golden"]["frame_hex"] = (
            encode_var_int(len(body) + 1) + body
        ).hex()
        with self.assertRaisesRegex(ContractError, "frame length"):
            self._validate(contract)

        contract = copy.deepcopy(self.contract)
        altered = bytearray(body)
        altered[-1] ^= 0xFF
        contract["packets"][0]["golden"]["frame_hex"] = (
            encode_var_int(len(altered)) + altered
        ).hex()
        with self.assertRaisesRegex(ContractError, "frame body does not match"):
            self._validate(contract)

    def test_packet_metadata_is_strict_and_nonempty(self) -> None:
        mutations = [
            ("phase", "future"),
            ("direction", "sideways"),
            ("id", -1),
            ("id", 0x8000_0000),
            ("name", "Status Request"),
            ("semantic_rules", []),
            ("source_records", []),
        ]
        for field, value in mutations:
            with self.subTest(field=field):
                contract = copy.deepcopy(self.contract)
                contract["packets"][0][field] = value
                with self.assertRaises(ContractError):
                    self._validate(contract)


if __name__ == "__main__":
    unittest.main()
