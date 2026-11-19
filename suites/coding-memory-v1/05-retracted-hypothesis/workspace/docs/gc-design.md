# Garbage Collection & Reader Watermarks

`GarbageCollector.collect()` computes `safe_horizon = min(min_active_read_ts, current_ts)`
when at least one reader lease is active in `WatermarkTracker`, or `current_ts`
when no active readers exist.

## Reader Pinning Protocol

Any multi-step or streaming read (including `MVCCEngine.scan()`) MUST:

1. Register `effective_ts` in `self.watermarks` **before** capturing
   `_acquire_segment_snapshot()`.
2. Release the watermark lease in a `finally` block (and when the iterator is
   closed or exhausted) so subsequent `run_gc()` passes can reclaim obsolete
   versions once the scan finishes.

> **Deprecated Knob Warning:** Never enable `allow_unpinned_gc = True`. Falling
> back to `_head_cache` returns post-snapshot writes and violates snapshot isolation.
