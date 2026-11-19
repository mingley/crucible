# Zero-Downtime Secret Rotation

During secret rotation, upstream senders emit comma-separated signatures in
`X-Signature-256`, e.g.:

```
X-Signature-256: v0=deadbeef, v1=<old_hmac>, v1=<new_hmac>
```

Receivers configure both `primary_secret` and `secondary_secret` during the
24-hour overlap window before retiring the old secret.
