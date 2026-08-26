# SA7 Archive Version Diagnostics

Date: 2026-08-26  
Baseline: `18c8a44fa`  
Work commit: `fb19455d4`  
Merge commit: `8ca40cc218503a7694454a21d037547b76d39637`  
Pull request: #325

## Result

Backup and PITR readers now validate the owning archive version before they
decode nested materialized-position payloads.

- `PointInTimeRestoreArchive::decode_json` parses and validates the archive
  header before typed payload decode. An old archive reports that it predates
  the materialized-position digest codec v2 and must be recreated.
- The pre-launch whole-deployment backup format is now version 2. Restore reads
  its outer header first, then delegates each tenant archive to the
  storage-owned decoder with tenant and path context.
- Object-backup manifest restore uses the same storage-owned decoder.
- Current archives still round-trip and validate. A future archive reports its
  found and supported versions without claiming that it predates the codec.

The change improves diagnostics only. Unsupported old archives already failed
closed, and SA7 does not add a compatibility or migration path.

## Fail-Before Evidence

The new regressions first ran against the baseline:

```text
storage legacy PITR archive
serialization error: invalid input: unsupported materialized position version 1
at line 8 column 5

CLI legacy whole-deployment backup
backup file legacy.json is not a valid nimbus backup: invalid input:
unsupported materialized position version 1 at line 12 column 17
```

Both failures came from nested typed deserialization before the owning
container-version check. The object-backup manifest reader also used direct
typed JSON deserialization and had the same ordering defect.

## Verification

```text
cargo test -p nimbus-storage pitr_json_decode
3 passed; 0 failed

cargo test -p nimbus-cli backup_file_decode_rejects
2 passed; 0 failed

cargo test -p nimbus-cli object_backup_manifest_decode_rejects
1 passed; 0 failed

cargo test -p nimbus-storage
PASS; 394 storage tests invoked

cargo test -p nimbus-cli
1,061 passed; 0 failed; 4 ignored

cargo clippy -p nimbus-storage -p nimbus-cli --all-targets -- -D warnings
PASS

cargo fmt --all --check
PASS

Nimbus autoreview --gate pre-pr --mode auto
PASS; no actionable findings

make ci
Runtime tests, format, Clippy, dependency policy, and 7,657 of 7,658 workspace
tests passed. One unrelated MongoDB spec test did not observe its listener
before the server exited under load.

NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
  cargo nextest run -p nimbus-server --test mongodb_spec \
  spec_executor_crud_execution_report
1 passed; 22 skipped

make test-rust-docs verify-harness build-js typecheck-js test-js proof-helpers
PASS; Rust doc tests, required storage/engine/server/runtime campaigns,
JavaScript builds and type checks, 832 UI tests, package tests, and all proof
helpers completed with exit 0
```
