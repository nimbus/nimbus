---
status: done
phase: SATH6
---

# SATH6 Capabilities Health

Storage now exposes machine-readable capabilities and health diagnostics for
the current backend posture. Diagnostics include backend layout, journal head,
applied head, retention floor, format version, encryption posture, freshness
lag, recovery status, and exact-summary support.

Evidence:

- `crates/nimbus-storage/src/diagnostics.rs`
- `StorageCapabilities`
- `StorageHealthDiagnostic`
- `storage_health_diagnostic_reports_backend_layout_and_heads`
- `docs/operating/storage-backends.md`
