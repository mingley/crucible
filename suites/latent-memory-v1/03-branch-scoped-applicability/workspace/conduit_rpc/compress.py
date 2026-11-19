"""Optional zlib payload compression for ConduitRPC binary frames."""

from __future__ import annotations

import zlib

from .errors import FrameCorruptError


def maybe_compress(payload: bytes, threshold: int = 128) -> tuple[bytes, bool]:
    if len(payload) < threshold:
        return payload, False
    compressed = zlib.compress(payload, level=6)
    if len(compressed) >= len(payload):
        return payload, False
    return compressed, True


def maybe_decompress(payload: bytes, is_compressed: bool) -> bytes:
    if not is_compressed:
        return payload
    try:
        return zlib.decompress(payload)
    except zlib.error as exc:
        raise FrameCorruptError(f"failed to decompress payload: {exc}") from exc
