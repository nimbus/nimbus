# Node.js Runtime Support Evidence Snapshot

This directory is the checked-in latest snapshot of the generated Node.js runtime support evidence outputs.

- evidence_generated_at: `2026-09-04T20:01:27.628894+00:00`
- publish_root: `tests/runtime/node/compat/node-compat-evidence/latest`
- status source: `target/node-compat/status/status-summary.json`
- dashboard source: `target/node-compat/dashboard/dashboard-summary.json`

## Node Test Results

| Lane | Upstream | Vendored test files | Documented passed | Unclassified | Pass rate |
| --- | --- | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 4248 | 919 | 0 | 21.6% |
| `node22` | `v22.23.2` | 4762 | 2362 | 0 | 49.6% |
| `node24` | `v24.20.0` | 5671 | 2397 | 0 | 42.3% |
| `node26` | `v26.8.1` | 5940 | 2090 | 0 | 35.2% |

## Expectation Coverage

- Rust ignored tests: 150
- catalog entries: 150
- catalog path: `tests/runtime/node/expectations/rust-watchpoints.json`
- unexpected passes: 0

## Dashboard Coverage

- representative Node test checks: 5
- package/framework canary claims: 79
- package/framework canary checks: 101
- canary artifact bundles: 2
- oracle reports: 4
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
