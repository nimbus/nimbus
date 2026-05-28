# Node20 Runtime Evidence

This page is generated from the checked-in Node compatibility evidence snapshots.

## Summary

- role: `Legacy grace; EOL`
- support phase: `EOL legacy`
- product default: `no`
- evidence policy: `legacy-grace regression only`
- upstream fixture line: `v20.20.2`
- runtime execution target: `Node20`
- vendored official fixtures: `1308`
- passed official fixtures: `904`
- expected failure / known gap fixtures: `399`
- skipped / excluded fixtures: `5`
- unclassified fixtures: `0`
- official fixture pass rate: `69.1%`
- classified coverage: `100.0%`

## Classification Catalog

- catalog: `tests/runtime/node/classifications/node20.json`

| Expectation | Count |
| --- | ---: |
| Expected failure | 36 |
| Known gap | 363 |
| Skipped / excluded | 5 |

## Canary Coverage

| Package | Preset | Pinned version | Status |
| --- | --- | --- | --- |
| `express` | Application | `4.19.2` | Passed |
| `fastify` | Application | `4.28.1` | Passed |

## Claim Boundary

This lane remains selectable as legacy-grace regression coverage, but it
is not an active enterprise LTS support target after Node20 EOL on
2026-04-30.
Known gaps and expected failures are intentionally not support claims.
