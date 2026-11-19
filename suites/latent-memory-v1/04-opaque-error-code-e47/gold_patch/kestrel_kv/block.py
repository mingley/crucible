"""SSTable data block serialization and index fence separator computation."""

from __future__ import annotations

from .errors import E23_BLOCK_MAGIC, KestrelError
from .varint import decode_uvarint, encode_uvarint

_BLOCK_MAGIC = b"KBLK"


def shortest_separator(left_key: str, right_key: str) -> str:
    """Compute a compact fence key ``sep`` satisfying ``left_key <= sep < right_key``.

    Finds the first differing position between ``left_key`` and ``right_key`` and,
    if the character can be incremented while remaining strictly below ``right_key``
    (skipping UTF-16 surrogates U+D800..U+DFFF), returns the shortened prefix;
    otherwise returns ``left_key``.
    """
    if left_key >= right_key:
        return left_key
    diff = 0
    limit = min(len(left_key), len(right_key))
    while diff < limit and left_key[diff] == right_key[diff]:
        diff += 1
    if diff < limit:
        next_cp = ord(left_key[diff]) + 1
        if 0xD800 <= next_cp <= 0xDFFF:
            next_cp = 0xE000
        if next_cp < ord(right_key[diff]):
            return left_key[:diff] + chr(next_cp)
    return left_key


class DataBlockBuilder:
    """Encodes sorted ``(key, value_or_none_tombstone)`` records into a binary block."""

    def __init__(self) -> None:
        self._records: list[tuple[str, bytes | None]] = []

    @property
    def count(self) -> int:
        return len(self._records)

    @property
    def min_key(self) -> str:
        return self._records[0][0]

    @property
    def max_key(self) -> str:
        return self._records[-1][0]

    def add(self, key: str, value: bytes | None) -> None:
        self._records.append((key, value))

    def finish(self) -> bytes:
        buf = bytearray(_BLOCK_MAGIC)
        buf.extend(encode_uvarint(len(self._records)))
        for key, value in self._records:
            key_bytes = key.encode("utf-8")
            buf.extend(encode_uvarint(len(key_bytes)))
            buf.extend(key_bytes)
            if value is None:
                buf.append(1)  # tombstone marker
            else:
                buf.append(0)
                buf.extend(encode_uvarint(len(value)))
                buf.extend(value)
        return bytes(buf)


class DataBlockReader:
    """Decodes records from a serialized ``DataBlockBuilder`` payload."""

    def __init__(self, raw: bytes) -> None:
        if len(raw) < 4 or raw[:4] != _BLOCK_MAGIC:
            raise KestrelError(E23_BLOCK_MAGIC, "invalid data block magic")
        self._raw = raw

    def iter_records(self) -> list[tuple[str, bytes | None]]:
        count, pos = decode_uvarint(self._raw, 4)
        out: list[tuple[str, bytes | None]] = []
        for _ in range(count):
            klen, pos = decode_uvarint(self._raw, pos)
            key = self._raw[pos : pos + klen].decode("utf-8")
            pos += klen
            is_tombstone = self._raw[pos]
            pos += 1
            if is_tombstone == 1:
                out.append((key, None))
            else:
                vlen, pos = decode_uvarint(self._raw, pos)
                val = self._raw[pos : pos + vlen]
                pos += vlen
                out.append((key, val))
        return out
