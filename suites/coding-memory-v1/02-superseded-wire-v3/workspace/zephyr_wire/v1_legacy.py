"""Archived v1 byte-stuffed framing codec retained for capture replay tools."""

from __future__ import annotations

import struct

from .constants import V1_MAGIC, WireFormatError


def _xor8(data: bytes) -> int:
    acc = 0
    for b in data:
        acc ^= b
    return acc


def encode_v1_frame(frame_type: int, payload: bytes) -> bytes:
    if not isinstance(payload, (bytes, bytearray)):
        raise WireFormatError("payload must be bytes")
    raw_payload = bytes(payload)
    if len(raw_payload) > 255:
        raise WireFormatError("v1 payload exceeds 255 bytes")
    header = struct.pack(">BBB", V1_MAGIC, frame_type & 0xFF, len(raw_payload))
    chk = _xor8(header + raw_payload)
    return header + raw_payload + bytes([chk])


def decode_v1_frame(raw: bytes) -> tuple[int, bytes]:
    if len(raw) < 4:
        raise WireFormatError("truncated v1 frame")
    magic, frame_type, length = struct.unpack(">BBB", raw[:3])
    if magic != V1_MAGIC:
        raise WireFormatError(f"invalid v1 magic: 0x{magic:02x}")
    if len(raw) != 4 + length:
        raise WireFormatError("v1 frame length mismatch")
    body = raw[:-1]
    if _xor8(body) != raw[-1]:
        raise WireFormatError("v1 xor checksum mismatch")
    return frame_type, raw[3:-1]
