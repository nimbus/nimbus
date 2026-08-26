# SA10 Consistency Verification Observability

Date: 2026-08-26  
Baseline: `5dc55cf25`  
Work commit: `47ddbbc7b`  
Merge commit: `56682f558c91603b87a3cc9535b2306e6332708d`  
Pull request: #326

## Result

The tenant consistency route remains a report-semantics endpoint, but every
request now produces operator-visible structured evidence.

- A clean report emits an `INFO` event with action, tenant, requested mode,
  escalation reason, mismatch count, event count, and anchor sequence.
- A divergent report emits a `WARN` event and names the first failed
  invariant.
- A verification failure emits a `WARN` event before the existing error maps
  to the HTTP response.
- The operator guide defines the warning event as the alert hook for scheduled
  verification.
- The server route test now proves that all three fingerprints contain a
  serialized `position.state_digest` and compares the complete positions.

The response shape, consistency algorithm, and error mapping did not change.

## Fail-Before Evidence

`SnapshotFingerprint` serializes a `position` object and has no top-level
`digest` field. The route test indexed `report[scope]["digest"]`, so both sides
of each equality were `serde_json::Value::Null`. The assertions passed without
checking a digest or position.

The endpoint returned the report or HTTP error without a structured tracing
event. A scheduled caller could receive a mismatch report with no server-side
operator signal.

## Verification

```text
cargo test -p nimbus-server \
  consistency_reports_emit_structured_info_and_warning_events -- --exact
1 passed; 0 failed

cargo test -p nimbus-server \
  tenant_consistency_route_returns_green_report_for_live_state -- --exact
1 passed; 0 failed

cargo test -p nimbus-server --lib -- --test-threads=1
666 passed; 0 failed; 35 ignored

cargo clippy -p nimbus-server --all-targets -- -D warnings
PASS

cargo fmt --all --check
PASS

bash scripts/check-docs.sh
PASS; 109 pages checked

npm --prefix website run build
PASS; 110 pages built

bash scripts/verify-nimbus-docs-site.sh
PASS; 17 passed, 0 failed

Nimbus autoreview --gate pre-pr --mode auto
PASS; no accepted or actionable findings; Trufflehog clean

make ci
PASS; format, strict workspace Clippy, dependency policy, runtime tests,
7,659-test nextest workspace lane, required verification harness, JavaScript
build and typecheck, 95 UI files / 832 UI tests, and proof helpers exited 0
```

Plain parallel `cargo test -p nimbus-server` exposed the existing global
network-authority fixture collision: 623 tests passed and 43 failed with
`DuplicateProcessComposition`. The first failed test passed alone, the full
serialized crate run passed, and the repository nextest lane passed the same
machine-lifecycle, service-manager, workload-composition, and tenant-isolation
tests.
