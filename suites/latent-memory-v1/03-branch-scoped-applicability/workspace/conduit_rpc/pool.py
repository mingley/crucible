"""Connection pool and zero-payload heartbeat probe handling for ConduitRPC."""

from __future__ import annotations

from collections import deque

from .errors import PoolExhaustedError
from .frame import FLAG_NONE, MSG_ACK, MSG_HEARTBEAT, PROTOCOL_V2_BINARY, FrameHeader, pack_wire_frame
from .transport import BinaryTransport


class ConnectionPool:
    """Manages logical stream slots and validates peer heartbeat ACKs."""

    def __init__(self, capacity: int = 8, transport: BinaryTransport | None = None) -> None:
        if capacity <= 0:
            raise ValueError("capacity must be positive")
        self._capacity = capacity
        self._transport = transport or BinaryTransport()
        self._idle_streams: deque[int] = deque(range(1, capacity + 1))

    def acquire_stream(self) -> int:
        if not self._idle_streams:
            raise PoolExhaustedError("no idle streams available")
        return self._idle_streams.popleft()

    def release_stream(self, stream_id: int) -> None:
        self._idle_streams.append(stream_id)

    def build_peer_heartbeat(self, stream_id: int) -> bytes:
        """Simulate a standard v2-binary peer emitting an empty HEARTBEAT frame."""
        hdr = FrameHeader(
            version=PROTOCOL_V2_BINARY,
            msg_type=MSG_HEARTBEAT,
            flags=FLAG_NONE,
            stream_id=stream_id,
            payload_len=0,
        )
        return pack_wire_frame(hdr, b"")

    def handle_heartbeat_and_ack(self, inbound_wire: bytes) -> bytes:
        hdr, payload = self._transport.decode_frame(inbound_wire)
        if hdr.msg_type != MSG_HEARTBEAT or payload != b"":
            raise ValueError("expected empty MSG_HEARTBEAT frame")
        return self._transport.encode_frame(MSG_ACK, stream_id=hdr.stream_id, payload=b"")
