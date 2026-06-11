# Test-only self-signed TLS fixtures

Self-signed localhost pair used by nimbus-server TLS tests. Not a secret:
the key exists only to exercise the TLS code path in tests.

Regenerate:

```bash
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
  -keyout localhost-key.pem -out localhost-cert.pem -days 36500 -nodes \
  -subj "/CN=localhost" \
  -addext "subjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1"
```
