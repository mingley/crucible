"""Unsigned LEB128 varint encoding and decoding primitives."""

from __future__ import annotations

from .errors import E12_CORRUPT_VARINT, KestrelError


def encode_uvarint(value: int) -> bytes:
    if value < 0:
        raise KestrelError(E12_CORRUPT_VARINT, "negative integer in uvarint encoder")
    out = bytearray()
    rem = value
    while rem >= 0x80:
        out.append((rem & 0x7F) | 0x80)
        rem >>= 7
    out.append(rem & 0x7F)
    return bytes(out)


def decode_uvarint(buf: bytes, offset: int = 0) -> tuple[int, int]:
    shift = 0
    result = 0
    pos = offset
    while pos < len(buf):
        byte = buf[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        if (byte & 0x80) == 0:
            return result, pos
        shift += 7
        if shift > 63:
            raise KestrelError(E12_CORRUPT_VARINT, "uvarint overflow")
    raise KestrelError(E12_CORRUPT_VARINT, "truncated uvarint")
