# Node26 Runtime Evidence

This page is generated from the checked-in Node compatibility evidence snapshots.

## Summary

- role: `Current`
- support phase: `Current non-LTS`
- product default: `no`
- evidence policy: `Current non-LTS; promote to LTS support only after LTS and lane-local evidence`
- upstream fixture line: `v26.2.0`
- runtime execution target: `Node26`
- vendored official fixtures: `5578`
- passed official fixtures: `2092`
- expected failure / known gap fixtures: `3432`
- skipped / excluded fixtures: `54`
- unclassified fixtures: `0`
- official fixture pass rate: `37.5%`
- classified coverage: `100.0%`

## Classification Catalog

- catalog: `tests/runtime/node/classifications/node26.json`

| Expectation | Count |
| --- | ---: |
| Known gap | 3432 |
| Skipped / excluded | 54 |

## Canary Coverage

| Package | Preset | Pinned version | Evidence | Support boundary | Status |
| --- | --- | --- | --- | --- | --- |
| `@anthropic-ai/sdk` | Application | `0.100.0` | Support | Supported | Passed |
| `@aws-sdk/client-s3` | Application | `3.1056.0` | Support | Supported | Passed |
| `convex-use-node-real-app` | Application | `nimbus` | Support | Supported | Passed |
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
| `octokit` | Application | `5.0.5` | Support | Supported | Passed |
| `openai` | Application | `6.39.1` | Support | Supported | Passed |
| `resend` | Application | `6.12.4` | Support | Supported | Passed |
| `@slack/web-api` | Application | `7.16.0` | Support | Supported | Passed |
| `stripe` | Application | `22.2.0` | Support | Supported | Passed |
| `@upstash/redis` | Application | `1.38.0` | Support | Supported | Passed |
| `uuid` | Application | `14.0.0` | Support | Supported | Passed |
| `ai` | Application | `6.0.192` | Support | Supported | Passed |
| `zod` | Application | `4.4.3` | Support | Supported | Passed |

## Claim Boundary

This lane is supported only for the measured surfaces represented by its
passed fixtures, canaries, and explicit classifications.
Known gaps and expected failures are intentionally not support claims.
