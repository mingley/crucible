"""Compact double-hashing Bloom filter for SSTable key membership checks."""

from __future__ import annotations

import hashlib


class BloomFilter:
    """Deterministic bitset filter over UTF-8 encoded keys."""

    def __init__(self, bit_count: int = 512, num_hashes: int = 4) -> None:
        if bit_count <= 0 or (bit_count % 8) != 0:
            raise ValueError("bit_count must be a positive multiple of 8")
        self._bit_count = bit_count
        self._num_hashes = num_hashes
        self._bits = bytearray(bit_count // 8)

    def _positions(self, key: str) -> list[int]:
        digest = hashlib.blake2s(key.encode("utf-8"), digest_size=16).digest()
        h1 = int.from_bytes(digest[:8], "big")
        h2 = int.from_bytes(digest[8:], "big") | 1
        return [(h1 + i * h2) % self._bit_count for i in range(self._num_hashes)]

    def add(self, key: str) -> None:
        for bit in self._positions(key):
            self._bits[bit // 8] |= 1 << (bit % 8)

    def might_contain(self, key: str) -> bool:
        for bit in self._positions(key):
            if (self._bits[bit // 8] & (1 << (bit % 8))) == 0:
                return False
        return True
