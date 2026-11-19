"""ZephyrWire v2 binary frame codec (magic 0xA5, LE u16 length, CRC16-CCITT)."""

from __future__ import annotations

import struct
from dataclasses import dataclass

from .constants import V2_MAGIC, WireFormatError
from .crc import crc16_ccitt

_V2_HEADER = struct.Struct("<BBBH")
_V2_TRAILER = struct.Struct(">H")


@dataclass(frozen=True)
class FrameV2:
    frame_type: int
    flags: int
    payload: bytes


def encode_v2_frame(frame_type: int, flags: int, payload: bytes) -> bytes:
    if not isinstance(payload, (bytes, bytearray)):
        raise WireFormatError("payload must be bytes")
    raw_payload = bytes(payload)
    if len(raw_payload) > 0xFFFF:
        raise WireFormatError("v2 payload exceeds u16 max")
    if not (0 <= frame_type <= 0xFF) or not (0 <= flags <= 0xFF):
        raise WireFormatError("frame_type and flags must fit u8")
    header = _V2_HEADER.pack(V2_MAGIC, frame_type, flags, len(raw_payload))
    digest = crc16_ccitt(header + raw_payload)
    return header + raw_payload + _V2_TRAILER.pack(digest)


def decode_v2_frame(raw: bytes) -> FrameV2:
    if not isinstance(raw, (bytes, bytearray)):
        raise WireFormatError("raw frame must be bytes")
    buf = bytes(raw)
    if len(buf) < _V2_HEADER.size + _V2_TRAILER.size:
        raise WireFormatError("truncated v2 frame")
    magic, frame_type, flags, length = _V2_HEADER.unpack_from(buf, 0)
    if magic != V2_MAGIC:
        raise WireFormatError(f"unexpected v2 magic: 0x{magic:02x}")
    expected_total = _V2_HEADER.size + length + _V2_TRAILER.size
    if len(buf) != expected_total:
        raise WireFormatError("v2 frame length mismatch")
    payload = buf[_V2_HEADER.size : _V2_HEADER.size + length]
    (wire_crc,) = _V2_TRAILER.unpack_from(buf, _V2_HEADER.size + length)
    if crc16_ccitt(buf[: _V2_HEADER.size + length]) != wire_crc:
        raise WireFormatError("v2 crc16 mismatch")
    return FrameV2(frame_type=frame_type, flags=flags, payload=payload)
