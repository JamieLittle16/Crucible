#!/usr/bin/env python3
"""Capture the first source-admitted Minecraft Login connection through the plaintext proxy.

Minecraft's multiplayer screen may open one or more STATUS connections before the user joins the
server.  The generic capture proxy deliberately records exactly one TCP connection and is unaware
of Minecraft semantics, so a status ping can legitimately win that race.

This R1A2 selector keeps one listening socket open across a bounded number of connections.  Every
connection is transparently proxied to vanilla using the already-qualified frame capture machinery.
Clean non-LOGIN connections are discarded as evidence; the first connection whose admitted
handshake envelope has intent LOGIN=2 is written as the capture artifact.

No post-handshake packet is interpreted here.  Login semantics remain the responsibility of
``tools.r1_login_contract``.
"""

from __future__ import annotations

import argparse
import json
import socket
import sys
import threading
from pathlib import Path

from tools.protocol_capture_proxy import (
    CAPTURE_KIND,
    CaptureError,
    FrameStreamCapture,
    _pump,
    _read_target,
    build_artifact,
    render_artifact,
)

LOGIN_INTENT = 2
HANDSHAKE_PACKET_ID = 0
MAX_HANDSHAKE_ADDRESS_BYTES = 255 * 3


def _read_var_int(data: bytes, cursor: int, label: str) -> tuple[int, int]:
    """Read one canonical non-negative Minecraft VarInt from ``data``."""
    value = 0
    start = cursor
    for index in range(5):
        if cursor >= len(data):
            raise CaptureError(f"{label} is truncated")
        byte = data[cursor]
        cursor += 1
        value |= (byte & 0x7F) << (7 * index)
        if byte & 0x80 == 0:
            if value > 0x7FFF_FFFF:
                raise CaptureError(f"{label} exceeds non-negative i32 range")
            canonical = bytearray()
            remaining = value
            while True:
                encoded = remaining & 0x7F
                remaining >>= 7
                if remaining:
                    canonical.append(encoded | 0x80)
                else:
                    canonical.append(encoded)
                    break
            if data[start:cursor] != bytes(canonical):
                raise CaptureError(f"{label} uses a noncanonical VarInt")
            return value, cursor
    raise CaptureError(f"{label} exceeds the five-byte VarInt bound")


def handshake_intent(body: bytes) -> int:
    """Return the intent from the source-admitted handshake envelope."""
    packet_id, cursor = _read_var_int(body, 0, "handshake packet id")
    if packet_id != HANDSHAKE_PACKET_ID:
        raise CaptureError(
            f"first client frame packet id is {packet_id}, handshake packet id must be 0"
        )

    _protocol, cursor = _read_var_int(body, cursor, "handshake protocol version")
    address_bytes, cursor = _read_var_int(body, cursor, "handshake server address length")
    if address_bytes > MAX_HANDSHAKE_ADDRESS_BYTES:
        raise CaptureError(
            "handshake server address encoded length exceeds the source-backed 255-unit bound"
        )
    end = cursor + address_bytes
    if end > len(body):
        raise CaptureError("handshake server address is truncated")
    try:
        body[cursor:end].decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise CaptureError("handshake server address is not valid UTF-8") from error
    cursor = end

    if cursor + 2 > len(body):
        raise CaptureError("handshake server port is truncated")
    cursor += 2

    intent, cursor = _read_var_int(body, cursor, "handshake intent")
    if cursor != len(body):
        raise CaptureError("handshake packet has trailing payload bytes")
    return intent


def _capture_one(
    client: socket.socket,
    *,
    upstream_host: str,
    upstream_port: int,
    target: dict[str, object],
    max_frame_bytes: int,
    max_stream_bytes: int,
    max_frames: int,
    timeout_seconds: float,
) -> tuple[dict[str, object], int]:
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

    artifact = build_artifact(
        target=target,
        client_to_server=c2s,
        server_to_client=s2c,
    )
    if not c2s.frames:
        raise CaptureError("client connection contained no framed handshake")
    return artifact, handshake_intent(c2s.frames[0].body)


def proxy_until_login(
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
    max_connections: int,
) -> tuple[dict[str, object], int, tuple[int, ...]]:
    """Proxy a bounded number of connections and retain the first LOGIN handshake."""
    if not 0 <= listen_port <= 65535 or not 1 <= upstream_port <= 65535:
        raise CaptureError("listen/upstream ports are out of range")
    if timeout_seconds <= 0:
        raise CaptureError("timeout must be positive")
    if max_connections <= 0:
        raise CaptureError("max_connections must be positive")

    target = _read_target(lock_path)
    skipped: list[int] = []

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((listen_host, listen_port))
        listener.listen(max_connections)
        listener.settimeout(timeout_seconds)
        print(
            f"R1 Login capture listening on {listener.getsockname()[0]}:{listener.getsockname()[1]} "
            f"-> {upstream_host}:{upstream_port}; waiting for handshake intent {LOGIN_INTENT}",
            file=sys.stderr,
        )

        for attempt in range(1, max_connections + 1):
            try:
                client, _ = listener.accept()
            except TimeoutError as error:
                raise CaptureError("timed out waiting for the next client connection") from error

            artifact, intent = _capture_one(
                client,
                upstream_host=upstream_host,
                upstream_port=upstream_port,
                target=target,
                max_frame_bytes=max_frame_bytes,
                max_stream_bytes=max_stream_bytes,
                max_frames=max_frames,
                timeout_seconds=timeout_seconds,
            )
            if intent == LOGIN_INTENT:
                output_path.parent.mkdir(parents=True, exist_ok=True)
                if output_path.exists() and output_path.is_symlink():
                    raise CaptureError(f"refusing to replace symlink output: {output_path}")
                output_path.write_text(render_artifact(artifact), encoding="utf-8")
                return artifact, attempt, tuple(skipped)

            skipped.append(intent)
            print(
                f"ignored clean connection {attempt}/{max_connections} with handshake intent {intent}; "
                f"still waiting for LOGIN={LOGIN_INTENT}",
                file=sys.stderr,
            )

    raise CaptureError(
        f"no LOGIN={LOGIN_INTENT} connection observed within {max_connections} clean connections"
    )


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--listen-host", default="127.0.0.1")
    parser.add_argument("--listen-port", type=int, default=25566)
    parser.add_argument("--upstream-host", default="127.0.0.1")
    parser.add_argument("--upstream-port", type=int, default=25565)
    parser.add_argument("--lock", type=Path, default=Path("vanilla/vanilla.lock.toml"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--max-frame-bytes", type=int, default=8 << 20)
    parser.add_argument("--max-stream-bytes", type=int, default=64 << 20)
    parser.add_argument("--max-frames", type=int, default=512)
    parser.add_argument("--timeout-seconds", type=float, default=300.0)
    parser.add_argument("--max-connections", type=int, default=8)
    return parser.parse_args()


def main() -> int:
    args = _arguments()
    try:
        artifact, selected_connection, skipped_intents = proxy_until_login(
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
            max_connections=args.max_connections,
        )
    except (CaptureError, OSError) as error:
        print(f"R1 Login capture error: {error}", file=sys.stderr)
        return 1

    print(
        json.dumps(
            {
                "kind": CAPTURE_KIND,
                "capture_sha256": artifact["capture_sha256"],
                "output": str(args.output),
                "selected_connection": selected_connection,
                "skipped_intents": list(skipped_intents),
            },
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
