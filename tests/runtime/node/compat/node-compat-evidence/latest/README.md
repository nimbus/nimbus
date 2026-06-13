# Node.js Runtime Support Evidence Snapshot

This directory is the checked-in latest snapshot of the generated Node.js runtime support evidence outputs.

- evidence_generated_at: `2026-06-13T07:11:20.414653+00:00`
- publish_root: `tests/runtime/node/compat/node-compat-evidence/latest`
- status source: `target/node-compat/status/status-summary.json`
- dashboard source: `target/node-compat/dashboard/dashboard-summary.json`

## Node Test Results

| Lane | Upstream | Vendored test files | Documented passed | Unclassified | Pass rate |
| --- | --- | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 4248 | 917 | 0 | 21.6% |
| `node22` | `v22.22.3` | 4748 | 2304 | 0 | 48.5% |
| `node24` | `v24.16.0` | 5198 | 2334 | 0 | 44.9% |
| `node26` | `v26.2.0` | 5578 | 1009 | 0 | 18.1% |

## Expectation Coverage

- Rust ignored tests: 135
- catalog entries: 135
- catalog path: `tests/runtime/node/expectations/rust-watchpoints.json`
- unexpected passes: 0

## Dashboard Coverage

- representative Node test checks: 0
- package/framework canary claims: 79
- package/framework canary checks: 0
- canary artifact bundles: 0
- oracle reports: 0
- required canary gaps: 0

## Trend Coverage

- trend snapshot: `trend-summary.json` and `trend-summary.md`
- baseline available: `true`
- lane trend rows: 4
- evidence trend metrics: 8

## Files

- `status-summary.json` and `status-summary.md` are copied from `make node-compat-status`.
- `dashboard-summary.json` and `dashboard-summary.md` are copied from `make node-compat-dashboard`.
- `trend-summary.json` and `trend-summary.md` are copied from `make node-compat-trends` when present.
