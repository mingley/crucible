# Edge Deployment Topology

PulseGate instances run as stateless sidecars paired 1:1 with regional ingress
listeners. Each sidecar maintains local in-memory window slots partitioned by
consistent hash ring assignment from the upstream L4 load balancer.

## Rolling Restarts

During rolling restarts, connections drain over a 15-second grace period before
listener sockets close. Cold sidecars start with empty `InMemoryWindowStore`
tables and warm within two window intervals.
