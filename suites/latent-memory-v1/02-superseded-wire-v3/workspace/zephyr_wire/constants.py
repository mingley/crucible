"""Protocol constants and error hierarchy for ZephyrWire."""

from __future__ import annotations


class WireFormatError(ValueError):
    """Raised when a binary frame violates wire framing or field invariants."""


V1_MAGIC = 0x7E
V2_MAGIC = 0xA5
V3_MAGIC = 0xB7

FRAME_TYPE_DATA = 0x00
FRAME_TYPE_PING = 0x01
FRAME_TYPE_PONG = 0x02
FRAME_TYPE_CLOSE = 0x03

FLAG_NONE = 0x00
FLAG_COMPRESSED = 0x01
FLAG_END_STREAM = 0x02
FLAG_CONTROL = 0x04

PING_NONCE_LEN = 8
PING_PAYLOAD_LEN = 12
U32_MAX = 0xFFFFFFFF
