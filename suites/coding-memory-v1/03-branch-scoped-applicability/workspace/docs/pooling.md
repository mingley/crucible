# Stream Pool Sizing

`ConnectionPool` pre-allocates stream IDs `1..capacity` in FIFO order so idle
streams are reused predictably across round-robin workers.
