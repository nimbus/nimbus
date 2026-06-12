# Node.js Runtime Evidence

This page is generated from the checked-in Node.js runtime support evidence snapshots.
It is a support summary, not a blanket Node.js compatibility claim.

## Snapshot

- generated at: `2026-06-12T00:53:07.439961+00:00`
- status source: `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`
- dashboard source: `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`
- trend source: `tests/runtime/node/compat/node-compat-evidence/latest/trend-summary.json`

## Node Test Results

| Target | Role | Upstream | Vendored official fixtures | Passed | Expected failure / known gap | Skipped / excluded | Unclassified | Official fixture pass rate | Classified coverage |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Node20 | Legacy grace; EOL | `v20.20.2` | 4248 | 917 | 3318 | 13 | 0 | 21.6% | 100.0% |
| Node22 | Supported; Maintenance LTS | `v22.22.3` | 4748 | 2299 | 2429 | 20 | 0 | 48.4% | 100.0% |
| Node24 | Product default; Active LTS | `v24.16.0` | 5198 | 2328 | 2822 | 48 | 0 | 44.8% | 100.0% |
| Node26 | Current | `v26.2.0` | 5578 | 1009 | 4523 | 46 | 0 | 18.1% | 100.0% |

## Package/Framework Canaries

| Package | Preset | Lane | Pinned version | Evidence | Support boundary | Status |
| --- | --- | --- | --- | --- | --- | --- |

## Oracle Checks

| Lane | Fixture | Runtime | Oracle | Drift | Node oracle |
| --- | --- | --- | --- | --- | --- |

## Notes

- `Passed` fixtures and canaries may support public claims.
- Expected failures, known gaps, skips, and unclassified fixtures are not pass claims.
- Product default is a routing default, not an evidence priority.
- Node22 and Node24 are the current supported LTS lanes; Node20 is legacy-grace regression coverage after its 2026-04-30 EOL.
