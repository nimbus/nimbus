# Node.js Runtime Evidence

This page is generated from the checked-in Node.js runtime support evidence snapshots.
It is a support summary, not a blanket Node.js compatibility claim.

## Snapshot

- generated at: `2026-09-01T05:11:22.668294+00:00`
- status source: `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`
- dashboard source: `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`
- trend source: `tests/runtime/node/compat/node-compat-evidence/latest/trend-summary.json`

## Node Test Results

| Target | Role | Upstream | Vendored official fixtures | Passed | Expected failure / known gap | Skipped / excluded | Unclassified | Official fixture pass rate | Classified coverage |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Node20 | Legacy grace; EOL | `v20.20.2` | 4248 | 919 | 3316 | 13 | 0 | 21.6% | 100.0% |
| Node22 | Supported; Maintenance LTS | `v22.23.2` | 4762 | 2362 | 2380 | 20 | 0 | 49.6% | 100.0% |
| Node24 | Product default; Active LTS | `v24.20.0` | 5671 | 2397 | 3226 | 48 | 0 | 42.3% | 100.0% |
| Node26 | Current | `v26.8.1` | 5940 | 2090 | 3795 | 55 | 0 | 35.2% | 100.0% |

## Package/Framework Canaries

| Package | Preset | Lane | Pinned version | Evidence | Support boundary | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `node-platform-builtins` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `express` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `fastify` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `socket.io` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `undici` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `axios` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `convex-use-node-action` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `convex-use-node-real-app` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `openai` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `@anthropic-ai/sdk` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `ai` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `stripe` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `resend` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `@aws-sdk/client-s3` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `@slack/web-api` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `octokit` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `jose` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `zod` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `uuid` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `nanoid` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `@upstash/redis` | Application | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `node:child_process` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `node:worker_threads` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `node:inspector` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `node:repl` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `node --test` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `native-addon` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `persistent-filesystem` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `raw-server-listen` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `prisma` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `sharp` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `esbuild` | Application | Node22, Node24 | n/a | Diagnostic | Service/microVM required | Missing Observation |
| `tsx` | Tooling | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `ts-node` | Tooling | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `jest` | Tooling | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `prisma` | Tooling | Node22, Node24 | n/a | Support | Supported | Missing Observation |
| `next` | Tooling | Node22, Node24 | n/a | Support | Supported | Missing Observation |

## Oracle Checks

| Lane | Fixture | Runtime | Oracle | Drift | Node oracle |
| --- | --- | --- | --- | --- | --- |

## Notes

- `Passed` fixtures and canaries may support public claims.
- Expected failures, known gaps, skips, and unclassified fixtures are not pass claims.
- Product default is a routing default, not an evidence priority.
- Node22 and Node24 are the current supported LTS lanes; Node20 is legacy-grace regression coverage after its 2026-04-30 EOL.
