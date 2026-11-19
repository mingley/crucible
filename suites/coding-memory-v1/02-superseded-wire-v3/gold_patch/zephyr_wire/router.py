"""Frame routing and active control-plane ping codec for ZephyrWire."""

from __future__ import annotations

import struct

from .constants import (
    FLAG_CONTROL,
    FRAME_TYPE_DATA,
    FRAME_TYPE_PING,
    PING_NONCE_LEN,
    PING_PAYLOAD_LEN,
    U32_MAX,
    V1_MAGIC,
    V2_MAGIC,
    V3_MAGIC,
    WireFormatError,
)
from .v1_legacy import decode_v1_frame
from .v2_codec import decode_v2_frame
from .v3_codec import decode_v3_frame, encode_v3_frame

_PING_SEQ = struct.Struct(">I")


def route_data_frame(raw: bytes) -> tuple[int, bytes]:
    """Decode an inbound DATA frame across supported replay/live protocol versions.

    Returns ``(protocol_version, payload)``.
    """
    if not raw:
        raise WireFormatError("empty wire buffer")
    magic = raw[0]
    if magic == V1_MAGIC:
        frame_type, payload = decode_v1_frame(raw)
        if frame_type != FRAME_TYPE_DATA:
            raise WireFormatError("expected DATA frame")
        return 1, payload
    if magic == V2_MAGIC:
        f2 = decode_v2_frame(raw)
        if f2.frame_type != FRAME_TYPE_DATA:
            raise WireFormatError("expected DATA frame")
        return 2, f2.payload
    if magic == V3_MAGIC:
        f3 = decode_v3_frame(raw)
        if f3.frame_type != FRAME_TYPE_DATA:
            raise WireFormatError("expected DATA frame")
        return 3, f3.payload
    raise WireFormatError(f"unknown wire magic 0x{magic:02x}")


def encode_control_ping(seq: int, nonce: bytes) -> bytes:
    """Encode a control PING frame using the active ZephyrWire v3 standard."""
    if isinstance(seq, bool) or not isinstance(seq, int) or not (0 <= seq <= U32_MAX):
        raise WireFormatError("seq must be an unsigned 32-bit integer")
    if not isinstance(nonce, (bytes, bytearray)) or len(nonce) != PING_NONCE_LEN:
        raise WireFormatError("nonce must be exactly 8 bytes")
    payload = _PING_SEQ.pack(seq) + bytes(nonce)
    return encode_v3_frame(FRAME_TYPE_PING, FLAG_CONTROL, payload)


def decode_control_ping(raw: bytes) -> tuple[int, bytes]:
    """Decode and validate an active-standard v3 control PING frame into ``(seq, nonce)``."""
    frame = decode_v3_frame(raw)
    if frame.frame_type != FRAME_TYPE_PING:
        raise WireFormatError(f"expected PING frame_type 0x01, got 0x{frame.frame_type:02x}")
    if (frame.flags & FLAG_CONTROL) == 0:
        raise WireFormatError("control PING frame missing FLAG_CONTROL (0x04)")
    if len(frame.payload) != PING_PAYLOAD_LEN:
        raise WireFormatError(f"expected 12-byte PING payload, got {len(frame.payload)}")
    (seq,) = _PING_SEQ.unpack_from(frame.payload, 0)
    nonce = frame.payload[4:12]
    return seq, nonce
