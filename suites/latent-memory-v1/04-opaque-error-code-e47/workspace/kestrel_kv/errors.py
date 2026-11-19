"""Structured error codes for KestrelKV storage subsystems."""

from __future__ import annotations

E12_CORRUPT_VARINT = "E12_CORRUPT_VARINT"
E23_BLOCK_MAGIC = "E23_BLOCK_MAGIC"
E31_SSTABLE_ORDER = "E31_SSTABLE_ORDER"
E47_MANIFEST_DRIFT = "E47_MANIFEST_DRIFT"
E52_COMPACTION_CONFLICT = "E52_COMPACTION_CONFLICT"


class KestrelError(Exception):
    """Domain exception carrying a machine-readable KestrelKV error code."""

    def __init__(self, code: str, detail: str = "") -> None:
        self.code = code
        self.detail = detail
        msg = f"{code}: {detail}" if detail else code
        super().__init__(msg)
