# KestrelKV Error Catalog

- `E12_CORRUPT_VARINT`: Truncated or overflowing LEB128 varint in block header.
- `E23_BLOCK_MAGIC`: Data block missing `KBLK` header signature.
- `E31_SSTABLE_ORDER`: Out-of-order or duplicate key passed to `SSTable.build`.
- `E47_MANIFEST_DRIFT`: Compacted segment failed `Manifest.validate_segment_fences`
  (`fence_key < block_max_key` or `fence_key >= next_block_min_key` in UTF-8 order).
- `E52_COMPACTION_CONFLICT`: Empty or conflicting compaction segment list.
