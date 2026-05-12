# Node.js Runtime Evidence

This page is generated from the checked-in Node.js runtime support evidence snapshots.
It is a support summary, not a blanket Node.js compatibility claim.

## Snapshot

- generated at: `2026-05-12T16:10:29.966284+00:00`
- status source: `docs/architecture/runtime/node-compat-evidence/latest/status-summary.json`
- dashboard source: `docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.json`
- trend source: `docs/architecture/runtime/node-compat-evidence/latest/trend-summary.json`

## Node Test Results

| Target | Role | Upstream | Vendored official fixtures | Passed | Expected failure / known gap | Skipped / excluded | Unclassified | Official fixture pass rate | Classified coverage |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Node20 | Supported | `v20.20.2` | 1308 | 904 | 399 | 5 | 0 | 69.1% | 100.0% |
| Node22 | Default | `v22.15.0` | 1283 | 876 | 403 | 4 | 0 | 68.3% | 100.0% |
| Node24 | Supported | `v24.15.0` | 1495 | 925 | 567 | 3 | 0 | 61.9% | 100.0% |

## Package/Framework Canaries

| Package | Preset | Lane | Pinned version | Status |
| --- | --- | --- | --- | --- |
| `express` | Application | Node20 | `4.19.2` | Passed |
| `fastify` | Application | Node20 | `4.28.1` | Passed |
| `axios` | Application | Node22 | `1.7.7` | Passed |
| `express` | Application | Node22 | `4.19.2` | Passed |
| `fastify` | Application | Node22 | `4.28.1` | Passed |
| `socket.io` | Application | Node22 | `4.7.5` | Passed |
| `undici` | Application | Node22 | `6.19.8` | Passed |
| `jest` | Tooling | Node22 | `30.4.2` | Passed |
| `next` | Tooling | Node22 | `16.2.6` | Passed |
| `prisma` | Tooling | Node22 | `7.8.0` | Passed |
| `ts-node` | Tooling | Node22 | `10.9.2` | Passed |
| `tsx` | Tooling | Node22 | `4.21.0` | Passed |

## Oracle Checks

| Lane | Fixture | Runtime | Oracle | Drift | Node oracle |
| --- | --- | --- | --- | --- | --- |
| Node22 | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement Pass | `v22.22.2` |

## Notes

- `Passed` fixtures and canaries may support public claims.
- Expected failures, known gaps, skips, and unclassified fixtures are not pass claims.
- Node22 remains the default target until an explicit Node24-default migration.
