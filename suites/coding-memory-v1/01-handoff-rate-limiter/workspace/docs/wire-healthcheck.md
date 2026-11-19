# Liveness & Readiness Probes

Health probes hit `/_healthz` directly on the admin port and bypass
`GatewayMiddleware` so synthetic probe traffic never consumes tenant sustained
or burst quotas.
