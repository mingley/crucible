# Snapshot Isolation Guarantees

Every read operation (`get` or `scan`) selects an immutable `read_ts` and must
observe the exact database state committed at or before `read_ts`:

- Versions committed strictly after `read_ts` MUST NEVER be visible (no dirty or
  non-repeatable reads, and no phantom rows).
- Rows visible at `read_ts` MUST NOT vanish if overwritten or deleted by a
  later transaction while the read is still in flight.
