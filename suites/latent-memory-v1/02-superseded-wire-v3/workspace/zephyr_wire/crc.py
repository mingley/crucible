"""Checksum algorithms used across ZephyrWire protocol generations."""

from __future__ import annotations

import zlib


def crc16_ccitt(data: bytes, init: int = 0xFFFF) -> int:
    """Compute CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF)."""
    crc = init & 0xFFFF
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF
    return crc


def crc32_ieee(data: bytes) -> int:
    """Compute unsigned 32-bit IEEE 802.3 CRC-32."""
    return zlib.crc32(data) & 0xFFFFFFFF
