---
status: done
phase: SATH7
---

# SATH7 Format Versioning

Nimbus now has an explicit `StorageFormatVersion` surface and unknown future
versions fail closed through validation. The version is included in storage
health diagnostics.

Evidence:

- `crates/nimbus-storage/src/format.rs`
- `CURRENT_STORAGE_FORMAT_VERSION`
- `storage_format_version`
- `validate_storage_format_version`
- `unknown_storage_format_version_is_rejected`
