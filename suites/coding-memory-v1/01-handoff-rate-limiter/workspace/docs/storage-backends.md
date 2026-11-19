# Window State Storage Backends

`InMemoryWindowStore` stores immutable `TenantWindowState` records keyed by
`tenant_id`. Eviction sweeps may purge entries older than 60 windows during idle
maintenance cycles without affecting active tenants.
