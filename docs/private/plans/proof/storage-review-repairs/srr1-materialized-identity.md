# SRR1 Complete Materialized Identity

Date: 2026-08-26
Work commit: `0a4ba6119`

## Fail-before

The two new regression tests failed against the baseline:

- `materialized_position_covers_resource_bindings_and_trigger_cursor` failed
  because a resource binding did not change position version 2.
- `full_verification_root_covers_bindings_and_trigger_cursor` failed because a
  resource binding did not change verification root version 1.

## Change

- Materialized position version 3 covers sorted resource bindings and the
  trigger delivery cursor.
- Verification root version 2 adds binding and cursor leaf families.
- Applied document and trigger records publish exact incremental deltas for
  those leaf families.
- Snapshot fingerprints report binding count and trigger cursor sequence.
- Mismatch diagnostics name `resource_path_bindings` or
  `trigger_delivery_cursor`.
- PITR and whole-deployment backup envelopes use version 3. They reject an
  older position codec before nested payload decoding.

## Verification

| Command | Result |
|---|---|
| `cargo test -p nimbus-storage materialized_position` | 16 passed |
| `cargo test -p nimbus-storage materialized_verification::tests` | 24 passed |
| `cargo test -p nimbus-storage store::journal_snapshot::tests` | 17 passed |
| `cargo test -p nimbus-engine verification::tests` | 2 passed |
| `cargo test -p nimbus-cli backup_decode` | 2 passed |
| `cargo test -p nimbus-cli restore_manifest_decode_reports_archive_version_before_nested_position` | 1 passed |
| `cargo test -p nimbus-server http::metadata::tests` | 1 passed |
| `cargo test -p nimbus-storage` | 395 passed, 3 ignored; one integration test ignored by its dedicated plan gate |
| `cargo fmt --all --check` | passed |
| `git diff --check` | passed |

The warnings came from unchanged vendored dependencies.
