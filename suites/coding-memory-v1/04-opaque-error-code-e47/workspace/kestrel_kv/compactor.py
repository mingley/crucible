"""Multi-segment k-way merge compactor for KestrelKV."""

from __future__ import annotations

from .errors import E52_COMPACTION_CONFLICT, KestrelError
from .manifest import Manifest
from .sstable import SSTable


class Compactor:
    """Merges ordered SSTable segments newest-wins and drops deleted tombstones."""

    def __init__(self, manifest: Manifest, block_max_entries: int = 4) -> None:
        self._manifest = manifest
        self._block_max_entries = block_max_entries

    def compact(self, tables_oldest_first: list[SSTable]) -> SSTable | None:
        if not tables_oldest_first:
            raise KestrelError(E52_COMPACTION_CONFLICT, "no input segments supplied")

        merged_map: dict[str, bytes | None] = {}
        obsolete_ids: set[int] = set()
        for table in tables_oldest_first:
            obsolete_ids.add(table.segment_id)
            for key, val in table.iter_all():
                merged_map[key] = val

        live_records = sorted(
            ((k, v) for k, v in merged_map.items() if v is not None),
            key=lambda item: item[0].encode("utf-8"),
        )
        if not live_records:
            self._manifest.commit_compaction(obsolete_ids, None)
            return None

        seg_id = self._manifest.allocate_segment_id()
        compacted = SSTable.build(
            segment_id=seg_id,
            records=live_records,
            block_max_entries=self._block_max_entries,
        )
        self._manifest.commit_compaction(obsolete_ids, compacted)
        return compacted
