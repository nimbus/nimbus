# Node.js Runtime Support Evidence Snapshot

This directory is the checked-in latest snapshot of the generated Node.js runtime support evidence outputs.

- evidence_generated_at: `2026-05-12T16:10:29.966284+00:00`
- publish_root: `docs/architecture/runtime/node-compat-evidence/latest`
- status source: `target/node-compat/status/status-summary.json`
- dashboard source: `target/node-compat/dashboard/dashboard-summary.json`

## Node Test Results

| Lane | Upstream | Vendored test files | Documented passed | Unclassified | Pass rate |
| --- | --- | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 1308 | 904 | 0 | 69.1% |
| `node22` | `v22.15.0` | 1283 | 876 | 0 | 68.3% |
| `node24` | `v24.15.0` | 1495 | 925 | 0 | 61.9% |

## Expectation Coverage

- Rust ignored tests: 61
- catalog entries: 61
- catalog path: `tests/runtime/node/expectations/rust-watchpoints.json`
- unexpected passes: 0

## Dashboard Coverage

- representative Node test checks: 8
- package/framework canary claims: 10
- package/framework canary checks: 12
- canary artifact bundles: 2
- oracle reports: 1
- required canary gaps: 0

## Trend Coverage

- trend snapshot: `trend-summary.json` and `trend-summary.md`
- baseline available: `true`
- lane trend rows: 3
- evidence trend metrics: 8

## Files

- `status-summary.json` and `status-summary.md` are copied from `make node-compat-status`.
- `dashboard-summary.json` and `dashboard-summary.md` are copied from `make node-compat-dashboard`.
- `trend-summary.json` and `trend-summary.md` are copied from `make node-compat-trends` when present.
