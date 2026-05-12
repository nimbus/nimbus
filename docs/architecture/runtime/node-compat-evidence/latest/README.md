# Node Compatibility Evidence Snapshot

This directory is the checked-in latest snapshot of the generated Node compatibility evidence outputs.

- evidence_generated_at: `2026-05-12T01:03:02.236934+00:00`
- publish_root: `docs/architecture/runtime/node-compat-evidence/latest`
- status source: `target/node-compat/status/status-summary.json`
- dashboard source: `target/node-compat/dashboard/dashboard-summary.json`

## Lane Denominators

| Lane | Upstream | Vendored test files | Documented green | Unmanifested/unclassified | Ratio |
| --- | --- | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 1308 | 913 | 395 | 69.8% |
| `node22` | `v22.15.0` | 1283 | 994 | 0 | 77.5% |
| `node24` | `v24.15.0` | 1495 | 925 | 570 | 61.9% |

## Expectation Coverage

- Rust ignored tests: 61
- catalog entries: 61
- catalog path: `tests/node-compat/expectations/rust-watchpoints.json`
- unexpected passes: 0

## Dashboard Coverage

- slice reports: 8
- canary reports: 2
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
