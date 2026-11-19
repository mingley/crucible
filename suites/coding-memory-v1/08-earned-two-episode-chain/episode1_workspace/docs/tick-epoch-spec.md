# ChronosSched `EpochTick` & Window Specification

## 1. `EpochTick` Resolution & Leap-Offset Table

- **Base unit:** `1 EpochTick = 250ms` (`TICK_MS = 250`).
- **Wall-clock conversion:** Every absolute wall-clock millisecond timestamp
  MUST convert through `ticks.wall_ms_to_tick(wall_ms)`:
  ```python
  wall_ms // 250 + leap_offset_ticks(wall_ms)
  ```
  where `LEAP_OFFSET_TABLE` adds cumulative tick offsets for every historical
  cutover with `cutover_wall_ms <= wall_ms` (`+4` at `1_700_000_000_000`, `+3` at
  `1_750_000_000_000`, `+5` at `1_800_000_000_000`). Never call
  `raw_unadjusted_tick` directly for wall-clock timestamps.
- **Relative duration conversion:** Elapsed durations (`window_span_ms`,
  `policy.budget_ms`) convert via ceiling ticks
  `ticks.duration_ms_to_ticks_ceil(ms)` and NEVER apply `LEAP_OFFSET_TABLE`.

## 2. Half-Open `[start_tick, end_tick)` Windows

All execution windows and maintenance blackout windows `[start_tick, end_tick)`
are **half-open**:

- Tick `t` is inside the window iff `start_tick <= t < end_tick`.
- Empty or inverted pairs (`end_tick <= start_tick`) represent zero ticks and
  are discarded by `calendar.merge_half_open_windows`.
- Overlapping and adjacent windows (`next_start <= prev_end`) coalesce into a
  single contiguous half-open interval.

## 3. SLA Breach Escalation (`compute_sla_breach_tick`)

Given `enqueue_wall_ms`, `policy: SlaPolicy`, and `blackout_windows`:

1. Compute `start_tick = wall_ms_to_tick(enqueue_wall_ms)` (raising `ValueError`
   if `enqueue_wall_ms < 0`).
2. Compute `required_active_ticks = duration_ms_to_ticks_ceil(policy.budget_ms) + policy.grace_ticks`.
3. Normalize `blackout_windows` with `merge_half_open_windows(blackout_windows)`.
4. Maintenance blackout ticks pause the SLA clock, and escalations never fire
   inside an active blackout window. Therefore, `compute_sla_breach_tick` returns
   the **first non-blackout tick at or after `start_tick` after
   `required_active_ticks` non-blackout ticks have elapsed**.
5. Because budgets and maintenance blackouts can span days (millions of ticks),
   jump across merged intervals in `O(K log K)` rather than stepping 1 tick at a time.
