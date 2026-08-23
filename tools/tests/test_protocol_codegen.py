import copy
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from tools.protocol_codegen import CodegenError, generate, render_rust
from tools.protocol_contract import ContractError


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


def packet(
    name: str,
    phase: str,
    direction: str,
    packet_id: int,
    payload: bytes,
    rule: str,
) -> dict[str, object]:
    body = encode_var_int(packet_id) + payload
    frame = encode_var_int(len(body)) + body
    return {
        "name": name,
        "phase": phase,
        "direction": direction,
        "id": packet_id,
        "semantic_rules": [rule],
        "source_records": ["VAR-PROTOCOL-CODEGEN-001"],
        "golden": {"body_hex": body.hex(), "frame_hex": frame.hex()},
    }


class ProtocolCodegenTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.lock = self.root / "vanilla.lock.toml"
        self.lock.write_text(LOCK_TEXT, encoding="utf-8")
        self.records = self.root / "records"
        self.records.mkdir()
        self.record = {
            "schema": 1,
            "id": "VAR-PROTOCOL-CODEGEN-001",
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
            "semantic_rules": [
                "SEM-PROTOCOL-CODEGEN-HANDSHAKE",
                "SEM-PROTOCOL-CODEGEN-STATUS-REQUEST",
                "SEM-PROTOCOL-CODEGEN-STATUS-RESPONSE",
                "SEM-PROTOCOL-CODEGEN-PING",
            ],
            "evidence": [],
            "notes": [],
        }
        (self.records / "VAR-PROTOCOL-CODEGEN-001.json").write_text(
            json.dumps(self.record, separators=(",", ":")), encoding="utf-8"
        )
        self.contract = {
            "schema": 1,
            "id": "PROTO-TEST-CODEGEN-001",
            "target": {
                "minecraft": "test-version",
                "protocol": 42,
                "source_archive_sha256": "a" * 64,
                "fingerprint_algorithm": "test-fingerprint-v1",
            },
            "packets": [
                packet(
                    "status-response",
                    "status",
                    "clientbound",
                    5,
                    b"response",
                    "SEM-PROTOCOL-CODEGEN-STATUS-RESPONSE",
                ),
                packet(
                    "handshake-intention",
                    "handshake",
                    "serverbound",
                    300,
                    b"\x2a\x00",
                    "SEM-PROTOCOL-CODEGEN-HANDSHAKE",
                ),
                packet(
                    "ping-request",
                    "status",
                    "serverbound",
                    9,
                    b"12345678",
                    "SEM-PROTOCOL-CODEGEN-PING",
                ),
                packet(
                    "status-request",
                    "status",
                    "serverbound",
                    1,
                    b"",
                    "SEM-PROTOCOL-CODEGEN-STATUS-REQUEST",
                ),
            ],
        }
        self.contract_path = self.root / "contract.json"
        self.output = self.root / "generated.rs"
        self._write_contract()

    def tearDown(self) -> None:
        self.temp.cleanup()

    def _write_contract(self, value: dict[str, object] | None = None) -> None:
        self.contract_path.write_text(
            json.dumps(self.contract if value is None else value, separators=(",", ":")),
            encoding="utf-8",
        )

    def _generate(self, *, check: bool = False) -> str:
        return generate(
            self.contract_path,
            lock_path=self.lock,
            records_root=self.records,
            output_path=self.output,
            check=check,
        )

    def test_admitted_contract_generates_only_static_runtime_constants(self) -> None:
        rendered = self._generate()
        runtime = rendered.split("#[cfg(test)]", 1)[0]
        self.assertIn("pub const PROTOCOL_VERSION: i32 = 42;", runtime)
        self.assertIn("pub mod handshake", runtime)
        self.assertIn("pub mod status", runtime)
        self.assertIn("pub const HANDSHAKE_INTENTION: i32 = 300;", runtime)
        self.assertIn("pub const STATUS_REQUEST: i32 = 1;", runtime)
        self.assertIn("pub const STATUS_RESPONSE: i32 = 5;", runtime)
        self.assertIn("pub const PING_REQUEST: i32 = 9;", runtime)

        for forbidden in (
            "HashMap",
            "BTreeMap",
            "dyn ",
            "Vec<",
            "Box<",
            "Arc<",
            "Mutex<",
            "OnceLock",
            "lazy_static",
        ):
            self.assertNotIn(forbidden, runtime)

        self.assertNotIn("response", runtime)
        golden = rendered.split("#[cfg(test)]", 1)[1]
        self.assertIn("STATUS_CLIENTBOUND_STATUS_RESPONSE_BODY", golden)
        self.assertIn("STATUS_SERVERBOUND_PING_REQUEST_FRAME", golden)

    def test_generation_is_canonical_under_packet_and_evidence_list_reordering(self) -> None:
        first = self._generate()
        reordered = copy.deepcopy(self.contract)
        reordered["packets"].reverse()
        for current in reordered["packets"]:
            current["semantic_rules"].reverse()
            current["source_records"].reverse()
        self._write_contract(reordered)
        second = render_rust(reordered)
        self.assertEqual(first, second)
        self.assertEqual(self._generate(check=True), first)

    def test_check_mode_detects_byte_drift_and_rejects_symlink_output(self) -> None:
        self._generate()
        self.output.write_text(self.output.read_text(encoding="utf-8") + "// drift\n", encoding="utf-8")
        with self.assertRaisesRegex(CodegenError, "drifted"):
            self._generate(check=True)

        self.output.unlink()
        target = self.root / "target.rs"
        target.write_text("safe", encoding="utf-8")
        self.output.symlink_to(target)
        with self.assertRaisesRegex(CodegenError, "unsafe"):
            self._generate(check=True)

    def test_invalid_contract_is_rejected_before_codegen(self) -> None:
        invalid = copy.deepcopy(self.contract)
        invalid["packets"][0]["id"] = 6
        self._write_contract(invalid)
        with self.assertRaises(ContractError):
            self._generate()
        self.assertFalse(self.output.exists())

    def test_generated_source_compiles_as_library_and_test_when_rustc_is_available(self) -> None:
        rustc = shutil.which("rustc")
        if rustc is None:
            self.skipTest("rustc is unavailable")
        self._generate()
        library = self.root / "generated.rlib"
        test_binary = self.root / "generated-test"
        for extra, output in [
            (["--crate-type=lib"], library),
            (["--test"], test_binary),
        ]:
            completed = subprocess.run(
                [rustc, "--edition=2024", *extra, str(self.output), "-o", str(output)],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_ascii_codegen_boundary_is_explicit(self) -> None:
        contract = copy.deepcopy(self.contract)
        contract["target"]["minecraft"] = "téšt"
        with self.assertRaisesRegex(CodegenError, "ASCII"):
            render_rust(contract)


if __name__ == "__main__":
    unittest.main()
