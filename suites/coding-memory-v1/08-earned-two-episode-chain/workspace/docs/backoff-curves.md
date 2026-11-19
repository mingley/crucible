# Transient Failure Backoff Curves

Failed task attempts back off according to `min(max_delay, base_delay * 2 ** attempt)`
capped at 15 minutes.
