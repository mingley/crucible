"""ZephyrWire binary framing codecs, routing, and peer session management."""

from .constants import (
    FLAG_COMPRESSED,
    FLAG_CONTROL,
    FLAG_END_STREAM,
    FLAG_NONE,
    FRAME_TYPE_CLOSE,
    FRAME_TYPE_DATA,
    FRAME_TYPE_PING,
    FRAME_TYPE_PONG,
    PING_NONCE_LEN,
    PING_PAYLOAD_LEN,
    V1_MAGIC,
    V2_MAGIC,
    V3_MAGIC,
    WireFormatError,
)
from .router import decode_control_ping, encode_control_ping, route_data_frame
from .session import PeerSession

__all__ = [
    "FLAG_COMPRESSED",
    "FLAG_CONTROL",
    "FLAG_END_STREAM",
    "FLAG_NONE",
    "FRAME_TYPE_CLOSE",
    "FRAME_TYPE_DATA",
    "FRAME_TYPE_PING",
    "FRAME_TYPE_PONG",
    "PING_NONCE_LEN",
    "PING_PAYLOAD_LEN",
    "PeerSession",
    "V1_MAGIC",
    "V2_MAGIC",
    "V3_MAGIC",
    "WireFormatError",
    "decode_control_ping",
    "encode_control_ping",
    "route_data_frame",
]
