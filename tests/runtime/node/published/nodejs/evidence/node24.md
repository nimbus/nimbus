# Node24 Runtime Evidence

This page is generated from the checked-in Node compatibility evidence snapshots.

## Summary

- role: `Product default; Active LTS`
- support phase: `Active LTS`
- product default: `yes`
- evidence policy: `lane-local LTS evidence`
- upstream fixture line: `v24.16.0`
- runtime execution target: `Node24`
- vendored official fixtures: `5198`
- passed official fixtures: `2400`
- expected failure / known gap fixtures: `2750`
- skipped / excluded fixtures: `48`
- unclassified fixtures: `0`
- official fixture pass rate: `46.2%`
- classified coverage: `100.0%`

## Classification Catalog

- catalog: `tests/runtime/node/classifications/node24.json`

| Expectation | Count |
| --- | ---: |
| Expected failure | 4 |
| Known gap | 2746 |
| Skipped / excluded | 48 |

## Canary Coverage

| Package | Preset | Pinned version | Evidence | Support boundary | Status |
| --- | --- | --- | --- | --- | --- |
| `@anthropic-ai/sdk` | Application | `0.100.0` | Support | Supported | Passed |
| `@aws-sdk/client-s3` | Application | `3.1056.0` | Support | Supported | Passed |
| `axios` | Application | `1.20.0` | Support | Supported | Passed |
| `convex-use-node-action` | Application | `nimbus` | Support | Supported | Passed |
| `convex-use-node-real-app` | Application | `nimbus` | Support | Supported | Passed |
| `express` | Application | `4.22.2` | Support | Supported | Passed |
| `fastify` | Application | `5.12.1` | Support | Supported | Passed |
| `node:child_process` | Application | `builtin` | Diagnostic | Service/microVM required | Passed |
| `esbuild` | Application | `0.28.2` | Diagnostic | Service/microVM required | Passed |
| `node:inspector` | Application | `builtin` | Diagnostic | Service/microVM required | Passed |
| `native-addon` | Application | `nimbus` | Diagnostic | Service/microVM required | Passed |
| `node --test` | Application | `builtin` | Diagnostic | Service/microVM required | Passed |
| `persistent-filesystem` | Application | `nimbus` | Diagnostic | Service/microVM required | Passed |
| `prisma` | Application | `engine-boundary` | Diagnostic | Service/microVM required | Passed |
| `raw-server-listen` | Application | `builtin` | Diagnostic | Service/microVM required | Passed |
| `node:repl` | Application | `builtin` | Diagnostic | Service/microVM required | Passed |
| `sharp` | Application | `0.35.4` | Diagnostic | Service/microVM required | Passed |
| `node:worker_threads` | Application | `builtin` | Diagnostic | Service/microVM required | Passed |
| `jose` | Application | `6.2.3` | Support | Supported | Passed |
| `nanoid` | Application | `5.1.16` | Support | Supported | Passed |
| `node-platform-builtins` | Application | `builtin` | Support | Supported | Passed |
| `octokit` | Application | `5.0.5` | Support | Supported | Passed |
| `openai` | Application | `6.39.1` | Support | Supported | Passed |
| `resend` | Application | `6.12.4` | Support | Supported | Passed |
| `@slack/web-api` | Application | `7.16.0` | Support | Supported | Passed |
| `socket.io` | Application | `4.8.3` | Support | Supported | Passed |
| `stripe` | Application | `22.2.0` | Support | Supported | Passed |
| `undici` | Application | `6.28.0` | Support | Supported | Passed |
| `@upstash/redis` | Application | `1.38.0` | Support | Supported | Passed |
| `uuid` | Application | `14.0.0` | Support | Supported | Passed |
| `ai` | Application | `6.0.192` | Support | Supported | Passed |
| `zod` | Application | `4.4.3` | Support | Supported | Passed |
| `jest` | Tooling | `30.4.2` | Support | Supported | Passed |
| `next` | Tooling | `16.3.3` | Support | Supported | Passed |
| `prisma` | Tooling | `7.10.0` | Support | Supported | Passed |
| `ts-node` | Tooling | `10.9.2` | Support | Supported | Passed |
| `tsx` | Tooling | `4.21.0` | Support | Supported | Passed |

## Claim Boundary

This lane is supported only for the measured surfaces represented by its
passed fixtures, canaries, and explicit classifications.
Known gaps and expected failures are intentionally not support claims.
