# Worker Heartbeat Leases

Assigned job leases renew every 5 seconds and expire after 3 missed heartbeats,
returning unfinished attempts to the ready queue.
