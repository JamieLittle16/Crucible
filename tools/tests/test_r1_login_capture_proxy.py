import unittest

from tools.protocol_capture_proxy import CaptureError
from tools.r1_login_capture_proxy import LOGIN_INTENT, handshake_intent


def encode_var_int(value: int) -> bytes:
    output = bytearray()
    remaining = value
    while True:
        byte = remaining & 0x7F
        remaining >>= 7
        if remaining:
            output.append(byte | 0x80)
        else:
            output.append(byte)
            return bytes(output)


def handshake(*, intent: int, address: str = "127.0.0.1", port: int = 25566) -> bytes:
    address_bytes = address.encode("utf-8")
    return (
        encode_var_int(0)
        + encode_var_int(776)
        + encode_var_int(len(address_bytes))
        + address_bytes
        + port.to_bytes(2, "big")
        + encode_var_int(intent)
    )


class R1LoginCaptureProxyTests(unittest.TestCase):
    def test_distinguishes_status_and_login_handshakes(self) -> None:
        self.assertEqual(handshake_intent(handshake(intent=1)), 1)
        self.assertEqual(handshake_intent(handshake(intent=LOGIN_INTENT)), LOGIN_INTENT)

    def test_rejects_non_handshake_first_packet(self) -> None:
        body = bytearray(handshake(intent=LOGIN_INTENT))
        body[0] = 1
        with self.assertRaisesRegex(CaptureError, "handshake packet id must be 0"):
            handshake_intent(bytes(body))

    def test_rejects_truncated_and_trailing_handshake(self) -> None:
        body = handshake(intent=LOGIN_INTENT)
        with self.assertRaises(CaptureError):
            handshake_intent(body[:-1])
        with self.assertRaisesRegex(CaptureError, "trailing payload"):
            handshake_intent(body + b"\x00")

    def test_rejects_noncanonical_varint(self) -> None:
        body = handshake(intent=LOGIN_INTENT)
        noncanonical_packet_id = b"\x80\x00" + body[1:]
        with self.assertRaisesRegex(CaptureError, "noncanonical"):
            handshake_intent(noncanonical_packet_id)

    def test_rejects_address_encoded_beyond_source_bound(self) -> None:
        address = "a" * 766
        body = (
            encode_var_int(0)
            + encode_var_int(776)
            + encode_var_int(len(address))
            + address.encode("ascii")
            + (25566).to_bytes(2, "big")
            + encode_var_int(LOGIN_INTENT)
        )
        with self.assertRaisesRegex(CaptureError, "255-unit bound"):
            handshake_intent(body)


if __name__ == "__main__":
    unittest.main()
