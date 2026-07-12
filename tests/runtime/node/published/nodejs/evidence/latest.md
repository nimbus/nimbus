# Node.js Runtime Evidence

This page is generated from the checked-in Node.js runtime support evidence snapshots.
It is a support summary, not a blanket Node.js compatibility claim.

## Snapshot

- generated at: `2026-07-10T18:59:53.275575+00:00`
- status source: `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`
- dashboard source: `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`
- trend source: `tests/runtime/node/compat/node-compat-evidence/latest/trend-summary.json`

## Node Test Results

| Target | Role | Upstream | Vendored official fixtures | Passed | Expected failure / known gap | Skipped / excluded | Unclassified | Official fixture pass rate | Classified coverage |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Node20 | Legacy grace; EOL | `v20.20.2` | 4248 | 919 | 3316 | 13 | 0 | 21.6% | 100.0% |
| Node22 | Supported; Maintenance LTS | `v22.22.3` | 4748 | 2363 | 2365 | 20 | 0 | 49.8% | 100.0% |
| Node24 | Product default; Active LTS | `v24.16.0` | 5198 | 2400 | 2750 | 48 | 0 | 46.2% | 100.0% |
| Node26 | Current | `v26.2.0` | 5578 | 2092 | 3432 | 54 | 0 | 37.5% | 100.0% |

## Package/Framework Canaries

| Package | Preset | Lane | Pinned version | Evidence | Support boundary | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `@anthropic-ai/sdk` | Application | Node22 | `0.100.0` | Support | Supported | Passed |
| `@aws-sdk/client-s3` | Application | Node22 | `3.1056.0` | Support | Supported | Passed |
| `axios` | Application | Node22 | `1.7.7` | Support | Supported | Passed |
| `convex-use-node-action` | Application | Node22 | `nimbus` | Support | Supported | Passed |
| `convex-use-node-real-app` | Application | Node22 | `nimbus` | Support | Supported | Passed |
| `express` | Application | Node22 | `4.19.2` | Support | Supported | Passed |
| `fastify` | Application | Node22 | `4.28.1` | Support | Supported | Passed |
| `node:child_process` | Application | Node22 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `esbuild` | Application | Node22 | `0.28.0` | Diagnostic | Service/microVM required | Passed |
| `node:inspector` | Application | Node22 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `native-addon` | Application | Node22 | `nimbus` | Diagnostic | Service/microVM required | Passed |
| `node --test` | Application | Node22 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `persistent-filesystem` | Application | Node22 | `nimbus` | Diagnostic | Service/microVM required | Passed |
| `prisma` | Application | Node22 | `engine-boundary` | Diagnostic | Service/microVM required | Passed |
| `raw-server-listen` | Application | Node22 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `node:repl` | Application | Node22 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `sharp` | Application | Node22 | `0.34.5` | Diagnostic | Service/microVM required | Passed |
| `node:worker_threads` | Application | Node22 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `jose` | Application | Node22 | `6.2.3` | Support | Supported | Passed |
| `nanoid` | Application | Node22 | `5.1.11` | Support | Supported | Passed |
| `node-platform-builtins` | Application | Node22 | `builtin` | Support | Supported | Passed |
| `octokit` | Application | Node22 | `5.0.5` | Support | Supported | Passed |
| `openai` | Application | Node22 | `6.39.1` | Support | Supported | Passed |
| `resend` | Application | Node22 | `6.12.4` | Support | Supported | Passed |
| `@slack/web-api` | Application | Node22 | `7.16.0` | Support | Supported | Passed |
| `socket.io` | Application | Node22 | `4.7.5` | Support | Supported | Passed |
| `stripe` | Application | Node22 | `22.2.0` | Support | Supported | Passed |
| `undici` | Application | Node22 | `6.19.8` | Support | Supported | Passed |
| `@upstash/redis` | Application | Node22 | `1.38.0` | Support | Supported | Passed |
| `uuid` | Application | Node22 | `14.0.0` | Support | Supported | Passed |
| `ai` | Application | Node22 | `6.0.192` | Support | Supported | Passed |
| `zod` | Application | Node22 | `4.4.3` | Support | Supported | Passed |
| `@anthropic-ai/sdk` | Application | Node24 | `0.100.0` | Support | Supported | Passed |
| `@aws-sdk/client-s3` | Application | Node24 | `3.1056.0` | Support | Supported | Passed |
| `axios` | Application | Node24 | `1.7.7` | Support | Supported | Passed |
| `convex-use-node-action` | Application | Node24 | `nimbus` | Support | Supported | Passed |
| `convex-use-node-real-app` | Application | Node24 | `nimbus` | Support | Supported | Passed |
| `express` | Application | Node24 | `4.19.2` | Support | Supported | Passed |
| `fastify` | Application | Node24 | `4.28.1` | Support | Supported | Passed |
| `node:child_process` | Application | Node24 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `esbuild` | Application | Node24 | `0.28.0` | Diagnostic | Service/microVM required | Passed |
| `node:inspector` | Application | Node24 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `native-addon` | Application | Node24 | `nimbus` | Diagnostic | Service/microVM required | Passed |
| `node --test` | Application | Node24 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `persistent-filesystem` | Application | Node24 | `nimbus` | Diagnostic | Service/microVM required | Passed |
| `prisma` | Application | Node24 | `engine-boundary` | Diagnostic | Service/microVM required | Passed |
| `raw-server-listen` | Application | Node24 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `node:repl` | Application | Node24 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `sharp` | Application | Node24 | `0.34.5` | Diagnostic | Service/microVM required | Passed |
| `node:worker_threads` | Application | Node24 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `jose` | Application | Node24 | `6.2.3` | Support | Supported | Passed |
| `nanoid` | Application | Node24 | `5.1.11` | Support | Supported | Passed |
| `node-platform-builtins` | Application | Node24 | `builtin` | Support | Supported | Passed |
| `octokit` | Application | Node24 | `5.0.5` | Support | Supported | Passed |
| `openai` | Application | Node24 | `6.39.1` | Support | Supported | Passed |
| `resend` | Application | Node24 | `6.12.4` | Support | Supported | Passed |
| `@slack/web-api` | Application | Node24 | `7.16.0` | Support | Supported | Passed |
| `socket.io` | Application | Node24 | `4.7.5` | Support | Supported | Passed |
| `stripe` | Application | Node24 | `22.2.0` | Support | Supported | Passed |
| `undici` | Application | Node24 | `6.19.8` | Support | Supported | Passed |
| `@upstash/redis` | Application | Node24 | `1.38.0` | Support | Supported | Passed |
| `uuid` | Application | Node24 | `14.0.0` | Support | Supported | Passed |
| `ai` | Application | Node24 | `6.0.192` | Support | Supported | Passed |
| `zod` | Application | Node24 | `4.4.3` | Support | Supported | Passed |
| `@anthropic-ai/sdk` | Application | Node26 | `0.100.0` | Support | Supported | Passed |
| `@aws-sdk/client-s3` | Application | Node26 | `3.1056.0` | Support | Supported | Passed |
| `convex-use-node-real-app` | Application | Node26 | `nimbus` | Support | Supported | Passed |
| `node:child_process` | Application | Node26 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `esbuild` | Application | Node26 | `0.28.0` | Diagnostic | Service/microVM required | Passed |
| `node:inspector` | Application | Node26 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `native-addon` | Application | Node26 | `nimbus` | Diagnostic | Service/microVM required | Passed |
| `node --test` | Application | Node26 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `persistent-filesystem` | Application | Node26 | `nimbus` | Diagnostic | Service/microVM required | Passed |
| `prisma` | Application | Node26 | `engine-boundary` | Diagnostic | Service/microVM required | Passed |
| `raw-server-listen` | Application | Node26 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `node:repl` | Application | Node26 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `sharp` | Application | Node26 | `0.34.5` | Diagnostic | Service/microVM required | Passed |
| `node:worker_threads` | Application | Node26 | `builtin` | Diagnostic | Service/microVM required | Passed |
| `jose` | Application | Node26 | `6.2.3` | Support | Supported | Passed |
| `nanoid` | Application | Node26 | `5.1.11` | Support | Supported | Passed |
| `octokit` | Application | Node26 | `5.0.5` | Support | Supported | Passed |
| `openai` | Application | Node26 | `6.39.1` | Support | Supported | Passed |
| `resend` | Application | Node26 | `6.12.4` | Support | Supported | Passed |
| `@slack/web-api` | Application | Node26 | `7.16.0` | Support | Supported | Passed |
| `stripe` | Application | Node26 | `22.2.0` | Support | Supported | Passed |
| `@upstash/redis` | Application | Node26 | `1.38.0` | Support | Supported | Passed |
| `uuid` | Application | Node26 | `14.0.0` | Support | Supported | Passed |
| `ai` | Application | Node26 | `6.0.192` | Support | Supported | Passed |
| `zod` | Application | Node26 | `4.4.3` | Support | Supported | Passed |
| `jest` | Tooling | Node22 | `30.4.2` | Support | Supported | Passed |
| `next` | Tooling | Node22 | `16.2.6` | Support | Supported | Passed |
| `prisma` | Tooling | Node22 | `7.8.0` | Support | Supported | Passed |
| `ts-node` | Tooling | Node22 | `10.9.2` | Support | Supported | Passed |
| `tsx` | Tooling | Node22 | `4.21.0` | Support | Supported | Passed |
| `jest` | Tooling | Node24 | `30.4.2` | Support | Supported | Passed |
| `next` | Tooling | Node24 | `16.2.6` | Support | Supported | Passed |
| `prisma` | Tooling | Node24 | `7.8.0` | Support | Supported | Passed |
| `ts-node` | Tooling | Node24 | `10.9.2` | Support | Supported | Passed |
| `tsx` | Tooling | Node24 | `4.21.0` | Support | Supported | Passed |

## Oracle Checks

| Lane | Fixture | Runtime | Oracle | Drift | Node oracle |
| --- | --- | --- | --- | --- | --- |
| Node20 | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement Pass | `v20.20.2` |
| Node22 | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement Pass | `v22.23.1` |
| Node24 | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement Pass | `v24.16.0` |
| Node26 | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement Pass | `v26.0.0` |

## Notes

- `Passed` fixtures and canaries may support public claims.
- Expected failures, known gaps, skips, and unclassified fixtures are not pass claims.
- Product default is a routing default, not an evidence priority.
- Node22 and Node24 are the current supported LTS lanes; Node20 is legacy-grace regression coverage after its 2026-04-30 EOL.
