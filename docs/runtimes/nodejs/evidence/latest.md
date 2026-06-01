# Node.js Runtime Evidence

This page is generated from the checked-in Node.js runtime support evidence snapshots.
It is a support summary, not a blanket Node.js compatibility claim.

## Snapshot

- generated at: `2026-06-01T09:23:52.136897+00:00`
- status source: `docs/architecture/runtime/node-compat-evidence/latest/status-summary.json`
- dashboard source: `docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.json`
- trend source: `docs/architecture/runtime/node-compat-evidence/latest/trend-summary.json`

## Node Test Results

| Target | Role | Upstream | Vendored official fixtures | Passed | Expected failure / known gap | Skipped / excluded | Unclassified | Official fixture pass rate | Classified coverage |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Node20 | Legacy grace; EOL | `v20.20.2` | 1308 | 899 | 403 | 6 | 0 | 68.7% | 100.0% |
| Node22 | Supported; Maintenance LTS | `v22.22.3` | 4773 | 1024 | 3729 | 20 | 0 | 21.5% | 100.0% |
| Node24 | Product default; Active LTS | `v24.16.0` | 5198 | 323 | 4825 | 50 | 0 | 6.2% | 100.0% |
| Node26 | Current | `v26.2.0` | 5578 | 0 | 5529 | 49 | 0 | 0.0% | 100.0% |

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
