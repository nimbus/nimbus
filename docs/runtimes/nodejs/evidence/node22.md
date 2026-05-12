# Node22 Runtime Evidence

This page is generated from the checked-in Node compatibility evidence snapshots.

## Summary

- role: `default`
- upstream fixture line: `v22.15.0`
- runtime execution target: `Node22`
- vendored official fixtures: `1283`
- passed official fixtures: `876`
- expected failure / known gap fixtures: `403`
- skipped / excluded fixtures: `4`
- unclassified fixtures: `0`
- official fixture pass rate: `68.3%`
- classified coverage: `100.0%`

## Classification Catalog

- catalog: `tests/runtime/node/classifications/node22.json`

| Expectation | Count |
| --- | ---: |
| Expected failure | 25 |
| Known gap | 378 |
| Skipped / excluded | 4 |

## Canary Coverage

| Package | Profile | Pinned version | Status |
| --- | --- | --- | --- |
| `axios` | Application | `1.7.7` | Passed |
| `express` | Application | `4.19.2` | Passed |
| `fastify` | Application | `4.28.1` | Passed |
| `socket.io` | Application | `4.7.5` | Passed |
| `undici` | Application | `6.19.8` | Passed |
| `jest` | Tooling | `30.4.2` | Passed |
| `next` | Tooling | `16.2.6` | Passed |
| `prisma` | Tooling | `7.8.0` | Passed |
| `ts-node` | Tooling | `10.9.2` | Passed |
| `tsx` | Tooling | `4.21.0` | Passed |

## Claim Boundary

This lane is supported only for the measured surfaces represented by its
passed fixtures, canaries, and explicit classifications. Known gaps and
expected failures are intentionally not support claims.
