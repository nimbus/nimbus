# Node.js Runtime Support Evidence Snapshot

This directory is the checked-in latest snapshot of the generated Node.js runtime support evidence outputs.

- evidence_generated_at: `2026-09-22T18:30:20.746144+00:00`
- publish_root: `tests/runtime/node/compat/node-compat-evidence/latest`
- status source: `target/node-compat/status/status-summary.json`
- dashboard source: `target/node-compat/dashboard/dashboard-summary.json`

## Node Test Results

| Lane | Upstream | Vendored test files | Documented passed | Unclassified | Pass rate |
| --- | --- | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 4248 | 938 | 0 | 22.1% |
| `node22` | `v22.23.2` | 4762 | 2364 | 0 | 49.6% |
| `node24` | `v24.21.0` | 5729 | 2399 | 0 | 41.9% |
| `node26` | `v26.10.0` | 6201 | 2093 | 0 | 33.8% |

## Expectation Coverage

- Rust ignored tests: 129
- catalog entries: 129
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
