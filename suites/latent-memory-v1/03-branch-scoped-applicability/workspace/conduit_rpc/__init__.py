"""ConduitRPC v2-binary multiplexed framing, compression, pooling, and retries."""

from .errors import (
    ChecksumMismatchError,
    ConduitError,
    FrameCorruptError,
    PoolExhaustedError,
    RetryExhaustedError,
)
from .frame import (
    FLAG_COMPRESSED,
    FLAG_NONE,
    MSG_ACK,
    MSG_CALL,
    MSG_HEARTBEAT,
    MSG_REPLY,
    PROTOCOL_V2_BINARY,
    FrameHeader,
    pack_wire_frame,
    unpack_wire_frame,
)
from .pool import ConnectionPool
from .retry import RetryPolicy, execute_with_retry
from .transport import BinaryTransport, frame_digest

__all__ = [
    "BinaryTransport",
    "ChecksumMismatchError",
    "ConduitError",
    "ConnectionPool",
    "FLAG_COMPRESSED",
    "FLAG_NONE",
    "FrameCorruptError",
    "FrameHeader",
    "MSG_ACK",
    "MSG_CALL",
    "MSG_HEARTBEAT",
    "MSG_REPLY",
    "PROTOCOL_V2_BINARY",
    "PoolExhaustedError",
    "RetryExhaustedError",
    "RetryPolicy",
    "execute_with_retry",
    "frame_digest",
    "pack_wire_frame",
    "unpack_wire_frame",
]
