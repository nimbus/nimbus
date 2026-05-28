# Node.js Runtime Evidence

This page is generated from the checked-in Node.js runtime support evidence snapshots.
It is a support summary, not a blanket Node.js compatibility claim.

## Snapshot

- generated at: `2026-05-28T17:25:48.420213+00:00`
- status source: `docs/architecture/runtime/node-compat-evidence/latest/status-summary.json`
- dashboard source: `docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.json`
- trend source: `docs/architecture/runtime/node-compat-evidence/latest/trend-summary.json`

## Node Test Results

| Target | Role | Upstream | Vendored official fixtures | Passed | Expected failure / known gap | Skipped / excluded | Unclassified | Official fixture pass rate | Classified coverage |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Node20 | Legacy grace; EOL | `v20.20.2` | 1308 | 901 | 402 | 5 | 0 | 68.9% | 100.0% |
| Node22 | Product default; Maintenance LTS | `v22.15.0` | 1283 | 873 | 406 | 4 | 0 | 68.0% | 100.0% |
| Node24 | Supported; Active LTS | `v24.15.0` | 1495 | 922 | 570 | 3 | 0 | 61.7% | 100.0% |

## Package/Framework Canaries

| Package | Preset | Lane | Pinned version | Status |
| --- | --- | --- | --- | --- |
| `express` | Application | Node20 | `4.19.2` | Passed |
| `fastify` | Application | Node20 | `4.28.1` | Passed |
| `axios` | Application | Node22 | `1.7.7` | Passed |
| `convex-use-node-action` | Application | Node22 | `nimbus` | Passed |
| `express` | Application | Node22 | `4.19.2` | Passed |
| `fastify` | Application | Node22 | `4.28.1` | Passed |
| `node-platform-builtins` | Application | Node22 | `builtin` | Passed |
| `socket.io` | Application | Node22 | `4.7.5` | Passed |
| `undici` | Application | Node22 | `6.19.8` | Passed |
| `axios` | Application | Node24 | `1.7.7` | Passed |
| `convex-use-node-action` | Application | Node24 | `nimbus` | Passed |
| `express` | Application | Node24 | `4.19.2` | Passed |
| `fastify` | Application | Node24 | `4.28.1` | Passed |
| `node-platform-builtins` | Application | Node24 | `builtin` | Passed |
| `socket.io` | Application | Node24 | `4.7.5` | Passed |
| `undici` | Application | Node24 | `6.19.8` | Passed |
| `jest` | Tooling | Node22 | `30.4.2` | Passed |
| `next` | Tooling | Node22 | `16.2.6` | Passed |
| `prisma` | Tooling | Node22 | `7.8.0` | Passed |
| `ts-node` | Tooling | Node22 | `10.9.2` | Passed |
| `tsx` | Tooling | Node22 | `4.21.0` | Passed |
| `jest` | Tooling | Node24 | `30.4.2` | Passed |
| `next` | Tooling | Node24 | `16.2.6` | Passed |
| `prisma` | Tooling | Node24 | `7.8.0` | Passed |
| `ts-node` | Tooling | Node24 | `10.9.2` | Passed |
| `tsx` | Tooling | Node24 | `4.21.0` | Passed |

## Oracle Checks

| Lane | Fixture | Runtime | Oracle | Drift | Node oracle |
| --- | --- | --- | --- | --- | --- |
| Node22 | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement Pass | `v22.22.2` |
| Node24 | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement Pass | `v24.16.0` |

## Notes

- `Passed` fixtures and canaries may support public claims.
- Expected failures, known gaps, skips, and unclassified fixtures are not pass claims.
- Product default is a routing default, not an evidence priority.
- Node22 and Node24 are the current supported LTS lanes; Node20 is legacy-grace regression coverage after its 2026-04-30 EOL.
