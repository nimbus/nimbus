# Node.js Runtime Support Evidence Snapshot

This directory is the checked-in latest snapshot of the generated Node.js runtime support evidence outputs.

- evidence_generated_at: `2026-06-01T15:09:55.591130+00:00`
- publish_root: `docs/architecture/runtime/node-compat-evidence/latest`
- status source: `target/node-compat/status/status-summary.json`
- dashboard source: `target/node-compat/dashboard/dashboard-summary.json`

## Node Test Results

| Lane | Upstream | Vendored test files | Documented passed | Unclassified | Pass rate |
| --- | --- | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 1308 | 893 | 0 | 68.3% |
| `node22` | `v22.22.3` | 4773 | 1023 | 0 | 21.4% |
| `node24` | `v24.16.0` | 5198 | 892 | 0 | 17.2% |
| `node26` | `v26.2.0` | 5578 | 0 | 0 | 0.0% |

## Expectation Coverage

- Rust ignored tests: 76
- catalog entries: 76
- catalog path: `tests/runtime/node/expectations/rust-watchpoints.json`
- unexpected passes: 0

## Dashboard Coverage

- representative Node test checks: 0
- package/framework canary claims: 37
- package/framework canary checks: 0
- canary artifact bundles: 0
- oracle reports: 0
- required canary gaps: 0

## Trend Coverage

- trend snapshot: unavailable; run `make node-compat-trends` before publishing

## Files

- `status-summary.json` and `status-summary.md` are copied from `make node-compat-status`.
- `dashboard-summary.json` and `dashboard-summary.md` are copied from `make node-compat-dashboard`.
- `trend-summary.json` and `trend-summary.md` are copied from `make node-compat-trends` when present.
