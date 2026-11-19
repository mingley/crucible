# TLS Termination Profile

Ingress listeners require TLS 1.3 with AEAD cipher suites (`TLS_AES_128_GCM_SHA256`
or `TLS_CHACHA20_POLY1305_SHA256`). Session tickets rotate every 4 hours via
local file watches.
