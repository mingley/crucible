"""KestrelKV embedded LSM key-value engine."""

from .block import DataBlockBuilder, DataBlockReader, shortest_separator
from .bloom import BloomFilter
from .compactor import Compactor
from .db import KestrelDB
from .errors import (
    E12_CORRUPT_VARINT,
    E23_BLOCK_MAGIC,
    E31_SSTABLE_ORDER,
    E47_MANIFEST_DRIFT,
    E52_COMPACTION_CONFLICT,
    KestrelError,
)
from .manifest import Manifest, SegmentMeta
from .sstable import IndexEntry, SSTable

__all__ = [
    "BloomFilter",
    "Compactor",
    "DataBlockBuilder",
    "DataBlockReader",
    "E12_CORRUPT_VARINT",
    "E23_BLOCK_MAGIC",
    "E31_SSTABLE_ORDER",
    "E47_MANIFEST_DRIFT",
    "E52_COMPACTION_CONFLICT",
    "IndexEntry",
    "KestrelDB",
    "KestrelError",
    "Manifest",
    "SSTable",
    "SegmentMeta",
    "shortest_separator",
]
