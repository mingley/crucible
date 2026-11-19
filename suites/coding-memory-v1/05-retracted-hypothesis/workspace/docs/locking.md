# Optimistic Commit Locking

`KeyLockTable` acquires per-key locks in sorted lexicographical key order during
`commit_writes` to prevent deadlocks across overlapping multi-key transactions.
