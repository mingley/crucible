"""ZephyrWire v3 binary frame codec (magic 0xB7, BE u32 length, CRC32)."""

from __future__ import annotations

import struct
from dataclasses import dataclass

from .constants import U32_MAX, V3_MAGIC, WireFormatError
from .crc import crc32_ieee

_V3_HEADER = struct.Struct(">BBBI")
_V3_TRAILER = struct.Struct(">I")


@dataclass(frozen=True)
class FrameV3:
    frame_type: int
    flags: int
    payload: bytes


def encode_v3_frame(frame_type: int, flags: int, payload: bytes) -> bytes:
    if not isinstance(payload, (bytes, bytearray)):
        raise WireFormatError("payload must be bytes")
    raw_payload = bytes(payload)
    if len(raw_payload) > U32_MAX:
        raise WireFormatError("v3 payload exceeds u32 max")
    if not (0 <= frame_type <= 0xFF) or not (0 <= flags <= 0xFF):
        raise WireFormatError("frame_type and flags must fit u8")
    header = _V3_HEADER.pack(V3_MAGIC, frame_type, flags, len(raw_payload))
    digest = crc32_ieee(header + raw_payload)
    return header + raw_payload + _V3_TRAILER.pack(digest)


def decode_v3_frame(raw: bytes) -> FrameV3:
    if not isinstance(raw, (bytes, bytearray)):
        raise WireFormatError("raw frame must be bytes")
    buf = bytes(raw)
    if len(buf) < _V3_HEADER.size + _V3_TRAILER.size:
        raise WireFormatError("truncated v3 frame")
    magic, frame_type, flags, length = _V3_HEADER.unpack_from(buf, 0)
    if magic != V3_MAGIC:
        raise WireFormatError(f"unexpected v3 magic: 0x{magic:02x}")
    expected_total = _V3_HEADER.size + length + _V3_TRAILER.size
    if len(buf) != expected_total:
        raise WireFormatError("v3 frame length mismatch")
    payload = buf[_V3_HEADER.size : _V3_HEADER.size + length]
    (wire_crc,) = _V3_TRAILER.unpack_from(buf, _V3_HEADER.size + length)
    if crc32_ieee(buf[: _V3_HEADER.size + length]) != wire_crc:
        raise WireFormatError("v3 crc32 mismatch")
    return FrameV3(frame_type=frame_type, flags=flags, payload=payload)
