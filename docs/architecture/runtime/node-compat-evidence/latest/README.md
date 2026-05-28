# Node.js Runtime Support Evidence Snapshot

This directory is the checked-in latest snapshot of the generated Node.js runtime support evidence outputs.

- evidence_generated_at: `2026-05-28T17:25:48.420213+00:00`
- publish_root: `docs/architecture/runtime/node-compat-evidence/latest`
- status source: `target/node-compat/status/status-summary.json`
- dashboard source: `target/node-compat/dashboard/dashboard-summary.json`

## Node Test Results

| Lane | Upstream | Vendored test files | Documented passed | Unclassified | Pass rate |
| --- | --- | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 1308 | 901 | 0 | 68.9% |
| `node22` | `v22.15.0` | 1283 | 873 | 0 | 68.0% |
| `node24` | `v24.15.0` | 1495 | 922 | 0 | 61.7% |

## Expectation Coverage

- Rust ignored tests: 67
- catalog entries: 67
- catalog path: `tests/runtime/node/expectations/rust-watchpoints.json`
- unexpected passes: 0

## Dashboard Coverage

- representative Node test checks: 8
- package/framework canary claims: 12
- package/framework canary checks: 26
- canary artifact bundles: 2
- oracle reports: 2
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
