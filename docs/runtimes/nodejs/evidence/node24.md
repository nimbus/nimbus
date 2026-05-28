# Node24 Runtime Evidence

This page is generated from the checked-in Node compatibility evidence snapshots.

## Summary

- role: `Supported; Active LTS`
- support phase: `Active LTS`
- product default: `no`
- evidence policy: `lane-local LTS evidence`
- upstream fixture line: `v24.15.0`
- runtime execution target: `Node24`
- vendored official fixtures: `1495`
- passed official fixtures: `922`
- expected failure / known gap fixtures: `570`
- skipped / excluded fixtures: `3`
- unclassified fixtures: `0`
- official fixture pass rate: `61.7%`
- classified coverage: `100.0%`

## Classification Catalog

- catalog: `tests/runtime/node/classifications/node24.json`

| Expectation | Count |
| --- | ---: |
| Expected failure | 32 |
| Known gap | 538 |
| Skipped / excluded | 3 |

## Canary Coverage

| Package | Preset | Pinned version | Status |
| --- | --- | --- | --- |
| `axios` | Application | `1.7.7` | Passed |
| `convex-use-node-action` | Application | `nimbus` | Passed |
| `express` | Application | `4.19.2` | Passed |
| `fastify` | Application | `4.28.1` | Passed |
| `node-platform-builtins` | Application | `builtin` | Passed |
| `socket.io` | Application | `4.7.5` | Passed |
| `undici` | Application | `6.19.8` | Passed |
| `jest` | Tooling | `30.4.2` | Passed |
| `next` | Tooling | `16.2.6` | Passed |
| `prisma` | Tooling | `7.8.0` | Passed |
| `ts-node` | Tooling | `10.9.2` | Passed |
| `tsx` | Tooling | `4.21.0` | Passed |

## Claim Boundary

This lane is supported only for the measured surfaces represented by its
passed fixtures, canaries, and explicit classifications.
Known gaps and expected failures are intentionally not support claims.
