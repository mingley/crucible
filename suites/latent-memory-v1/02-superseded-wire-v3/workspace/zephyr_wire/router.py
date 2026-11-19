"""Frame routing and active control-plane ping codec for ZephyrWire."""

from __future__ import annotations

from .constants import (
    FRAME_TYPE_DATA,
    V1_MAGIC,
    V2_MAGIC,
    V3_MAGIC,
    WireFormatError,
)
from .v1_legacy import decode_v1_frame
from .v2_codec import decode_v2_frame
from .v3_codec import decode_v3_frame


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
    """Encode a control PING frame using the active ZephyrWire framing standard."""
    raise NotImplementedError("Control ping encoder not yet implemented")


def decode_control_ping(raw: bytes) -> tuple[int, bytes]:
    """Decode and validate an active-standard control PING frame into ``(seq, nonce)``."""
    raise NotImplementedError("Control ping decoder not yet implemented")
