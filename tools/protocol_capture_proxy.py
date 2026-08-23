#!/usr/bin/env python3
"""Capture bounded pre-play Minecraft frames while transparently proxying one TCP connection."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import socket
import sys
import threading
import tomllib
from dataclasses import dataclass
from pathlib import Path

SCHEMA = 1
CAPTURE_KIND = "preplay-frame-capture-v1"
HEX_64 = re.compile(r"^[0-9a-f]{64}$")


class CaptureError(ValueError):
    """Raised when a stream cannot be admitted as bounded canonical pre-play framing."""


def _encode_nonnegative_var_int(value: int) -> bytes:
    if not 0 <= value <= 0x7FFF_FFFF:
        raise CaptureError(f"frame length is outside non-negative i32 range: {value}")
    encoded = bytearray()
    remaining = value
    while True:
        byte = remaining & 0x7F
        remaining >>= 7
        if remaining:
            encoded.append(byte | 0x80)
        else:
            encoded.append(byte)
            return bytes(encoded)


def _decode_frame_length(data: bytearray) -> tuple[int, int] | None:
    value = 0
    for index in range(min(len(data), 5)):
        byte = data[index]
        value |= (byte & 0x7F) << (7 * index)
        if byte & 0x80 == 0:
            consumed = index + 1
            if value > 0x7FFF_FFFF:
                raise CaptureError("frame length exceeds non-negative i32 range")
            if bytes(data[:consumed]) != _encode_nonnegative_var_int(value):
                raise CaptureError("frame length uses noncanonical VarInt encoding")
            return value, consumed
    if len(data) < 5:
        return None
    raise CaptureError("frame length exceeds the five-byte VarInt bound")


@dataclass(frozen=True)
class CapturedFrame:
    ordinal: int
    stream_offset: int
    frame: bytes
    body: bytes

    def to_json(self) -> dict[str, object]:
        return {
            "ordinal": self.ordinal,
            "stream_offset": self.stream_offset,
            "frame_bytes": len(self.frame),
            "body_bytes": len(self.body),
            "frame_sha256": hashlib.sha256(self.frame).hexdigest(),
            "frame_hex": self.frame.hex(),
            "body_hex": self.body.hex(),
        }


class FrameStreamCapture:
    """Incrementally reconstruct canonical length-prefixed frames independent of TCP chunking."""

    def __init__(self, *, max_frame_bytes: int, max_stream_bytes: int, max_frames: int) -> None:
        if max_frame_bytes <= 0 or max_stream_bytes <= 0 or max_frames <= 0:
            raise CaptureError("capture limits must all be positive")
        if max_frame_bytes > 0x7FFF_FFFF:
            raise CaptureError("max frame bytes exceeds non-negative i32 range")
        self.max_frame_bytes = max_frame_bytes
        self.max_stream_bytes = max_stream_bytes
        self.max_frames = max_frames
        self._buffer = bytearray()
        self._frames: list[CapturedFrame] = []
        self._stream_bytes = 0
        self._consumed_bytes = 0
        self._stream_sha256 = hashlib.sha256()

    @property
    def frames(self) -> tuple[CapturedFrame, ...]:
        return tuple(self._frames)

    @property
    def stream_bytes(self) -> int:
        return self._stream_bytes

    @property
    def limits(self) -> tuple[int, int, int]:
        return self.max_frame_bytes, self.max_stream_bytes, self.max_frames

    def feed(self, data: bytes) -> None:
        if not data:
            return
        next_total = self._stream_bytes + len(data)
        if next_total > self.max_stream_bytes:
            raise CaptureError(
                f"stream exceeds configured byte bound {self.max_stream_bytes}: {next_total}"
            )
        self._stream_bytes = next_total
        self._stream_sha256.update(data)
        self._buffer.extend(data)

        while self._buffer:
            decoded = _decode_frame_length(self._buffer)
            if decoded is None:
                return
            body_length, prefix_length = decoded
            if body_length > self.max_frame_bytes:
                raise CaptureError(
                    f"frame body exceeds configured bound {self.max_frame_bytes}: {body_length}"
                )
            total_length = prefix_length + body_length
            if len(self._buffer) < total_length:
                return
            if len(self._frames) >= self.max_frames:
                raise CaptureError(f"stream exceeds configured frame bound {self.max_frames}")
            raw = bytes(self._buffer[:total_length])
            body = raw[prefix_length:]
            self._frames.append(
                CapturedFrame(
                    ordinal=len(self._frames),
                    stream_offset=self._consumed_bytes,
                    frame=raw,
                    body=body,
                )
            )
            del self._buffer[:total_length]
            self._consumed_bytes += total_length

    def finish(self) -> None:
        if self._buffer:
            raise CaptureError(
                f"stream ended with {len(self._buffer)} bytes of an incomplete frame"
            )
        if self._consumed_bytes != self._stream_bytes:
            raise CaptureError("internal capture accounting mismatch")

    def to_json(self, direction: str) -> dict[str, object]:
        self.finish()
        return {
            "direction": direction,
            "stream_bytes": self._stream_bytes,
            "stream_sha256": self._stream_sha256.hexdigest(),
            "frame_count": len(self._frames),
            "frames": [frame.to_json() for frame in self._frames],
        }


def _read_target(lock_path: Path) -> dict[str, object]:
    if lock_path.is_symlink() or not lock_path.is_file():
        raise CaptureError(f"vanilla lock must be a real non-symlink file: {lock_path}")
    try:
        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise CaptureError(f"could not read vanilla lock {lock_path}: {error}") from error
    try:
        source = lock["source"]
        atlas = lock["atlas"]
        target = {
            "minecraft": lock["minecraft"],
            "protocol": lock["protocol"],
            "source_archive_sha256": source["archive_sha256"],
            "fingerprint_algorithm": atlas["fingerprint_algorithm"],
        }
    except (KeyError, TypeError) as error:
        raise CaptureError("vanilla lock is missing required target identity") from error
    if not isinstance(target["minecraft"], str) or not target["minecraft"]:
        raise CaptureError("vanilla lock Minecraft version must be a non-empty string")
    if type(target["protocol"]) is not int:
        raise CaptureError("vanilla lock protocol must be an integer")
    source_digest = target["source_archive_sha256"]
    if not isinstance(source_digest, str) or HEX_64.fullmatch(source_digest) is None:
        raise CaptureError("vanilla lock source archive SHA-256 must be canonical lowercase hex")
    fingerprint_algorithm = target["fingerprint_algorithm"]
    if not isinstance(fingerprint_algorithm, str) or not fingerprint_algorithm:
        raise CaptureError("vanilla lock fingerprint algorithm must be a non-empty string")
    return target


def build_artifact(
    *,
    target: dict[str, object],
    client_to_server: FrameStreamCapture,
    server_to_client: FrameStreamCapture,
) -> dict[str, object]:
    if client_to_server.limits != server_to_client.limits:
        raise CaptureError("both capture directions must use identical evidence limits")
    max_frame_bytes, max_stream_bytes, max_frames = client_to_server.limits
    artifact: dict[str, object] = {
        "schema": SCHEMA,
        "kind": CAPTURE_KIND,
        "target": target,
        "limits": {
            "max_frame_bytes": max_frame_bytes,
            "max_stream_bytes": max_stream_bytes,
            "max_frames_per_direction": max_frames,
        },
        "streams": [
            client_to_server.to_json("client-to-server"),
            server_to_client.to_json("server-to-client"),
        ],
    }
    canonical = json.dumps(
        artifact, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("ascii")
    artifact["capture_sha256"] = hashlib.sha256(canonical).hexdigest()
    return artifact


def render_artifact(artifact: dict[str, object]) -> str:
    return json.dumps(artifact, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n"


def _pump(
    source: socket.socket,
    destination: socket.socket,
    capture: FrameStreamCapture,
    errors: list[str],
) -> None:
    try:
        while True:
            data = source.recv(64 * 1024)
            if not data:
                capture.finish()
                try:
                    destination.shutdown(socket.SHUT_WR)
                except OSError:
                    pass
                return
            capture.feed(data)
            destination.sendall(data)
    except (CaptureError, OSError) as error:
        errors.append(str(error))
        try:
            destination.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass


def proxy_once(
    *,
    listen_host: str,
    listen_port: int,
    upstream_host: str,
    upstream_port: int,
    lock_path: Path,
    output_path: Path,
    max_frame_bytes: int,
    max_stream_bytes: int,
    max_frames: int,
    timeout_seconds: float,
) -> dict[str, object]:
    """Proxy exactly one connection and return its deterministic frame capture artifact."""
    if not 0 <= listen_port <= 65535 or not 1 <= upstream_port <= 65535:
        raise CaptureError("listen/upstream ports are out of range")
    if timeout_seconds <= 0:
        raise CaptureError("timeout must be positive")
    target = _read_target(lock_path)
    c2s = FrameStreamCapture(
        max_frame_bytes=max_frame_bytes,
        max_stream_bytes=max_stream_bytes,
        max_frames=max_frames,
    )
    s2c = FrameStreamCapture(
        max_frame_bytes=max_frame_bytes,
        max_stream_bytes=max_stream_bytes,
        max_frames=max_frames,
    )

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((listen_host, listen_port))
        listener.listen(1)
        listener.settimeout(timeout_seconds)
        print(
            f"protocol capture listening on {listener.getsockname()[0]}:{listener.getsockname()[1]} "
            f"-> {upstream_host}:{upstream_port}",
            file=sys.stderr,
        )
        try:
            client, _ = listener.accept()
        except TimeoutError as error:
            raise CaptureError("timed out waiting for client connection") from error

        with client, socket.create_connection(
            (upstream_host, upstream_port), timeout=timeout_seconds
        ) as upstream:
            client.settimeout(timeout_seconds)
            upstream.settimeout(timeout_seconds)
            errors: list[str] = []
            threads = [
                threading.Thread(target=_pump, args=(client, upstream, c2s, errors)),
                threading.Thread(target=_pump, args=(upstream, client, s2c, errors)),
            ]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join(timeout_seconds * 2)
            if any(thread.is_alive() for thread in threads):
                raise CaptureError("capture pumps did not terminate within the bounded timeout")
            if errors:
                raise CaptureError("; ".join(errors))

    artifact = build_artifact(target=target, client_to_server=c2s, server_to_client=s2c)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    if output_path.exists() and output_path.is_symlink():
        raise CaptureError(f"refusing to replace symlink output: {output_path}")
    output_path.write_text(render_artifact(artifact), encoding="utf-8")
    return artifact


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--listen-host", default="127.0.0.1")
    parser.add_argument("--listen-port", type=int, default=25566)
    parser.add_argument("--upstream-host", default="127.0.0.1")
    parser.add_argument("--upstream-port", type=int, default=25565)
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--max-frame-bytes", type=int, default=1 << 20)
    parser.add_argument("--max-stream-bytes", type=int, default=8 << 20)
    parser.add_argument("--max-frames", type=int, default=256)
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    try:
        artifact = proxy_once(
            listen_host=args.listen_host,
            listen_port=args.listen_port,
            upstream_host=args.upstream_host,
            upstream_port=args.upstream_port,
            lock_path=args.lock,
            output_path=args.output,
            max_frame_bytes=args.max_frame_bytes,
            max_stream_bytes=args.max_stream_bytes,
            max_frames=args.max_frames,
            timeout_seconds=args.timeout_seconds,
        )
    except (CaptureError, OSError) as error:
        print(f"protocol capture error: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "kind": CAPTURE_KIND,
                "capture_sha256": artifact["capture_sha256"],
                "output": str(args.output),
            },
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
