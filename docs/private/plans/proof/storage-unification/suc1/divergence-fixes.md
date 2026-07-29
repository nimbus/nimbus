# SUC1.1 — Provider Divergence Fixes

- Postgres `validate_lease_request` now enforces the 1..=191-byte owner-id
  guard (parity with mysql/libsql); message text identical across providers.
- MySQL lease durations are canonical milliseconds (U3); the
  `TIMESTAMPADD(MICROSECOND, ...)` edge receives millis × 1000 with checked
  overflow. Observable behavior unchanged for all previously valid inputs.
- Each provider carries a validator parity unit test (no fixtures needed);
  full storage suite 435/435 under the documented fixture opt-out; clippy
  `-D warnings` clean.
