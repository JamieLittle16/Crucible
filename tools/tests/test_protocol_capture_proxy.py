import json
import socket
import tempfile
import threading
import unittest
from pathlib import Path

from tools.protocol_capture_proxy import (
    CaptureError,
    FrameStreamCapture,
    _pump,
    _read_target,
    build_artifact,
    render_artifact,
)


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
    encoded = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            encoded.append(byte | 0x80)
        else:
            encoded.append(byte)
            return bytes(encoded)


def frame(body: bytes) -> bytes:
    return encode_var_int(len(body)) + body


def capture_for(data: bytes, chunks: list[int]) -> FrameStreamCapture:
    capture = FrameStreamCapture(max_frame_bytes=4_096, max_stream_bytes=65_536, max_frames=32)
    cursor = 0
    for width in chunks:
        if cursor >= len(data):
            break
        capture.feed(data[cursor : cursor + width])
        cursor += width
    if cursor < len(data):
        capture.feed(data[cursor:])
    capture.finish()
    return capture


class ProtocolCaptureProxyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.lock = self.root / "vanilla.lock.toml"
        self.lock.write_text(LOCK_TEXT, encoding="utf-8")
        self.target = _read_target(self.lock)
        self.stream = frame(b"\x01alpha") + frame(b"\xac\x02beta") + frame(b"")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_capture_is_identical_for_every_single_split_boundary(self) -> None:
        whole = capture_for(self.stream, [len(self.stream)])
        expected = [item.to_json() for item in whole.frames]
        for split in range(len(self.stream) + 1):
            with self.subTest(split=split):
                candidate = capture_for(self.stream, [split])
                self.assertEqual([item.to_json() for item in candidate.frames], expected)
                self.assertEqual(candidate.stream_bytes, len(self.stream))

        bytewise = capture_for(self.stream, [1] * len(self.stream))
        self.assertEqual([item.to_json() for item in bytewise.frames], expected)

    def test_frame_offsets_and_exact_bytes_are_retained(self) -> None:
        capture = capture_for(self.stream, [2, 3, 1, 7])
        self.assertEqual(len(capture.frames), 3)
        first, second, third = capture.frames
        self.assertEqual(first.stream_offset, 0)
        self.assertEqual(second.stream_offset, len(first.frame))
        self.assertEqual(third.stream_offset, len(first.frame) + len(second.frame))
        self.assertEqual(b"".join(item.frame for item in capture.frames), self.stream)
        self.assertEqual(first.body, b"\x01alpha")
        self.assertEqual(second.body, b"\xac\x02beta")
        self.assertEqual(third.body, b"")

    def test_invalid_lengths_and_bounds_fail_before_admission(self) -> None:
        capture = FrameStreamCapture(max_frame_bytes=8, max_stream_bytes=64, max_frames=2)
        with self.assertRaisesRegex(CaptureError, "noncanonical"):
            capture.feed(b"\x80\x00")

        capture = FrameStreamCapture(max_frame_bytes=8, max_stream_bytes=64, max_frames=2)
        with self.assertRaisesRegex(CaptureError, "frame body exceeds"):
            capture.feed(encode_var_int(9))

        capture = FrameStreamCapture(max_frame_bytes=8, max_stream_bytes=4, max_frames=2)
        with self.assertRaisesRegex(CaptureError, "stream exceeds"):
            capture.feed(b"12345")

        capture = FrameStreamCapture(max_frame_bytes=8, max_stream_bytes=64, max_frames=1)
        capture.feed(frame(b"a"))
        with self.assertRaisesRegex(CaptureError, "frame bound"):
            capture.feed(frame(b"b"))

    def test_incomplete_final_frame_fails_closed(self) -> None:
        capture = FrameStreamCapture(max_frame_bytes=64, max_stream_bytes=64, max_frames=4)
        complete = frame(b"complete")
        capture.feed(complete + b"\x05xy")
        self.assertEqual(len(capture.frames), 1)
        with self.assertRaisesRegex(CaptureError, "incomplete frame"):
            capture.finish()

    def test_artifact_is_canonical_and_chunking_independent(self) -> None:
        empty_a = FrameStreamCapture(max_frame_bytes=4_096, max_stream_bytes=65_536, max_frames=32)
        empty_b = FrameStreamCapture(max_frame_bytes=4_096, max_stream_bytes=65_536, max_frames=32)
        first = build_artifact(
            target=self.target,
            client_to_server=capture_for(self.stream, [len(self.stream)]),
            server_to_client=empty_a,
        )
        second = build_artifact(
            target=self.target,
            client_to_server=capture_for(self.stream, [1] * len(self.stream)),
            server_to_client=empty_b,
        )
        self.assertEqual(first, second)
        rendered = render_artifact(first)
        self.assertEqual(json.loads(rendered), first)
        self.assertEqual(len(first["capture_sha256"]), 64)
        self.assertEqual(first["target"]["protocol"], 42)

    def test_artifact_rejects_directional_limit_disagreement(self) -> None:
        client = FrameStreamCapture(max_frame_bytes=4_096, max_stream_bytes=65_536, max_frames=32)
        server = FrameStreamCapture(max_frame_bytes=8_192, max_stream_bytes=65_536, max_frames=32)
        with self.assertRaisesRegex(CaptureError, "identical evidence limits"):
            build_artifact(
                target=self.target,
                client_to_server=client,
                server_to_client=server,
            )

    def test_socket_pump_is_byte_transparent(self) -> None:
        producer, source = socket.socketpair()
        destination, consumer = socket.socketpair()
        capture = FrameStreamCapture(max_frame_bytes=4_096, max_stream_bytes=65_536, max_frames=32)
        errors: list[str] = []
        thread = threading.Thread(target=_pump, args=(source, destination, capture, errors))
        thread.start()
        try:
            for byte in self.stream:
                producer.sendall(bytes([byte]))
            producer.shutdown(socket.SHUT_WR)
            received = bytearray()
            while True:
                chunk = consumer.recv(64 * 1024)
                if not chunk:
                    break
                received.extend(chunk)
            thread.join(2)
            self.assertFalse(thread.is_alive())
            self.assertEqual(errors, [])
            self.assertEqual(bytes(received), self.stream)
            self.assertEqual(b"".join(item.frame for item in capture.frames), self.stream)
        finally:
            producer.close()
            source.close()
            destination.close()
            consumer.close()

    def test_target_identity_is_bound_from_lock(self) -> None:
        self.assertEqual(
            self.target,
            {
                "minecraft": "test-version",
                "protocol": 42,
                "source_archive_sha256": "a" * 64,
                "fingerprint_algorithm": "test-fingerprint-v1",
            },
        )

        broken = self.root / "broken.toml"
        broken.write_text('minecraft = "x"\nprotocol = true\n', encoding="utf-8")
        with self.assertRaises(CaptureError):
            _read_target(broken)

        for digest in ("A" * 64, "a" * 63, "g" * 64, ""):
            with self.subTest(digest=digest):
                malformed = self.root / "malformed.toml"
                malformed.write_text(
                    LOCK_TEXT.replace("a" * 64, digest), encoding="utf-8"
                )
                with self.assertRaisesRegex(CaptureError, "canonical lowercase hex"):
                    _read_target(malformed)


if __name__ == "__main__":
    unittest.main()
