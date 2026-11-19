# Tenant Identity Extraction

Upstream TLS terminators validate client mTLS certificates and populate the
verified `tenant_id` field on `GatewayRequest` prior to invoking
`GatewayMiddleware.handle`. Unauthenticated traffic is rejected before rate
policy lookup.
